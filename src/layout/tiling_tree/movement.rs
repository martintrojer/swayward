use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn swap_nodes(&mut self, first: NodeId, second: NodeId) -> Result<(), &'static str> {
        self.check_swappable(first, second)?;
        // The workspace root is not a container (sway's swap only accepts
        // containers and views, sway/commands/swap.c:73-75); its missing
        // parent is what the lookups below would trip over.
        let (Some(first_parent), Some(second_parent)) = (
            self.nodes.get(&first).and_then(|node| node.parent),
            self.nodes.get(&second).and_then(|node| node.parent),
        ) else {
            return Err("Can only swap with containers and views");
        };
        let (Some(first_index), Some(second_index)) = (
            self.child_index(first_parent, first),
            self.child_index(second_parent, second),
        ) else {
            return Err("No matching node.");
        };
        let old = self.compute_geometry();
        let focus_after_swap = self.focus_after_swap(first, first_parent, second, second_parent);
        // Both parents and indices were resolved above and nothing has mutated the arena since.
        // Writing each slot works for a shared parent too: the two indices simply trade nodes.
        self.replace_child(first_parent, first_index, second);
        self.replace_child(second_parent, second_index, first);
        for (id, parent) in [(first, second_parent), (second, first_parent)] {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.parent = Some(parent);
            }
        }
        self.swap_fullscreen_modes(first, second);
        if self.focus != focus_after_swap {
            self.set_focus_id(focus_after_swap);
        }
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        Ok(())
    }

    fn check_swappable(&self, first: NodeId, second: NodeId) -> Result<(), &'static str> {
        if !self.nodes.contains_key(&first) || !self.nodes.contains_key(&second) {
            return Err("No matching node.");
        }
        if first == second {
            return Err("Cannot swap a container with itself");
        }
        if self.contains_node(first, second) || self.contains_node(second, first) {
            return Err("Cannot swap ancestor and descendant");
        }
        if !self.fits_at(first, self.subtree_height(second))
            || !self.fits_at(second, self.subtree_height(first))
        {
            return Err(TOO_DEEP);
        }
        Ok(())
    }

    /// A focused node moving into a tabbed or stacked parent keeps the focus with it, so that
    /// parent shows it; otherwise focus stays where it is.
    fn focus_after_swap(
        &self,
        first: NodeId,
        first_parent: NodeId,
        second: NodeId,
        second_parent: NodeId,
    ) -> Option<NodeId> {
        let parent_is_tabbed = |parent| {
            matches!(
                self.nodes.get(&parent).map(|node| &node.value),
                Some(TreeNode::Split {
                    layout: Layout::Tabbed | Layout::Stacked,
                    ..
                })
            )
        };
        if self.focus == Some(first) && parent_is_tabbed(second_parent) {
            Some(second)
        } else if self.focus == Some(second) && parent_is_tabbed(first_parent) {
            Some(first)
        } else {
            self.focus
        }
    }

    fn replace_child(&mut self, parent: NodeId, index: usize, child: NodeId) {
        if let Some(TreeNode::Split { children, .. }) =
            self.nodes.get_mut(&parent).map(|node| &mut node.value)
        {
            if let Some(slot) = children.get_mut(index) {
                *slot = child;
            }
        }
    }

    fn swap_fullscreen_modes(&mut self, first: NodeId, second: NodeId) {
        let first_fullscreen = self
            .pending_modes
            .get(&first)
            .and_then(|mode| mode.fullscreen);
        let second_fullscreen = self
            .pending_modes
            .get(&second)
            .and_then(|mode| mode.fullscreen);
        for (id, fullscreen) in [(first, second_fullscreen), (second, first_fullscreen)] {
            self.set_pending_fullscreen(id, fullscreen);
        }
    }

    pub fn move_subtree_to_node(&mut self, id: NodeId, destination: NodeId) -> bool {
        if id == self.root
            || id == destination
            || !self.nodes.contains_key(&id)
            || !self.nodes.contains_key(&destination)
            || self.contains_node(id, destination)
        {
            return false;
        }
        let moved = self
            .leaf_ids_in(id)
            .into_iter()
            .filter_map(|leaf| self.tile(leaf).map(|tile| tile.window().id().clone()))
            .collect::<Vec<_>>();
        let (parent, after) = match self.nodes.get(&destination) {
            Some(Node {
                parent: Some(parent),
                value: TreeNode::Leaf { .. },
            }) => (*parent, Some(destination)),
            Some(Node {
                value: TreeNode::Split { .. },
                ..
            }) => (destination, None),
            _ => return false,
        };
        if !self.fits_below(parent, self.subtree_height(id)) {
            return false;
        }
        let already_there = self.nodes.get(&id).and_then(|node| node.parent) == Some(parent)
            && self.child_index(parent, id)
                == after
                    .and_then(|node| self.child_index(parent, node))
                    .map(|index| index + 1);
        if already_there
            && self
                .focus
                .is_some_and(|focus| self.contains_node(id, focus))
        {
            return true;
        }
        let old = self.compute_geometry();
        let Some(old_parent) = self.detach_subtree_only(id) else {
            return false;
        };
        self.insert_child(parent, id, after);
        self.reap_empty_from(old_parent);
        self.compact_tree();
        self.reinsert_focus_history(moved, usize::from(self.focus.is_some()));
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        true
    }

    pub fn move_direction(&mut self, id: NodeId, direction: Direction) -> bool {
        let old = self.compute_geometry();
        let changed = self.move_direction_inner(id, direction);
        if changed {
            self.compact_tree();
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    pub fn move_window_direction(&mut self, window: &W::Id, direction: Direction) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        self.move_node_direction(id, direction)
    }

    pub fn move_node_direction(&mut self, id: NodeId, direction: Direction) -> bool {
        let focus = self.focus;
        let focus_history = self.window_focus_history();
        let moved_focus = focus.is_some_and(|focus| self.contains_node(id, focus));
        let containers_before = self
            .focus_history
            .iter()
            .copied()
            .filter(|node| self.is_split(*node))
            .collect::<Vec<_>>();
        let changed = self.move_direction(id, direction);
        if changed {
            self.restore_window_focus_history(focus_history);
            // Sway's seat focus stack also holds containers; keep the surviving ones after the
            // windows so `seat_get_focus_inactive` can still find a container that was focused
            // on its own.
            for node in containers_before {
                if self.nodes.contains_key(&node) && !self.focus_history.contains(&node) {
                    self.focus_history.push(node);
                }
            }
        }
        // Restore focus that was elsewhere, unless the move reaped that node.
        // Moving a window out of its focused singleton parent removes the
        // parent, and `contains_node(id, focus)` is false because the focus
        // was an ancestor of the moved node rather than inside it.
        if !moved_focus && focus.is_some_and(|focus| self.nodes.contains_key(&focus)) {
            self.focus = focus;
        }
        changed
    }

    fn move_direction_inner(&mut self, id: NodeId, direction: Direction) -> bool {
        if !self.nodes.contains_key(&id) || id == self.root {
            return false;
        }
        if self
            .fullscreen_node()
            .is_some_and(|fullscreen| self.contains_node(fullscreen, id))
        {
            return false;
        }
        self.interactive_resize = None;
        let wanted_layout = direction.axis();
        let boundary_root = self.resident_root().unwrap_or(self.root);
        if self.windows().nth(1).is_none() {
            if boundary_root == self.root {
                self.move_only_window(id, direction, wanted_layout);
            }
            return false;
        }
        if self.split_len(self.root) == Some(1) && self.root_branch(id) == Some(id) {
            self.set_layout(self.root, wanted_layout);
            return false;
        }
        let backwards = direction.is_backwards();
        let mut branch = id;
        let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
        while let Some(parent_id) = parent {
            let Some(Node {
                parent: grandparent,
                value: TreeNode::Split {
                    layout, children, ..
                },
            }) = self.nodes.get(&parent_id)
            else {
                return false;
            };
            if Self::layouts_parallel(*layout, wanted_layout) {
                let Some(index) = children.iter().position(|child| *child == branch) else {
                    return false;
                };
                let destination = if backwards {
                    index.checked_sub(1).and_then(|index| children.get(index))
                } else {
                    children.get(index + 1)
                }
                .copied();
                if let Some(destination) = destination {
                    if branch == id
                        && matches!(
                            self.nodes.get(&destination).map(|node| &node.value),
                            Some(TreeNode::Leaf { .. })
                        )
                    {
                        let new_index = if backwards { index - 1 } else { index + 1 };
                        return self.move_subtree_to_index_inner(id, new_index);
                    }
                    return self.move_into_directional_destination(
                        id,
                        destination,
                        direction,
                        false,
                    );
                }
                if parent_id == boundary_root {
                    if branch == id {
                        return false;
                    }
                    // A window whose parent is a singleton workspace child
                    // counts as workspace level, like i3, and stays put
                    // (sway/commands/move.c:387-393).
                    let id_parent = self.nodes.get(&id).and_then(|node| node.parent);
                    if boundary_root == self.root
                        && id_parent == Some(branch)
                        && self.split_len(branch) == Some(1)
                    {
                        return false;
                    }
                    return self.promote_to_boundary(id, boundary_root, backwards);
                }
            }
            branch = parent_id;
            parent = *grandparent;
        }
        if boundary_root != self.root || !self.can_wrap_root_children() {
            return false;
        }
        self.promote_by_wrapping_root(id, direction)
    }

    /// Promotes `id` to the outer end of `boundary_root`, a parallel root it has escaped
    /// ("Container will be promoted", sway/commands/move.c:394-412).
    fn promote_to_boundary(&mut self, id: NodeId, boundary_root: NodeId, backwards: bool) -> bool {
        let Some(TreeNode::Split { children, .. }) =
            self.nodes.get(&boundary_root).map(|node| &node.value)
        else {
            return false;
        };
        let Some(boundary) = children
            .get(if backwards { 0 } else { children.len() - 1 })
            .copied()
        else {
            return false;
        };
        let insert_index = if backwards { 0 } else { children.len() };
        let Some(old_parent) = self.detach_subtree_only(id) else {
            return false;
        };
        self.insert_existing_child(boundary_root, id, insert_index, boundary);
        self.reap_empty_from(old_parent);
        self.compact_tree();
        self.finish_directional_move(id);
        true
    }

    /// No ancestor runs along the move axis, so the root's children are wrapped and `id` becomes
    /// their sibling along that axis (sway/commands/move.c:333-344).
    fn promote_by_wrapping_root(&mut self, id: NodeId, direction: Direction) -> bool {
        let Some(old_parent) = self.detach_subtree_only(id) else {
            return false;
        };
        self.wrap_root_for_direction(id, direction);
        // Sway reaps only the emptied ancestors of the moved node
        // (`container_reap_empty`); a wrapper left with one view is not a
        // squashable H/V pair, so it survives (`container_squash`,
        // sway/tree/container.c:1686-1716).
        self.reap_empty_from(old_parent);
        // The wrap zeroes the moved container's fractions
        // (sway/commands/move.c:341-342).
        self.squash_for_move(Some(id));
        self.finish_directional_move(id);
        true
    }

    fn move_only_window(&mut self, id: NodeId, direction: Direction, wanted_layout: Layout) {
        let Some(&TreeNode::Split {
            layout: root_layout,
            ..
        }) = self.nodes.get(&self.root).map(|node| &node.value)
        else {
            return;
        };
        if !Self::layouts_parallel(root_layout, wanted_layout) {
            self.set_layout(self.root, wanted_layout);
            // `set_layout` compacts the tree, which squashes a singleton split.
            // When the moved node was that split, continue with the one window
            // that survives the compaction.
            let Some(id) = self
                .nodes
                .contains_key(&id)
                .then_some(id)
                .or_else(|| self.windows().next().map(|(leaf, _)| leaf))
            else {
                return;
            };
            let old_parent = self.nodes.get(&id).and_then(|node| node.parent);
            if let Some(parent) = old_parent.filter(|parent| *parent != self.root) {
                self.detach_subtree_only(id);
                self.insert_child_at(self.root, id, 0);
                self.reap_empty_from(parent);
                self.finish_directional_move(id);
            }
            return;
        }
        // A same-axis command cannot move the only window out of the
        // workspace, but sway still walks up to the first ancestor whose
        // parent lays out along the move axis and promotes the window beside
        // that ancestor, then reaps the emptied wrappers and squashes the
        // workspace (`container_move_in_direction`, sway/commands/move.c:322-412).
        // A window whose parent is a singleton workspace child counts as
        // already at workspace level and stays put
        // (sway/commands/move.c:387-393).
        let Some(leaf) = self.windows().next().map(|(leaf, _)| leaf) else {
            return;
        };
        let Some(old_parent) = self.nodes.get(&leaf).and_then(|node| node.parent) else {
            return;
        };
        if old_parent == self.root {
            return;
        }
        // Sway's ancestor walk: climb while the parent's layout is not along
        // the move axis; at a parallel parent, the window itself escapes
        // (it has no sibling in the move direction), and any other node is
        // the ancestor to promote beside (sway/commands/move.c:322-380).
        let mut current = leaf;
        let ancestor = loop {
            let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) else {
                return;
            };
            let Some(&TreeNode::Split { layout, .. }) =
                self.nodes.get(&parent).map(|node| &node.value)
            else {
                return;
            };
            if !Self::layouts_parallel(layout, wanted_layout) || current == leaf {
                if parent == self.root {
                    // Reached workspace level without a parallel ancestor:
                    // the window is already as far out as it can go.
                    return;
                }
                current = parent;
                continue;
            }
            break current;
        };
        if old_parent != self.root
            && self.nodes.get(&old_parent).and_then(|node| node.parent) == Some(self.root)
            && self.split_len(old_parent) == Some(1)
        {
            // Treat a singleton workspace child as workspace level, like i3
            // (sway/commands/move.c:387-393).
            return;
        }
        let Some(destination) = self.nodes.get(&ancestor).and_then(|node| node.parent) else {
            return;
        };
        let Some(index) = self.child_index(destination, ancestor) else {
            return;
        };
        let forwards = matches!(direction, Direction::Right | Direction::Down);
        self.detach_subtree_only(leaf);
        // `container_insert_child(ancestor->parent, container, index + (offs < 0 ? 0 : 1))`
        // (sway/commands/move.c:394-412).
        self.insert_child_at(destination, leaf, index + usize::from(forwards));
        self.reap_empty_from(old_parent);
        self.compact_tree();
        self.finish_directional_move(leaf);
    }

    fn move_into_directional_destination(
        &mut self,
        id: NodeId,
        destination: NodeId,
        direction: Direction,
        descended_perpendicularly: bool,
    ) -> bool {
        let wanted_layout = direction.axis();
        let backwards = direction.is_backwards();
        match self.nodes.get(&destination).map(|node| &node.value) {
            Some(TreeNode::Leaf { .. }) => {
                let Some(parent) = self.nodes.get(&destination).and_then(|node| node.parent) else {
                    return false;
                };
                let Some(index) = self.child_index(parent, destination) else {
                    return false;
                };
                let Some(old_parent) = self.detach_subtree_only(id) else {
                    return false;
                };
                self.insert_child_at(
                    parent,
                    id,
                    index + usize::from(backwards || descended_perpendicularly),
                );
                self.reap_empty_from(old_parent);
            }
            Some(TreeNode::Split {
                layout, children, ..
            }) if Self::layouts_parallel(*layout, wanted_layout) => {
                if children.is_empty() {
                    return false;
                }
                let index = if backwards { children.len() } else { 0 };
                let Some(old_parent) = self.detach_subtree_only(id) else {
                    return false;
                };
                self.insert_child_at(destination, id, index);
                self.reap_empty_from(old_parent);
            }
            Some(TreeNode::Split { .. }) => {
                let Some(child) = self.focused_child_in(destination) else {
                    return false;
                };
                return self.move_into_directional_destination(id, child, direction, true);
            }
            None => return false,
        }
        self.compact_tree();
        self.finish_directional_move(id);
        true
    }

    fn finish_directional_move(&mut self, id: NodeId) {
        self.set_focus_id(self.first_leaf_in(id).or(self.focus));
        self.request_window_sizes();
    }

    pub fn move_subtree_to_first(&mut self, id: NodeId) -> bool {
        self.move_subtree_to_index(id, 0)
    }

    pub fn move_subtree_to_last(&mut self, id: NodeId) -> bool {
        self.move_subtree_to_index(id, usize::MAX)
    }

    pub fn move_subtree_to_index(&mut self, id: NodeId, index: usize) -> bool {
        let old = self.compute_geometry();
        let changed = self.move_subtree_to_index_inner(id, index);
        if changed {
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    fn move_subtree_to_index_inner(&mut self, id: NodeId, index: usize) -> bool {
        self.interactive_resize = None;
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        let Some(old_index) = children.iter().position(|child| *child == id) else {
            return false;
        };
        let new_index = index.min(children.len() - 1);
        if old_index == new_index {
            return false;
        }
        let child = children.remove(old_index);
        let percent = percents.remove(old_index);
        children.insert(new_index, child);
        percents.insert(new_index, percent);
        self.set_focus_id(self.first_leaf_in(id).or(self.focus));
        self.request_window_sizes();
        true
    }

    pub fn move_left(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Left))
    }

    pub fn move_right(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Right))
    }

    pub fn move_up(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Up))
    }

    pub fn move_down(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Down))
    }

    pub fn move_focused_to_first(&mut self) -> bool {
        self.focus
            .and_then(|id| self.root_branch(id))
            .is_some_and(|id| self.move_subtree_to_first(id))
    }

    pub fn move_focused_to_last(&mut self) -> bool {
        self.focus
            .and_then(|id| self.root_branch(id))
            .is_some_and(|id| self.move_subtree_to_last(id))
    }

    pub fn move_focused_to_index(&mut self, index: usize) -> bool {
        self.focus
            .and_then(|id| self.root_branch(id))
            .is_some_and(|id| self.move_subtree_to_index(id, index))
    }

    /// Expels `window` (or the focused leaf) from its split toward `right`, or consumes the
    /// neighbour into it when there is nothing to expel. This backs niri's consume-or-expel
    /// actions.
    pub fn expel_or_consume(&mut self, window: Option<&W::Id>, right: bool) {
        let id = window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus);
        if let Some(id) = id {
            self.set_focus_id(Some(id));
            if !self.expel(id, right) {
                self.consume(id, right);
            }
        }
    }

    pub fn consume_focused(&mut self) {
        if let Some(id) = self.focus {
            self.consume(id, true);
        }
    }

    pub fn expel_focused(&mut self) {
        if let Some(id) = self.focus {
            self.expel(id, true);
        }
    }

    pub(super) fn consume(&mut self, id: NodeId, right: bool) -> bool {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        let Some(index) = self.child_index(parent, id) else {
            return false;
        };
        let sibling_index = if right {
            Some(index + 1)
        } else {
            index.checked_sub(1)
        };
        let Some(sibling) = sibling_index.and_then(|index| self.child_at(parent, index)) else {
            return false;
        };
        // The new wrapper holds both `id` and its sibling one level deeper.
        if !self.can_wrap(id) || !self.can_wrap(sibling) {
            return false;
        }

        self.interactive_resize = None;
        let old = self.compute_geometry();
        self.remove_child(parent, id);
        if !self.wrap_pair(parent, sibling, id, right) {
            return false;
        }
        self.compact_tree();
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        true
    }

    /// Replaces `sibling` in `parent` with a new SplitV holding `sibling` and the detached `id`,
    /// `id` after the sibling when `right`. The wrapper takes the sibling's share.
    fn wrap_pair(&mut self, parent: NodeId, sibling: NodeId, id: NodeId, right: bool) -> bool {
        let Some(sibling_percent) = self
            .child_index(parent, sibling)
            .and_then(|index| match &self.nodes.get(&parent)?.value {
                TreeNode::Split { percents, .. } => percents.get(index).copied(),
                TreeNode::Leaf { .. } => None,
            })
        else {
            return false;
        };
        let wrapper = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Split {
                layout: Layout::SplitV,
                children: if right {
                    vec![sibling, id]
                } else {
                    vec![id, sibling]
                },
                percents: vec![0.5, 0.5],
                meta: SplitMeta::default(),
            },
        });
        if let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        {
            if let Some(index) = children.iter().position(|child| *child == sibling) {
                if let (Some(child), Some(percent)) =
                    (children.get_mut(index), percents.get_mut(index))
                {
                    *child = wrapper;
                    *percent = sibling_percent;
                }
            }
        }
        self.nodes
            .get_mut(&sibling)
            .expect("invariant: a sibling child remains in the arena while it is wrapped")
            .parent = Some(wrapper);
        self.nodes
            .get_mut(&id)
            .expect("invariant: the consumed node remains in the arena while it is wrapped")
            .parent = Some(wrapper);
        true
    }

    pub(super) fn expel(&mut self, id: NodeId, after: bool) -> bool {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        let Some(grandparent) = self.nodes.get(&parent).and_then(|node| node.parent) else {
            return false;
        };
        self.interactive_resize = None;
        let old = self.compute_geometry();
        let Some(parent_index) = self.child_index(grandparent, parent) else {
            return false;
        };
        self.remove_child(parent, id);
        let index = parent_index + usize::from(after);
        self.insert_existing_child(grandparent, id, index, parent);
        self.collapse_from(parent);
        self.compact_tree();
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        true
    }

    pub(super) fn wrap_root_for_direction(&mut self, id: NodeId, direction: Direction) {
        let layout = direction.axis();
        let mut old_value = std::mem::replace(
            &mut self
                .nodes
                .get_mut(&self.root)
                .expect("invariant: the root is always present in the arena")
                .value,
            TreeNode::Split {
                layout,
                children: Vec::new(),
                percents: Vec::new(),
                meta: SplitMeta::default(),
            },
        );
        // The root's metadata belongs to the workspace and stays with it; the container that
        // takes over the root's children starts fresh.
        let root_meta = match &mut old_value {
            TreeNode::Split { meta, .. } => std::mem::take(meta),
            TreeNode::Leaf { .. } => SplitMeta::default(),
        };
        let old = self.alloc(Node {
            parent: Some(self.root),
            value: old_value,
        });
        if let TreeNode::Split { children, .. } = &self
            .nodes
            .get(&old)
            .expect("invariant: the freshly allocated old root remains in the arena")
            .value
        {
            for child in children.clone() {
                self.nodes
                    .get_mut(&child)
                    .expect("invariant: every split child is present in the arena")
                    .parent = Some(old);
            }
        }
        self.nodes
            .get_mut(&old)
            .expect("invariant: the freshly allocated old root remains in the arena")
            .parent = Some(self.root);
        let moving_first = direction.is_backwards();
        let (children, percents) = if moving_first {
            (vec![id, old], vec![0.5, 0.5])
        } else {
            (vec![old, id], vec![0.5, 0.5])
        };
        self.nodes
            .get_mut(&id)
            .expect("invariant: a node detached for a root wrap remains in the arena")
            .parent = Some(self.root);
        self.nodes
            .get_mut(&self.root)
            .expect("invariant: the root is always present in the arena")
            .value = TreeNode::Split {
            layout,
            children,
            percents,
            meta: root_meta,
        };
    }
}

use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn swap_nodes(&mut self, first: NodeId, second: NodeId) -> Result<(), &'static str> {
        if !self.nodes.contains_key(&first) || !self.nodes.contains_key(&second) {
            return Err("No matching node.");
        }
        if first == second {
            return Err("Cannot swap a container with itself");
        }
        if self.contains_node(first, second) || self.contains_node(second, first) {
            return Err("Cannot swap ancestor and descendant");
        }

        let old = self.compute_geometry();
        let first_parent = self.nodes[&first].parent.unwrap();
        let second_parent = self.nodes[&second].parent.unwrap();
        let first_index = self.child_index(first_parent, first).unwrap();
        let second_index = self.child_index(second_parent, second).unwrap();
        let parent_is_tabbed = |parent| {
            matches!(
                self.nodes[&parent].value,
                TreeNode::Split {
                    layout: Layout::Tabbed | Layout::Stacked,
                    ..
                }
            )
        };
        let focus_after_swap = if self.focus == Some(first) && parent_is_tabbed(second_parent) {
            Some(second)
        } else if self.focus == Some(second) && parent_is_tabbed(first_parent) {
            Some(first)
        } else {
            self.focus
        };
        if first_parent == second_parent {
            let TreeNode::Split { children, .. } =
                &mut self.nodes.get_mut(&first_parent).unwrap().value
            else {
                unreachable!();
            };
            children.swap(first_index, second_index);
        } else {
            let TreeNode::Split { children, .. } =
                &mut self.nodes.get_mut(&first_parent).unwrap().value
            else {
                unreachable!();
            };
            children[first_index] = second;
            let TreeNode::Split { children, .. } =
                &mut self.nodes.get_mut(&second_parent).unwrap().value
            else {
                unreachable!();
            };
            children[second_index] = first;
        }
        self.nodes.get_mut(&first).unwrap().parent = Some(second_parent);
        self.nodes.get_mut(&second).unwrap().parent = Some(first_parent);
        let first_mode = self.pending_modes.remove(&first);
        let second_mode = self.pending_modes.remove(&second);
        if let Some(mode) = first_mode {
            self.pending_modes.insert(second, mode);
        }
        if let Some(mode) = second_mode {
            self.pending_modes.insert(first, mode);
        }
        if self.focus != focus_after_swap {
            self.set_focus_id(focus_after_swap);
        }
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        Ok(())
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
        let (parent, after) = match self.nodes[&destination] {
            Node {
                parent: Some(parent),
                value: TreeNode::Leaf { .. },
            } => (parent, Some(destination)),
            Node {
                value: TreeNode::Split { .. },
                ..
            } => (destination, None),
            _ => return false,
        };
        let already_there = self.nodes[&id].parent == Some(parent)
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
        let insertion = usize::from(self.focus.is_some());
        for window in moved.into_iter().rev() {
            let Some(leaf) = self.node_for_window(&window) else {
                continue;
            };
            self.focus_history.retain(|candidate| *candidate != leaf);
            self.focus_history
                .insert(insertion.min(self.focus_history.len()), leaf);
        }
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
        let changed = self.move_direction(id, direction);
        if changed {
            self.restore_window_focus_history(focus_history);
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
        let wanted_layout = match direction {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        };
        if self.windows().nth(1).is_none()
            || self.split_len(self.root) == Some(1) && self.root_branch(id) == Some(id)
        {
            self.set_layout(self.root, wanted_layout);
            return false;
        }
        let backwards = matches!(direction, Direction::Left | Direction::Up);
        let mut branch = id;
        let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
        let mut found_axis = false;
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
                found_axis = true;
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
                if parent_id == self.root && branch != id {
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
                    self.insert_existing_child(self.root, id, insert_index, boundary);
                    self.reap_empty_from(old_parent);
                    self.compact_tree();
                    self.finish_directional_move(id);
                    return true;
                }
            }
            branch = parent_id;
            parent = *grandparent;
        }
        if found_axis {
            return false;
        }
        let Some(old_parent) = self.detach_subtree_only(id) else {
            return false;
        };
        self.wrap_root_for_direction(id, direction);
        self.reap_empty_from(old_parent);
        self.compact_tree();
        self.finish_directional_move(id);
        true
    }

    fn move_into_directional_destination(
        &mut self,
        id: NodeId,
        destination: NodeId,
        direction: Direction,
        descended_perpendicularly: bool,
    ) -> bool {
        let wanted_layout = match direction {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        };
        let backwards = matches!(direction, Direction::Left | Direction::Up);
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

    pub fn nest_or_unnest_window_left(&mut self, window: Option<&W::Id>) {
        let id = window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus);
        if let Some(id) = id {
            self.set_focus_id(Some(id));
            if !self.expel(id, false) {
                self.consume(id, false);
            }
        }
    }

    pub fn nest_or_unnest_window_right(&mut self, window: Option<&W::Id>) {
        let id = window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus);
        if let Some(id) = id {
            self.set_focus_id(Some(id));
            if !self.expel(id, true) {
                self.consume(id, true);
            }
        }
    }

    pub fn nest_focused_window(&mut self) {
        if let Some(id) = self.focus {
            self.consume(id, true);
        }
    }

    pub fn unnest_focused_window(&mut self) {
        if let Some(id) = self.focus {
            self.expel(id, true);
        }
    }

    pub fn swap_window_horizontal(&mut self, right: bool) {
        if right {
            self.move_right();
        } else {
            self.move_left();
        }
    }
}

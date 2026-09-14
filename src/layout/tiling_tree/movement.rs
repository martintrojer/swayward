use super::*;

impl<W: LayoutElement> TilingTree<W> {
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
        let changed = self.move_direction(id, direction);
        if changed {
            self.restore_window_focus_history(focus_history);
        }
        self.focus = focus;
        changed
    }

    fn move_direction_inner(&mut self, id: NodeId, direction: Direction) -> bool {
        if !self.nodes.contains_key(&id) || id == self.root || self.windows().nth(1).is_none() {
            return false;
        }
        if self
            .fullscreen_node()
            .is_some_and(|fullscreen| self.is_descendant(id, fullscreen))
        {
            return false;
        }
        self.interactive_resize = None;
        let wanted_layout = match direction {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        };
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

    pub fn move_column_to_first(&mut self) {
        self.move_focused_to_first();
    }

    pub fn move_column_to_last(&mut self) {
        self.move_focused_to_last();
    }

    pub fn move_column_to_index(&mut self, index: usize) {
        self.move_focused_to_index(index.saturating_sub(1));
    }

    pub fn consume_or_expel_window_left(&mut self, window: Option<&W::Id>) {
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

    pub fn consume_or_expel_window_right(&mut self, window: Option<&W::Id>) {
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

    pub fn consume_into_column(&mut self) {
        if let Some(id) = self.focus {
            self.consume(id, true);
        }
    }

    pub fn expel_from_column(&mut self) {
        if let Some(id) = self.focus {
            self.expel(id, true);
        }
    }

    pub fn swap_window_in_direction(&mut self, direction: ScrollDirection) {
        match direction {
            ScrollDirection::Left => {
                self.move_left();
            }
            ScrollDirection::Right => {
                self.move_right();
            }
        }
    }
}

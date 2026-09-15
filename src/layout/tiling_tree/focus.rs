use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn focus(&self) -> Option<NodeId> {
        self.focus
    }

    pub fn focus_rank_for_window(&self, window: &W::Id) -> Option<usize> {
        let node = self.node_for_window(window)?;
        self.focus_history
            .iter()
            .position(|candidate| *candidate == node)
    }

    pub fn non_root_parent_for_window(&self, window: &W::Id) -> Option<NodeId> {
        let node = self.node_for_window(window)?;
        self.nodes
            .get(&node)?
            .parent
            .filter(|parent| *parent != self.root && self.split_len(*parent).is_some_and(|n| n > 1))
    }

    pub fn restore_focus_rank(&mut self, window: &W::Id, rank: usize) {
        let Some(node) = self.node_for_window(window) else {
            return;
        };
        self.focus_history.retain(|candidate| *candidate != node);
        self.focus_history
            .insert(rank.min(self.focus_history.len()), node);
    }

    pub(super) fn window_focus_history(&self) -> Vec<W::Id> {
        self.focus_history
            .iter()
            .filter_map(|node| self.tile(*node).map(|tile| tile.window().id().clone()))
            .collect()
    }

    pub(super) fn restore_window_focus_history(&mut self, history: Vec<W::Id>) {
        self.focus_history = history
            .into_iter()
            .filter_map(|window| self.node_for_window(&window))
            .collect();
    }

    pub(crate) fn sort_focus_history_by_timestamp(&mut self) {
        let mut history = self
            .focus_history
            .iter()
            .filter_map(|node| {
                self.tile(*node)
                    .map(|tile| (*node, tile.window().focus_timestamp()))
            })
            .collect::<Vec<_>>();
        history.sort_by_key(|(_, timestamp)| std::cmp::Reverse(*timestamp));
        self.focus_history = history.into_iter().map(|(node, _)| node).collect();
    }

    pub fn root_is_focused(&self) -> bool {
        self.focus == Some(self.root)
    }

    pub fn is_root(&self, id: NodeId) -> bool {
        id == self.root
    }

    pub fn focus_root(&mut self) {
        self.set_focus_id(Some(self.root));
    }

    pub fn set_focus(&mut self, id: NodeId) {
        if self.nodes.contains_key(&id) {
            self.set_focus_id(Some(id));
        }
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn is_split(&self, id: NodeId) -> bool {
        matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Split { .. })
        )
    }

    pub fn focus_parent(&mut self) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        if self.fullscreen_node() == Some(focus) {
            return false;
        }
        let Some(parent) = self.nodes.get(&focus).and_then(|node| node.parent) else {
            return false;
        };
        self.set_focus_id(Some(parent));
        true
    }

    pub fn focus_child(&mut self) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let child = self.focused_child_in(focus);
        if let Some(child) = child {
            self.set_focus_id(Some(child));
            true
        } else {
            false
        }
    }

    pub fn focus_from_output_direction(&mut self, dir: Direction) -> bool {
        let target = if let Some(fullscreen) = self.fullscreen_node() {
            self.focused_leaf_in(fullscreen)
        } else {
            let TreeNode::Split {
                layout, children, ..
            } = &self.nodes[&self.root].value
            else {
                return false;
            };
            let matching_axis = matches!(
                (dir, layout),
                (
                    Direction::Left | Direction::Right,
                    Layout::SplitH | Layout::Tabbed
                ) | (
                    Direction::Up | Direction::Down,
                    Layout::SplitV | Layout::Stacked
                )
            );
            if matching_axis {
                let branch = if matches!(dir, Direction::Left | Direction::Up) {
                    children.last()
                } else {
                    children.first()
                };
                branch.and_then(|branch| self.focused_leaf_in(*branch))
            } else {
                self.focused_leaf_in(self.root)
            }
        };
        self.set_focus_id(target);
        target.is_some()
    }

    pub fn focus_next_prev_sibling(&mut self, next: bool) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let Some(parent) = self.nodes.get(&focus).and_then(|node| node.parent) else {
            return false;
        };
        let TreeNode::Split { layout, .. } = &self.nodes[&parent].value else {
            return false;
        };
        let direction_layout = match layout {
            Layout::SplitH | Layout::Tabbed => Layout::SplitH,
            Layout::SplitV | Layout::Stacked => Layout::SplitV,
        };
        let mut current = focus;
        let mut wrap = None;
        while let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) {
            let TreeNode::Split {
                layout, children, ..
            } = &self.nodes[&parent].value
            else {
                return false;
            };
            if Self::layouts_parallel(*layout, direction_layout) {
                let index = children.iter().position(|child| *child == current).unwrap();
                let target = if next {
                    children.get(index + 1).copied()
                } else {
                    index
                        .checked_sub(1)
                        .and_then(|index| children.get(index).copied())
                };
                if target.is_some() {
                    self.set_focus_id(target);
                    return true;
                }
                if children.len() > 1 && wrap.is_none() {
                    wrap = if next {
                        children.first().copied()
                    } else {
                        children.last().copied()
                    };
                    if self.options.layout.focus_wrapping == swayward_config::FocusWrapping::Force {
                        self.set_focus_id(wrap);
                        return true;
                    }
                }
            }
            current = parent;
        }
        if wrap.is_some() {
            self.set_focus_id(wrap);
        }
        wrap.is_some()
    }

    pub fn focus_next_or_prev(&mut self, next: bool) -> bool {
        let Some(parent) = self
            .focus
            .and_then(|focus| self.nodes.get(&focus))
            .and_then(|node| node.parent)
        else {
            return false;
        };
        let TreeNode::Split { layout, .. } = self.nodes[&parent].value else {
            return false;
        };
        let direction = match (next, layout) {
            (false, Layout::SplitH | Layout::Tabbed) => Direction::Left,
            (true, Layout::SplitH | Layout::Tabbed) => Direction::Right,
            (false, Layout::SplitV | Layout::Stacked) => Direction::Up,
            (true, Layout::SplitV | Layout::Stacked) => Direction::Down,
        };
        self.focus_direction(direction)
    }

    pub fn focus_direction(&mut self, dir: Direction) -> bool {
        let Some(mut current) = self.focus else {
            return false;
        };
        let barrier = self.fullscreen_node();
        let direction_layout = match dir {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        };
        let backwards = matches!(dir, Direction::Left | Direction::Up);
        let mut wrap = None;

        while let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) {
            let TreeNode::Split {
                layout, children, ..
            } = &self.nodes[&parent].value
            else {
                return false;
            };
            if Self::layouts_parallel(*layout, direction_layout) {
                let Some(index) = children.iter().position(|child| *child == current) else {
                    return false;
                };
                let desired = if backwards {
                    index.checked_sub(1)
                } else {
                    children.get(index + 1).map(|_| index + 1)
                };
                if let Some(desired) = desired {
                    let next = self.focused_leaf_in(children[desired]);
                    self.set_focus_id(next);
                    return next.is_some();
                }
                if children.len() > 1 {
                    let candidate = if backwards {
                        children.last().copied()
                    } else {
                        children.first().copied()
                    };
                    if self.options.layout.focus_wrapping == swayward_config::FocusWrapping::Force {
                        let next = candidate.and_then(|id| self.focused_leaf_in(id));
                        self.set_focus_id(next.or(self.focus));
                        return next.is_some();
                    }
                    wrap.get_or_insert(candidate.unwrap());
                }
            }
            if Some(parent) == barrier {
                break;
            }
            current = parent;
        }

        let next = wrap.and_then(|id| self.focused_leaf_in(id));
        self.set_focus_id(next.or(self.focus));
        next.is_some()
    }

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> {
        self.iter_depth_first().filter_map(|(_, node)| match node {
            TreeNode::Leaf { tile } => Some(tile.as_ref()),
            TreeNode::Split { .. } => None,
        })
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> {
        self.nodes
            .values_mut()
            .filter_map(|node| match &mut node.value {
                TreeNode::Leaf { tile } => Some(tile.as_mut()),
                TreeNode::Split { .. } => None,
            })
    }

    pub fn active_window(&self) -> Option<&W> {
        let id = self.focus?;
        self.tile(id).map(Tile::window)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        let id = self.focus?;
        self.tile_mut(id).map(Tile::window_mut)
    }

    pub fn active_tile(&self) -> Option<&Tile<W>> {
        self.focus.and_then(|id| self.tile(id))
    }

    pub fn active_tile_mut(&mut self) -> Option<&mut Tile<W>> {
        self.focus.and_then(|id| self.tile_mut(id))
    }

    pub fn activate_window(&mut self, window: &W::Id) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        self.set_focus_id(Some(id));
        true
    }

    pub fn focus_left(&mut self) -> bool {
        self.focus_direction(Direction::Left)
    }

    pub fn focus_right(&mut self) -> bool {
        self.focus_direction(Direction::Right)
    }

    pub fn focus_up(&mut self) -> bool {
        self.focus_direction(Direction::Up)
    }

    pub fn focus_down(&mut self) -> bool {
        self.focus_direction(Direction::Down)
    }

    pub fn focus_first(&mut self) {
        self.set_focus_id(self.first_leaf());
    }

    pub fn focus_last(&mut self) {
        let focus = self
            .iter_depth_first()
            .filter_map(|(id, node)| matches!(node, TreeNode::Leaf { .. }).then_some(id))
            .last();
        self.set_focus_id(focus);
    }

    pub fn focus_window_in_subtree(&mut self, subtree: NodeId, index: usize) {
        let leaf = self.leaf_ids_in(subtree).get(index).copied();
        if let Some(leaf) = leaf {
            self.set_focus_id(Some(leaf));
        }
    }

    pub fn focus_window_in_column(&mut self, index: u8) {
        let Some(subtree) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        self.focus_window_in_subtree(subtree, usize::from(index));
    }

    pub fn focus_column(&mut self, index: usize) {
        let branch = self
            .root_children()
            .and_then(|children| children.get(index))
            .copied();
        if let Some(branch) = branch {
            self.set_focus_id(self.first_leaf_in(branch).or(self.focus));
        }
    }

    pub fn focus_column_first(&mut self) {
        self.focus_column(0);
    }

    pub fn focus_column_last(&mut self) {
        if let Some(last) = self
            .root_children()
            .and_then(|children| children.len().checked_sub(1))
        {
            self.focus_column(last);
        }
    }

    pub fn focus_top(&mut self) {
        self.focus_extreme(false);
    }

    pub fn focus_bottom(&mut self) {
        self.focus_extreme(true);
    }

    pub fn focus_up_or_left(&mut self) {
        if !self.focus_up() {
            self.focus_left();
        }
    }

    pub fn focus_up_or_right(&mut self) {
        if !self.focus_up() {
            self.focus_right();
        }
    }

    pub fn focus_down_or_left(&mut self) {
        if !self.focus_down() {
            self.focus_left();
        }
    }

    pub fn focus_down_or_right(&mut self) {
        if !self.focus_down() {
            self.focus_right();
        }
    }

    fn focus_extreme(&mut self, bottom: bool) {
        let geometries = self.compute_geometry();
        let focus = geometries
            .nodes
            .iter()
            .min_by(|(_, a), (_, b)| {
                let a = a.loc.y + if bottom { a.size.h } else { 0. };
                let b = b.loc.y + if bottom { b.size.h } else { 0. };
                if bottom {
                    b.total_cmp(&a)
                } else {
                    a.total_cmp(&b)
                }
            })
            .map(|(id, _)| *id)
            .or(self.focus);
        self.set_focus_id(focus);
    }
}

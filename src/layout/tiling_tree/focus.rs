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

    pub(crate) fn parent_of_window(&self, window: &W::Id) -> Option<NodeId> {
        self.nodes.get(&self.node_for_window(window)?)?.parent
    }

    pub fn non_root_parent_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.parent_of_window(window)
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

    /// The most recent focus entry strictly inside `node`, like sway's
    /// `seat_get_focus_inactive` (sway/input/seat.c:1357-1372).
    pub fn focus_inactive_in(&self, node: NodeId) -> Option<NodeId> {
        self.focus_inactive_in_excluding(node, node)
    }

    /// As `focus_inactive_in`, ignoring `excluded` and everything inside it: sway runs this
    /// after detaching the moved container, so its entries are no longer under `node`.
    pub fn focus_inactive_in_excluding(&self, node: NodeId, excluded: NodeId) -> Option<NodeId> {
        self.focus_history.iter().copied().find(|candidate| {
            *candidate != node
                && self.nodes.contains_key(candidate)
                && self.contains_node(node, *candidate)
                && !self.contains_node(excluded, *candidate)
        })
    }

    /// Picks sway's refocus target for a container leaving `old_parent`: the most recent entry
    /// under the old parent, else under the workspace (sway/commands/move.c:598-608). `moved`
    /// names the container when it is still attached, so its own entries are skipped. Returns
    /// the target with its ancestors so `resolve_transfer_focus` can follow a reaped target.
    pub(super) fn transfer_focus_target(
        &self,
        moved: Option<NodeId>,
        old_parent: Option<NodeId>,
    ) -> Option<(NodeId, Vec<NodeId>)> {
        let search = |node: NodeId| match moved {
            Some(moved) => self.focus_inactive_in_excluding(node, moved),
            None => self.focus_inactive_in(node),
        };
        let target = old_parent
            .filter(|parent| *parent != self.root)
            .and_then(search)
            .or_else(|| search(self.root))?;
        let mut ancestors = Vec::new();
        let mut parent = self.nodes.get(&target).and_then(|node| node.parent);
        while let Some(ancestor) = parent {
            ancestors.push(ancestor);
            parent = self.nodes.get(&ancestor).and_then(|node| node.parent);
        }
        Some((target, ancestors))
    }

    /// Focuses a target from `transfer_focus_target` after the move reaped empty containers.
    /// When the target itself was reaped, sway's destroy handler focuses the most recent view
    /// under its nearest surviving ancestor (sway/input/seat.c:273-286).
    pub(super) fn resolve_transfer_focus(&mut self, target: Option<(NodeId, Vec<NodeId>)>) -> bool {
        let Some((target, ancestors)) = target else {
            return false;
        };
        let focus = if self.nodes.contains_key(&target) {
            Some(target)
        } else {
            ancestors
                .into_iter()
                .find(|ancestor| self.nodes.contains_key(ancestor))
                .and_then(|ancestor| self.focused_leaf_in(ancestor))
        };
        if focus.is_some() {
            self.set_focus_id(focus);
        }
        focus.is_some()
    }

    /// Sway raises a split's new wrapper just below its focused child (`container_split`,
    /// sway/tree/container.c:1554-1560).
    pub(super) fn raise_split_wrapper(&mut self, child: NodeId, wrapper: NodeId) {
        if self.focus != Some(child) {
            return;
        }
        self.focus_history.retain(|candidate| *candidate != wrapper);
        let index = self
            .focus_history
            .iter()
            .position(|candidate| *candidate == child)
            .map_or(0, |index| index + 1);
        self.focus_history.insert(index, wrapper);
    }

    pub fn root_is_focused(&self) -> bool {
        self.focus == Some(self.root)
    }

    pub fn focused_leaf_is_only_child_of_resident_root(&self) -> bool {
        let Some(root) = self.resident_root() else {
            return false;
        };
        self.focus.is_some_and(|focus| {
            self.tile(focus).is_some()
                && self.nodes.get(&focus).and_then(|node| node.parent) == Some(root)
                && self.split_len(root) == Some(1)
        })
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
            let Some(TreeNode::Split {
                layout, children, ..
            }) = self.nodes.get(&self.root).map(|node| &node.value)
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

    pub fn scroll_tab_indicator(&mut self, window: &W::Id, steps: i32) -> Option<W::Id> {
        let id = self.node_for_window(window)?;
        let mut child = id;
        let (parent, children) = loop {
            let parent = self.nodes.get(&child)?.parent?;
            let TreeNode::Split {
                layout, children, ..
            } = &self.nodes.get(&parent)?.value
            else {
                return None;
            };
            if matches!(layout, Layout::Tabbed | Layout::Stacked) {
                break (parent, children.clone());
            }
            child = parent;
        };
        let active = self.focused_child_in(parent)?;
        let index = children.iter().position(|child| *child == active)?;
        let desired = (index as i32 + steps).clamp(0, children.len() as i32 - 1) as usize;
        let focus = children
            .get(desired)
            .and_then(|child| self.focused_leaf_in(*child));
        self.set_focus_id(focus);
        focus.and_then(|focus| self.tile(focus).map(|tile| tile.window().id().clone()))
    }

    pub fn focus_next_prev_sibling(&mut self, next: bool) -> bool {
        let Some(focus) = self.focus else {
            return false;
        };
        let Some(parent) = self.nodes.get(&focus).and_then(|node| node.parent) else {
            return false;
        };
        let Some(TreeNode::Split { layout, .. }) = self.nodes.get(&parent).map(|node| &node.value)
        else {
            return false;
        };
        let direction_layout = match layout {
            Layout::SplitH | Layout::Tabbed => Layout::SplitH,
            Layout::SplitV | Layout::Stacked => Layout::SplitV,
        };
        let mut current = focus;
        let mut wrap = None;
        while let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) {
            let Some(TreeNode::Split {
                layout, children, ..
            }) = self.nodes.get(&parent).map(|node| &node.value)
            else {
                return false;
            };
            if Self::layouts_parallel(*layout, direction_layout) {
                let Some(index) = children.iter().position(|child| *child == current) else {
                    return false;
                };
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
                if self.options.layout.focus_wrapping != swayward_config::FocusWrapping::No
                    && children.len() > 1
                    && wrap.is_none()
                {
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
        let Some(&TreeNode::Split { layout, .. }) = self.nodes.get(&parent).map(|node| &node.value)
        else {
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
        self.focus_direction_inner(dir, true)
    }

    pub fn focus_direction_without_wrap(&mut self, dir: Direction) -> bool {
        self.focus_direction_inner(dir, false)
    }

    fn focus_direction_inner(&mut self, dir: Direction, allow_wrap: bool) -> bool {
        let next = self.directional_focus_target(dir, allow_wrap);
        self.set_focus_id(next.or(self.focus));
        next.is_some()
    }

    fn directional_focus_target(&self, dir: Direction, allow_wrap: bool) -> Option<NodeId> {
        let mut current = self.focus?;
        let barrier = self.fullscreen_node();
        let direction_layout = match dir {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        };
        let backwards = matches!(dir, Direction::Left | Direction::Up);
        let mut wrap = None;

        while let Some(parent) = self.nodes.get(&current).and_then(|node| node.parent) {
            if Some(current) == barrier {
                break;
            }
            let Some(TreeNode::Split {
                layout, children, ..
            }) = self.nodes.get(&parent).map(|node| &node.value)
            else {
                return None;
            };
            if Self::layouts_parallel(*layout, direction_layout) {
                let index = children.iter().position(|child| *child == current)?;
                let desired = if backwards {
                    index.checked_sub(1)
                } else {
                    children.get(index + 1).map(|_| index + 1)
                };
                if let Some(desired) = desired {
                    return children
                        .get(desired)
                        .and_then(|child| self.focused_leaf_in(*child));
                }
                if allow_wrap
                    && self.options.layout.focus_wrapping != swayward_config::FocusWrapping::No
                    && children.len() > 1
                {
                    let candidate = if backwards {
                        children.last().copied()
                    } else {
                        children.first().copied()
                    };
                    if self.options.layout.focus_wrapping == swayward_config::FocusWrapping::Force {
                        return candidate.and_then(|id| self.focused_leaf_in(id));
                    }
                    if let Some(candidate) = candidate {
                        wrap.get_or_insert(candidate);
                    }
                }
            }
            if Some(parent) == barrier {
                break;
            }
            current = parent;
        }

        wrap.and_then(|id| self.focused_leaf_in(id))
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
        let id = self.focused_leaf_in(self.focus?)?;
        self.tile(id).map(Tile::window)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        let id = self.focused_leaf_in(self.focus?)?;
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

    pub fn focus_window_in_parent(&mut self, index: u8) {
        let Some(subtree) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        self.focus_window_in_subtree(subtree, usize::from(index));
    }

    pub fn focus_root_child(&mut self, index: usize) {
        let branch = self
            .root_children()
            .and_then(|children| children.get(index))
            .copied();
        if let Some(branch) = branch {
            self.set_focus_id(self.first_leaf_in(branch).or(self.focus));
        }
    }

    pub fn focus_first_root_child(&mut self) {
        self.focus_root_child(0);
    }

    pub fn focus_last_root_child(&mut self) {
        if let Some(last) = self
            .root_children()
            .and_then(|children| children.len().checked_sub(1))
        {
            self.focus_root_child(last);
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
            .leaf_boxes
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

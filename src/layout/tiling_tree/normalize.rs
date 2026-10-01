use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub(super) fn reap_empty_from(&mut self, mut id: NodeId) {
        loop {
            let (parent, empty) = match self.nodes.get(&id) {
                Some(Node {
                    parent,
                    value: TreeNode::Split { children, .. },
                }) => (*parent, children.is_empty()),
                _ => return,
            };
            if id == self.root || !empty {
                return;
            }
            let Some(parent) = parent else { return };
            if self.focus == Some(id) {
                self.set_focus_id(Some(parent));
            }
            self.remove_node(id);
            self.remove_child(parent, id);
            self.raise_view_after_reap(parent);
            id = parent;
        }
    }

    /// Sway's seat-node destroy handler raises the most recent view under the reaped
    /// container's parent into the focus stack below the current focus
    /// (`handle_seat_node_destroy`, sway/input/seat.c:273-323).
    fn raise_view_after_reap(&mut self, mut parent: NodeId) {
        let view = loop {
            if let Some(view) = self.focus_history.iter().copied().find(|candidate| {
                self.tile(*candidate).is_some() && self.contains_node(parent, *candidate)
            }) {
                break view;
            }
            match self.nodes.get(&parent).and_then(|node| node.parent) {
                Some(next) => parent = next,
                None => return,
            }
        };
        if Some(view) == self.focus {
            return;
        }
        self.focus_history.retain(|candidate| *candidate != view);
        let index = usize::from(
            self.focus
                .is_some_and(|focus| self.focus_history.first() == Some(&focus)),
        );
        self.focus_history
            .insert(index.min(self.focus_history.len()), view);
    }

    pub(super) fn collapse_from(&mut self, mut id: NodeId) {
        loop {
            let (parent, only_child, empty) = match self.nodes.get(&id) {
                Some(Node {
                    parent,
                    value: TreeNode::Split { children, .. },
                }) => (*parent, children.first().copied(), children.is_empty()),
                _ => return,
            };
            if id == self.root {
                return;
            }
            let Some(parent) = parent else { return };
            if empty {
                if self.focus == Some(id) {
                    self.set_focus_id(Some(parent));
                }
                self.remove_node(id);
                self.remove_child(parent, id);
                id = parent;
                continue;
            }
            let Some(child) = only_child.filter(|_| self.split_len(id) == Some(1)) else {
                return;
            };
            let Some(Node {
                value: TreeNode::Split { children, .. },
                ..
            }) = self.nodes.get_mut(&parent)
            else {
                return;
            };
            let Some(index) = children.iter().position(|node| *node == id) else {
                return;
            };
            if let Some(slot) = children.get_mut(index) {
                *slot = child;
            }
            self.nodes
                .get_mut(&child)
                .expect("invariant: a split's only child is present in the arena")
                .parent = Some(parent);
            if let Some(fullscreen) = self.pending_modes.get(&id).and_then(|mode| mode.fullscreen) {
                self.set_pending_fullscreen(child, Some(fullscreen));
            }
            if self.focus == Some(id) {
                self.set_focus_id(Some(child));
            }
            self.remove_node(id);
            id = parent;
        }
    }

    pub(super) fn compact_tree(&mut self) {
        loop {
            let squashable = self
                .iter_depth_first()
                .find_map(|(id, _)| self.squashable_child(id).map(|_| id));
            let Some(id) = squashable else { return };
            self.squash(id);
        }
    }

    pub(super) fn squashable_child(&self, id: NodeId) -> Option<NodeId> {
        let TreeNode::Split { children, .. } = &self.nodes.get(&id)?.value else {
            return None;
        };
        let [child] = children.as_slice() else {
            return None;
        };
        self.is_squashable(id, *child).then_some(*child)
    }

    /// A split holding a single split child of the perpendicular orientation, whose own parent
    /// runs parallel to that child, is redundant and squashed (`container_is_squashable`,
    /// sway/tree/container.c:1670-1677).
    pub(super) fn is_squashable(&self, id: NodeId, child: NodeId) -> bool {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        if self.resident_root && parent == self.root {
            return false;
        }
        let Some(TreeNode::Split {
            layout: parent_layout,
            ..
        }) = self.nodes.get(&parent).map(|node| &node.value)
        else {
            return false;
        };
        let Some(TreeNode::Split {
            layout, children, ..
        }) = self.nodes.get(&id).map(|node| &node.value)
        else {
            return false;
        };
        let Some(TreeNode::Split {
            layout: child_layout,
            ..
        }) = self.nodes.get(&child).map(|node| &node.value)
        else {
            return false;
        };
        children.len() == 1
            && matches!(layout, Layout::SplitH | Layout::SplitV)
            && matches!(child_layout, Layout::SplitH | Layout::SplitV)
            && !Self::layouts_parallel(*layout, *child_layout)
            && Self::layouts_parallel(*parent_layout, *child_layout)
    }

    /// Tabbed lays out like SplitH and stacked like SplitV for sway's parallelism tests
    /// (`is_parallel`, sway/tree/container.c:1656-1668).
    pub(super) fn layouts_parallel(first: Layout, second: Layout) -> bool {
        matches!(
            (first, second),
            (
                Layout::SplitH | Layout::Tabbed,
                Layout::SplitH | Layout::Tabbed
            ) | (
                Layout::SplitV | Layout::Stacked,
                Layout::SplitV | Layout::Stacked
            )
        )
    }

    pub(super) fn squash(&mut self, id: NodeId) {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return;
        };
        let Some(child) = self.squashable_child(id) else {
            return;
        };
        let (grandchildren, child_percents) = match self.nodes.get(&child).map(|node| &node.value) {
            Some(TreeNode::Split {
                children, percents, ..
            }) => (children.clone(), percents.clone()),
            _ => return,
        };
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let Some(index) = children.iter().position(|candidate| *candidate == id) else {
            return;
        };
        children.remove(index);
        let percent = percents.remove(index);
        for (offset, (grandchild, child_percent)) in
            grandchildren.iter().zip(child_percents).enumerate()
        {
            children.insert(index + offset, *grandchild);
            percents.insert(index + offset, percent * child_percent);
        }
        for grandchild in &grandchildren {
            self.nodes
                .get_mut(grandchild)
                .expect("invariant: every squashed grandchild is present in the arena")
                .parent = Some(parent);
        }
        let replacement = grandchildren.first().copied().unwrap_or(parent);
        if let Some(fullscreen) = [id, child]
            .into_iter()
            .find_map(|id| self.pending_modes.get(&id).and_then(|mode| mode.fullscreen))
        {
            self.set_pending_fullscreen(parent, Some(fullscreen));
        }
        if self.focus == Some(id) || self.focus == Some(child) {
            self.set_focus_id(Some(replacement));
        }
        self.remove_node(id);
        self.remove_node(child);
    }
}

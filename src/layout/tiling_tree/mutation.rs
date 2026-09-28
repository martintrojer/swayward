use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn add_tile(&mut self, tile: Tile<W>, target: InsertTarget) -> NodeId {
        self.add_tile_with_activation(tile, target, true)
    }

    pub fn add_tile_at_drop(
        &mut self,
        tile: Tile<W>,
        target: NodeId,
        edge: ResizeEdge,
        activate: bool,
    ) -> NodeId {
        let layout = if edge.intersects(ResizeEdge::LEFT | ResizeEdge::RIGHT) {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        let parent = self.nodes[&target].parent.unwrap_or(self.root);
        if !matches!(
            self.nodes[&parent].value,
            TreeNode::Split { layout: current, .. } if current == layout
        ) {
            self.split(target, layout);
        }
        let id = self.add_tile_with_activation(tile, InsertTarget::Node(target), activate);
        if edge.intersects(ResizeEdge::LEFT | ResizeEdge::TOP) {
            let parent = self.nodes[&id]
                .parent
                .expect("invariant: an inserted tile has a parent");
            let first = self
                .child_index(parent, target)
                .expect("invariant: the drop target remains a child of the insertion parent");
            let second = self
                .child_index(parent, id)
                .expect("invariant: the inserted tile is a child of its parent");
            let TreeNode::Split {
                children, percents, ..
            } = &mut self
                .nodes
                .get_mut(&parent)
                .expect("invariant: every child parent is present in the arena")
                .value
            else {
                unreachable!()
            };
            children.swap(first, second);
            percents.swap(first, second);
        }
        id
    }

    pub fn add_tile_right_of(
        &mut self,
        right_of: &W::Id,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        let target = self.node_for_window(right_of)?;
        Some(self.add_tile_with_activation(tile, InsertTarget::Node(target), activate))
    }

    pub fn add_tile_to_subtree(
        &mut self,
        subtree: NodeId,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        self.nodes
            .contains_key(&subtree)
            .then(|| self.add_tile_with_activation(tile, InsertTarget::Node(subtree), activate))
    }

    pub fn add_tile_with_activation(
        &mut self,
        mut tile: Tile<W>,
        target: InsertTarget,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        self.has_had_tile = true;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        let pending_mode = tile.window().pending_sizing_mode();
        let mapped_under_fullscreen = self.fullscreen_node().is_some();
        let previous_focus = self.focus;
        let old_geometries = self.compute_geometry();
        if !self.fullscreen_layout_wrappers.is_empty() {
            self.pre_layout_ipc_rects.clear();
        }
        let id = self.alloc(Node {
            parent: None,
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
        });
        let (parent, after) = self.insertion_slot(target);
        self.insert_child(parent, id, after);
        if parent == self.root {
            if let Some(layout) = match self.options.layout.workspace_layout {
                swayward_config::WorkspaceLayout::Default => None,
                swayward_config::WorkspaceLayout::Stacking => Some(Layout::Stacked),
                swayward_config::WorkspaceLayout::Tabbed => Some(Layout::Tabbed),
            } {
                self.wrap_node(id, layout);
            }
        }
        if activate && !mapped_under_fullscreen {
            self.set_focus_id(Some(id));
        } else if let Some(previous_focus) = previous_focus {
            self.focus_history.retain(|candidate| *candidate != id);
            if mapped_under_fullscreen {
                self.focus_history.push(id);
            } else {
                self.focus_history
                    .insert(1.min(self.focus_history.len()), id);
            }
            self.focus = Some(previous_focus);
        } else {
            self.set_focus_id(Some(id));
        }
        if mapped_under_fullscreen
            && !pending_mode.is_fullscreen()
            && !matches!(
                self.nodes.get(&parent).map(|node| &node.value),
                Some(TreeNode::Split {
                    layout: Layout::Tabbed | Layout::Stacked,
                    ..
                })
            )
        {
            self.mapped_under_fullscreen.insert(id);
        }
        if pending_mode.is_maximized() {
            self.pending_modes.insert(
                id,
                PendingMode {
                    fullscreen: None,
                    maximized: true,
                },
            );
        }
        self.compact_tree();
        if pending_mode.is_fullscreen() {
            self.replace_fullscreen_state(id, Some(FullscreenMode::Workspace));
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }

    fn insertion_slot(&self, target: InsertTarget) -> (NodeId, Option<NodeId>) {
        let target = match target {
            InsertTarget::Focused => self.focus,
            InsertTarget::Node(id) => Some(id),
        };
        let parent = target
            .and_then(|id| self.nodes.get(&id)?.parent)
            .unwrap_or(self.root);
        let after = target
            .filter(|target| self.nodes.get(target).and_then(|node| node.parent) == Some(parent));
        (parent, after)
    }

    pub fn remove_tile_node(&mut self, id: NodeId) -> Option<Tile<W>> {
        let old_geometries = self.compute_geometry();
        if !matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            return None;
        }
        let removed_fullscreen = self.fullscreen_node() == Some(id);
        let node = self.remove_node(id)?;
        if removed_fullscreen {
            self.mapped_under_fullscreen.clear();
        }
        let TreeNode::Leaf { mut tile } = node.value else {
            unreachable!();
        };
        tile.clear_tiled_content_size();
        self.interactive_resize = None;
        if let Some(parent) = node.parent {
            self.remove_child(parent, id);
            self.reap_empty_from(parent);
        }
        if self.windows().next().is_none() {
            let TreeNode::Split { layout, .. } = self.nodes[&self.root].value else {
                unreachable!()
            };
            self.empty_representation_layout = Some(layout);
            self.pending_modes.clear();
            self.set_focus_id(None);
        } else if self.focus == Some(id) {
            self.set_focus_id(
                self.fullscreen_node()
                    .and_then(|fullscreen| self.focused_leaf_in(fullscreen))
                    .or_else(|| self.focused_leaf_in(self.root)),
            );
        }
        self.animate_geometry_changes(old_geometries, None);
        Some(*tile)
    }

    pub(super) fn remove_tile_node_preserving_parent(&mut self, id: NodeId) -> Option<Tile<W>> {
        self.remove_tile_node(id)
    }

    pub(super) fn remove_node(&mut self, id: NodeId) -> Option<Node<W>> {
        let node = self.nodes.remove(&id)?;
        self.previous_split_layouts.remove(&id);
        self.title_formats.remove(&id);
        self.pending_modes.remove(&id);
        self.mapped_under_fullscreen.remove(&id);
        self.fullscreen_layout_wrappers.remove(&id);
        self.pre_layout_ipc_rects.remove(&id);
        self.tab_indicators.remove(&id);
        self.tab_active.remove(&id);
        self.tab_active.retain(|_, active| *active != id);
        self.focus_history.retain(|candidate| *candidate != id);
        Some(node)
    }

    pub(super) fn set_focus_id(&mut self, focus: Option<NodeId>) {
        self.focus = focus;
        if let Some(id) = focus {
            self.focus_history.retain(|candidate| *candidate != id);
            self.focus_history.insert(0, id);
        }
    }

    pub(super) fn focused_child_in(&self, parent: NodeId) -> Option<NodeId> {
        let TreeNode::Split { children, .. } = &self.nodes.get(&parent)?.value else {
            return None;
        };
        self.focus_history
            .iter()
            .find_map(|focused| {
                children
                    .iter()
                    .copied()
                    .find(|child| self.contains_node(*child, *focused))
            })
            .or_else(|| children.first().copied())
    }

    pub fn remove_tile(&mut self, window: &W::Id, transaction: Transaction) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node(id)?;
        self.request_window_sizes_with(Some(transaction), true);
        Some(tile)
    }

    pub fn remove_tile_preserving_parent(&mut self, window: &W::Id) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node_preserving_parent(id)?;
        self.request_window_sizes();
        Some(tile)
    }

    pub fn add_tile_to_existing_parent(
        &mut self,
        mut tile: Tile<W>,
        parent: NodeId,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        let old_geometries = self.compute_geometry();
        let id = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
        });
        self.insert_child(parent, id, None);
        if activate {
            self.set_focus_id(Some(id));
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }

    pub(super) fn alloc(&mut self, node: Node<W>) -> NodeId {
        let id = NodeId(NODE_ID_COUNTER.next());
        self.nodes.insert(id, node);
        id
    }

    pub(super) fn insert_with_id(&mut self, old_id: NodeId, node: Node<W>) -> NodeId {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.nodes.entry(old_id) {
            entry.insert(node);
            old_id
        } else {
            self.alloc(node)
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
            index + 1
        } else {
            let Some(index) = index.checked_sub(1) else {
                return false;
            };
            index
        };
        let Some(sibling) = (match &self.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split { children, .. }) => children.get(sibling_index),
            _ => None,
        })
        .copied() else {
            return false;
        };

        self.interactive_resize = None;
        let old = self.compute_geometry();
        self.remove_child(parent, id);
        let sibling_percent = match &self.nodes[&parent].value {
            TreeNode::Split {
                children, percents, ..
            } => children
                .iter()
                .position(|child| *child == sibling)
                .map(|index| percents[index]),
            TreeNode::Leaf { .. } => None,
        };
        let Some(sibling_percent) = sibling_percent else {
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
                children[index] = wrapper;
                percents[index] = sibling_percent;
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
        self.compact_tree();
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
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

    pub(super) fn insert_child(&mut self, parent: NodeId, child: NodeId, after: Option<NodeId>) {
        let index = match &self.nodes[&parent].value {
            TreeNode::Split { children, .. } => after
                .and_then(|id| children.iter().position(|child| *child == id))
                .map_or(children.len(), |index| index + 1),
            TreeNode::Leaf { .. } => return,
        };
        self.insert_child_at(parent, child, index);
    }

    pub(super) fn insert_child_at(&mut self, parent: NodeId, child: NodeId, index: usize) {
        if !self.nodes.contains_key(&child) {
            debug_assert!(false, "inserted child must be present in the arena");
            return;
        }
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let percent = 1. / (children.len() + 1) as f64;
        for existing in percents.iter_mut() {
            *existing *= 1. - percent;
        }
        children.insert(index.min(children.len()), child);
        percents.insert(index.min(percents.len()), percent);
        self.nodes
            .get_mut(&child)
            .expect("invariant: the validated inserted child remains in the arena")
            .parent = Some(parent);
    }

    pub(super) fn child_index(&self, parent: NodeId, child: NodeId) -> Option<usize> {
        match &self.nodes.get(&parent)?.value {
            TreeNode::Split { children, .. } => children.iter().position(|id| *id == child),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub(super) fn insert_existing_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: usize,
        split_share_of: NodeId,
    ) {
        if !self.nodes.contains_key(&child) {
            debug_assert!(false, "inserted child must be present in the arena");
            return;
        }
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let Some(target_index) = children.iter().position(|id| *id == split_share_of) else {
            return;
        };
        percents[target_index] /= 2.;
        let percent = percents[target_index];
        let index = index.min(children.len());
        children.insert(index, child);
        percents.insert(index, percent);
        self.nodes
            .get_mut(&child)
            .expect("invariant: the validated inserted child remains in the arena")
            .parent = Some(parent);
    }

    pub(super) fn detach_subtree_only(&mut self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes.get(&id).and_then(|node| node.parent)?;
        self.remove_child(parent, id);
        self.nodes.get_mut(&id)?.parent = None;
        Some(parent)
    }

    pub(super) fn wrap_root_for_direction(&mut self, id: NodeId, direction: Direction) {
        let layout = match direction {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        };
        let old_value = std::mem::replace(
            &mut self
                .nodes
                .get_mut(&self.root)
                .expect("invariant: the root is always present in the arena")
                .value,
            TreeNode::Split {
                layout,
                children: Vec::new(),
                percents: Vec::new(),
            },
        );
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
        let moving_first = matches!(direction, Direction::Left | Direction::Up);
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
        };
    }

    pub(super) fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        if let Some(index) = children.iter().position(|id| *id == child) {
            children.remove(index);
            percents.remove(index);
            // Rescale against what the siblings actually hold, not against
            // 1 - removed. Accumulated rounding, or a removed share of 1,
            // left the remainder not summing to 1 and tripped the tree
            // invariant. An all-zero remainder falls back to equal shares.
            let total: f64 = percents.iter().sum();
            if total > 0. {
                for percent in percents.iter_mut() {
                    *percent /= total;
                }
            } else if !percents.is_empty() {
                let equal = 1. / percents.len() as f64;
                for percent in percents.iter_mut() {
                    *percent = equal;
                }
            }
        }
    }

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
            id = parent;
        }
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
            children[index] = child;
            self.nodes
                .get_mut(&child)
                .expect("invariant: a split's only child is present in the arena")
                .parent = Some(parent);
            if let Some(fullscreen) = self.pending_modes.get(&id).and_then(|mode| mode.fullscreen) {
                self.pending_modes
                    .entry(child)
                    .or_insert(PendingMode {
                        fullscreen: None,
                        maximized: false,
                    })
                    .fullscreen = Some(fullscreen);
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
        let (grandchildren, child_percents) = match &self.nodes[&child].value {
            TreeNode::Split {
                children, percents, ..
            } => (children.clone(), percents.clone()),
            TreeNode::Leaf { .. } => return,
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
            self.pending_modes
                .entry(parent)
                .or_insert(PendingMode {
                    fullscreen: None,
                    maximized: false,
                })
                .fullscreen = Some(fullscreen);
        }
        if self.focus == Some(id) || self.focus == Some(child) {
            self.set_focus_id(Some(replacement));
        }
        self.remove_node(id);
        self.remove_node(child);
    }

    pub(super) fn split_len(&self, id: NodeId) -> Option<usize> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Split { children, .. } => Some(children.len()),
            TreeNode::Leaf { .. } => None,
        }
    }
}

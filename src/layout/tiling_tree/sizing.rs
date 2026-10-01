use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub(super) fn working_area(&self) -> Rectangle<f64, Logical> {
        apply_struts(self.parent_area, self.scale, self.options.layout.struts)
    }

    /// Nodes left out of their parent's split: views mapped under fullscreen,
    /// and a fullscreen node that arrived fullscreen. Sway zeroes only the
    /// moved container's width fraction and never re-arranges its new parent
    /// while the workspace is fullscreen, so the siblings keep their boxes.
    pub(super) fn split_excluded(&self) -> std::borrow::Cow<'_, HashSet<NodeId>> {
        let arrived = self
            .fullscreen_arrived
            .then(|| self.fullscreen_node())
            .flatten();
        if arrived.is_none() && self.moved_under_fullscreen.is_empty() {
            return std::borrow::Cow::Borrowed(&self.mapped_under_fullscreen);
        }
        let mut excluded = self.mapped_under_fullscreen.clone();
        excluded.extend(arrived);
        excluded.extend(self.moved_under_fullscreen.keys().copied());
        std::borrow::Cow::Owned(excluded)
    }

    /// Record that `window` was moved into this tree while it was fullscreen.
    /// `source_rect` is its IPC box in the source tree: sway keeps the moved
    /// container's position and content box, zeroing only its width and height.
    pub fn mark_moved_under_fullscreen(
        &mut self,
        window: &W::Id,
        source_rect: Rectangle<f64, Logical>,
    ) {
        let Some(fullscreen) = self.fullscreen_node() else {
            return;
        };
        if let Some(id) = self
            .node_for_window(window)
            .filter(|id| *id != fullscreen && !self.contains_node(fullscreen, *id))
        {
            self.mapped_under_fullscreen.remove(&id);
            self.moved_under_fullscreen.insert(id, source_rect);
            // `workspace_focus_fullscreen` (sway/commands/move.c:96-110) raises
            // the fullscreen container's focus-inactive view back above the
            // moved one, so the moved view's newer window focus no longer ranks
            // it first in the workspace focus list.
            // Sway's focus stack is seat-wide: the moved view, focused by the
            // move, sits just below the re-raised fullscreen view.
            self.focus_history.retain(|candidate| *candidate != id);
            self.focus_history.insert(0, id);
            if let Some(leaf) = self.focused_leaf_in(fullscreen) {
                self.focus_history.retain(|candidate| *candidate != leaf);
                self.focus_history.insert(0, leaf);
                self.ipc_focus_follows_history = true;
            }
        }
    }

    pub fn ipc_focus_follows_history(&self) -> bool {
        self.ipc_focus_follows_history
    }

    /// IPC box of a leaf as last arranged, before any transfer.
    pub fn ipc_rect_for_window(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        let id = self.node_for_window(window)?;
        self.compute_geometry().leaf_ipc_rects.remove(&id)
    }

    pub(super) fn compute_geometry(&self) -> geometry::Geometry<W::Id> {
        let excluded = self.split_excluded();
        let fullscreen = self.fullscreen_node().into_iter().collect();
        let visible_leaves = self.visible_leaves();
        geometry::compute(geometry::GeometryInput {
            nodes: &self.nodes,
            root: self.root,
            view_size: self.view_size,
            parent_area: self.parent_area,
            scale: self.scale,
            struts: if self.resident_root {
                Default::default()
            } else {
                self.options.layout.struts
            },
            gaps: self.gaps,
            outer_gaps_configured: self.options.layout.outer_gaps_configured || self.resident_root,
            gaps_to_edge: self.gaps_to_edge,
            titlebar_height: self.titlebar_height,
            fullscreen: &fullscreen,
            mapped_under_fullscreen: &excluded,
            hide_edge_borders: self.options.layout.hide_edge_borders,
            smart_borders: self.options.layout.smart_borders,
            visible_leaves: &visible_leaves,
            draw_uncovered_top_border: self.options.layout.draw_uncovered_top_border,
        })
    }

    pub(super) fn animate_geometry_changes(
        &mut self,
        old: geometry::Geometry<W::Id>,
        skip: Option<NodeId>,
    ) {
        let new = self.compute_geometry();
        for (id, old_rect) in old.leaf_boxes {
            if skip == Some(id) {
                continue;
            }
            let Some(new_rect) = new.leaf_boxes.get(&id) else {
                continue;
            };
            let offset = old_rect.loc - new_rect.loc;
            if offset != Point::default() {
                if let Some(tile) = self.tile_mut(id) {
                    tile.animate_move_from(offset);
                }
            }
        }
    }

    pub(super) fn titlebar_state(&self, id: NodeId, workspace_focused: bool) -> TitlebarState {
        let urgent = self.tile(id).is_some_and(|tile| tile.window().is_urgent());
        if urgent {
            return TitlebarState::Urgent;
        }
        let Some(focus) = self.focus else {
            return TitlebarState::Unfocused;
        };
        if id == focus {
            return if workspace_focused {
                TitlebarState::Focused
            } else {
                TitlebarState::FocusedInactive
            };
        }
        // A tab or stack entry labels its first leaf. It shows the focused-tab colour when the
        // entry's subtree holds the focus, so only the focus's ancestors can be such entries.
        let is_tab_title_with_focused_descendant =
            std::iter::successors(Some(focus), |child| self.nodes.get(child)?.parent).any(
                |child| {
                    let parent = self.nodes.get(&child).and_then(|node| node.parent);
                    matches!(
                        parent
                            .and_then(|parent| self.nodes.get(&parent))
                            .map(|node| &node.value),
                        Some(TreeNode::Split {
                            layout: Layout::Tabbed | Layout::Stacked,
                            ..
                        })
                    ) && self.first_leaf_in(child) == Some(id)
                },
            );
        if is_tab_title_with_focused_descendant {
            TitlebarState::FocusedTabTitle
        } else {
            TitlebarState::Unfocused
        }
    }

    pub(super) fn visible_leaves(&self) -> HashSet<NodeId> {
        if let Some(fullscreen) = self.fullscreen_node() {
            let mut visible = HashSet::new();
            self.collect_visible(fullscreen, &mut visible);
            return visible;
        }
        let mut visible = HashSet::new();
        self.collect_visible(self.root, &mut visible);
        visible
    }

    pub(super) fn collect_visible(&self, id: NodeId, visible: &mut HashSet<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        match &node.value {
            TreeNode::Leaf { .. } => {
                visible.insert(id);
            }
            TreeNode::Split {
                layout, children, ..
            } => {
                if matches!(layout, Layout::Tabbed | Layout::Stacked) {
                    if let Some(child) = self.shown_child_in(id) {
                        self.collect_visible(child, visible);
                    }
                } else {
                    for child in children {
                        self.collect_visible(*child, visible);
                    }
                }
            }
        }
    }

    pub(super) fn root_children(&self) -> Option<&[NodeId]> {
        match &self.nodes.get(&self.root)?.value {
            TreeNode::Split { children, .. } => Some(children),
            TreeNode::Leaf { .. } => None,
        }
    }

    pub(super) fn root_branch(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            let parent = self.nodes.get(&id)?.parent?;
            if parent == self.root {
                return Some(id);
            }
            id = parent;
        }
    }

    pub(super) fn tile(&self, id: NodeId) -> Option<&Tile<W>> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Leaf { tile } => Some(tile),
            TreeNode::Split { .. } => None,
        }
    }

    pub(super) fn tile_mut(&mut self, id: NodeId) -> Option<&mut Tile<W>> {
        match &mut self.nodes.get_mut(&id)?.value {
            TreeNode::Leaf { tile } => Some(tile),
            TreeNode::Split { .. } => None,
        }
    }

    pub(crate) fn node_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.windows()
            .find_map(|(id, candidate)| (candidate.id() == window).then_some(id))
    }

    pub(super) fn sibling_percents(&self, first: NodeId, second: NodeId) -> Option<(f64, f64)> {
        let parent = self.nodes.get(&first)?.parent?;
        if self.nodes.get(&second)?.parent != Some(parent) {
            return None;
        }
        let TreeNode::Split {
            children, percents, ..
        } = &self.nodes.get(&parent)?.value
        else {
            return None;
        };
        let first = children.iter().position(|id| *id == first)?;
        let second = children.iter().position(|id| *id == second)?;
        Some((*percents.get(first)?, *percents.get(second)?))
    }

    pub fn node_geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.compute_geometry().ipc_nodes.remove(&id)
    }

    pub(super) fn leaf_ids_in(&self, id: NodeId) -> Vec<NodeId> {
        let mut ids = Vec::new();
        self.collect_leaf_ids(id, &mut ids);
        ids
    }

    pub(super) fn collect_leaf_ids(&self, id: NodeId, ids: &mut Vec<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        match &node.value {
            TreeNode::Leaf { .. } => ids.push(id),
            TreeNode::Split { children, .. } => {
                for child in children {
                    self.collect_leaf_ids(*child, ids);
                }
            }
        }
    }

    pub(super) fn cancel_resize_for(&mut self, id: NodeId) {
        if self.interactive_resize.as_ref().is_some_and(|resize| {
            resize.target == id
                || resize
                    .axes
                    .iter()
                    .any(|axis| axis.first == id || axis.second == id)
        }) {
            self.interactive_resize = None;
        }
    }

    pub(super) fn first_leaf(&self) -> Option<NodeId> {
        self.first_leaf_in(self.root)
    }

    pub(super) fn first_leaf_in(&self, id: NodeId) -> Option<NodeId> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Leaf { .. } => Some(id),
            TreeNode::Split { children, .. } => {
                children.iter().find_map(|child| self.first_leaf_in(*child))
            }
        }
    }

    pub(super) fn focused_leaf_in(&self, id: NodeId) -> Option<NodeId> {
        self.focus_history
            .iter()
            .copied()
            .find(|candidate| self.tile(*candidate).is_some() && self.contains_node(id, *candidate))
            .or_else(|| self.first_leaf_in(id))
    }

    pub(super) fn request_window_sizes(&mut self) {
        self.request_window_sizes_with(None, false);
    }

    pub(super) fn request_window_sizes_with(
        &mut self,
        transaction: Option<Transaction>,
        animate: bool,
    ) {
        let geometries = self.compute_geometry();
        for (id, node) in &mut self.nodes {
            if let TreeNode::Leaf { tile } = &mut node.value {
                let transaction = transaction.clone();
                if let Some(rect) = geometries.leaf_boxes.get(id) {
                    tile.set_border_edges(
                        geometries
                            .border_edges
                            .get(id)
                            .copied()
                            .unwrap_or_else(ResizeEdge::all),
                    );
                    tile.set_decorated_box(
                        geometries
                            .border_corners
                            .get(id)
                            .copied()
                            .unwrap_or(DecoratedCorners::NONE),
                        geometries.titlebar_attached.contains(id),
                        geometries.titlebar_owned_by_parent.contains(id),
                    );
                    let mode = self.pending_modes.get(id).copied().unwrap_or_default();
                    if mode.fullscreen.is_some() {
                        tile.request_fullscreen(animate, transaction);
                    } else if mode.maximized {
                        tile.request_maximized(self.parent_area.size, animate, transaction);
                    } else {
                        tile.request_tile_size(rect.size, animate, transaction);
                    }
                }
            }
        }
    }
}

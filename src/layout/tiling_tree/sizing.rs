use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub(super) fn working_area(&self) -> Rectangle<f64, Logical> {
        apply_struts(self.parent_area, self.scale, self.options.layout.struts)
    }

    pub(super) fn compute_geometry(&self) -> geometry::Geometry<W::Id> {
        let fullscreen = self.fullscreen_node().into_iter().collect();
        let visible_leaves = self.visible_leaves();
        geometry::compute(
            &self.nodes,
            &self.title_formats,
            self.root,
            self.view_size,
            self.parent_area,
            self.scale,
            if self.resident_root {
                Default::default()
            } else {
                self.options.layout.struts
            },
            self.gaps,
            self.options.layout.outer_gaps_configured || self.resident_root,
            self.gaps_to_edge,
            self.titlebar_height,
            &fullscreen,
            &self.mapped_under_fullscreen,
            self.options.layout.hide_edge_borders,
            self.options.layout.smart_borders,
            &visible_leaves,
            self.options.layout.draw_uncovered_top_border,
        )
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
        let is_tab_title_with_focused_descendant = self.nodes.values().any(|node| {
            let TreeNode::Split {
                layout: Layout::Tabbed | Layout::Stacked,
                children,
                ..
            } = &node.value
            else {
                return false;
            };
            children.iter().any(|child| {
                self.first_leaf_in(*child) == Some(id) && self.contains_node(*child, focus)
            })
        });
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
                    let focused_branch = self.focus.and_then(|focus| {
                        children
                            .iter()
                            .find(|child| self.contains_node(**child, focus))
                    });
                    if let Some(child) = focused_branch.or_else(|| children.first()) {
                        self.collect_visible(*child, visible);
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
        Some((percents[first], percents[second]))
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
                    let mode = self.pending_modes.get(id).copied().unwrap_or(PendingMode {
                        fullscreen: None,
                        maximized: false,
                    });
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

use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn visible_window_count(&self) -> usize {
        self.visible_leaves().len()
    }

    pub fn geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.compute_geometry().leaf_boxes.remove(&id)
    }

    pub fn ipc_decoration_rect(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        let id = self.node_for_window(window)?;
        let mut geometry = self.compute_geometry();
        if let Some(bar) = geometry.titlebars.remove(&id) {
            return Some(bar.ipc_rect);
        }
        geometry
            .titlebars
            .into_iter()
            .find(|(titlebar_id, _)| geometry.titlebar_leaves[titlebar_id] == id)
            .map(|(_, bar)| bar.ipc_rect)
    }

    #[cfg(test)]
    pub fn titlebar_titles(&self) -> Vec<String> {
        self.compute_geometry()
            .titlebars
            .into_values()
            .map(|bar| bar.title)
            .collect()
    }

    pub fn titlebar_rects(&self) -> Vec<(W::Id, Rectangle<f64, Logical>)> {
        self.compute_geometry()
            .titlebars
            .into_values()
            .map(|bar| (bar.target, bar.rect))
            .collect()
    }

    pub fn tiled_drop_target(&self, pos: Point<f64, Logical>) -> Option<(NodeId, ResizeEdge)> {
        let geometries = self.compute_geometry();
        let visible = self.visible_leaves();
        let (id, rect) = geometries
            .ipc_nodes
            .iter()
            .filter(|(id, rect)| visible.contains(id) && rect.contains(pos))
            .min_by(|(_, a), (_, b)| {
                (a.size.w * a.size.h)
                    .partial_cmp(&(b.size.w * b.size.h))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;
        let distances = [
            (pos.x - rect.loc.x, ResizeEdge::LEFT),
            (pos.y - rect.loc.y, ResizeEdge::TOP),
            (rect.loc.x + rect.size.w - pos.x, ResizeEdge::RIGHT),
            (rect.loc.y + rect.size.h - pos.y, ResizeEdge::BOTTOM),
        ];
        let (distance, edge) = distances
            .into_iter()
            .min_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))?;
        let thickness = rect.size.w.min(rect.size.h) * 0.3;
        Some((
            *id,
            if distance > thickness {
                ResizeEdge::empty()
            } else {
                edge
            },
        ))
    }

    pub fn ipc_focus_is_stale(&self, id: NodeId) -> bool {
        self.ipc_stale_nodes.contains(&id)
    }

    pub fn window_for_node(&self, id: NodeId) -> Option<&W> {
        self.tile(id).map(Tile::window)
    }

    pub fn set_title_format(&mut self, id: NodeId, format: String) -> bool {
        if !self.is_split(id) {
            return false;
        }
        if format == "%title" {
            self.title_formats.remove(&id);
        } else {
            self.title_formats.insert(id, format);
        }
        true
    }

    pub fn is_split_sticky(&self, id: NodeId) -> bool {
        self.sticky_splits.contains(&id)
    }

    pub fn set_split_sticky(&mut self, id: NodeId, sticky: bool) -> bool {
        if !self.is_split(id) {
            return false;
        }
        if sticky {
            self.sticky_splits.insert(id);
        } else {
            self.sticky_splits.remove(&id);
        }
        true
    }

    pub fn windows(&self) -> impl Iterator<Item = (NodeId, &W)> {
        self.iter_depth_first().filter_map(|(id, node)| match node {
            TreeNode::Leaf { tile } => Some((id, tile.window())),
            TreeNode::Split { .. } => None,
        })
    }

    /// The tiled slot a fullscreen node reports after sway re-arranged its
    /// parent without re-arranging the workspace.
    fn fullscreen_tile_slot_rect(
        &self,
        id: NodeId,
        geometries: &geometry::Geometry<W::Id>,
    ) -> Option<Rectangle<f64, Logical>> {
        (self.fullscreen_tile_slot && self.fullscreen_node() == Some(id))
            .then(|| geometries.tiled_ipc_nodes.get(&id).copied())
            .flatten()
    }

    pub fn ipc_tree(&self) -> IpcNode<W::Id> {
        fn inset_split_by_parent_titlebar<I>(node: &mut IpcNode<I>, height: f64) {
            if let IpcNode::Split { rect, .. } = node {
                rect.loc.y += height;
                rect.size.h = (rect.size.h - height).max(0.);
            }
        }

        fn snapshot_leaf<W: LayoutElement>(
            tree: &TilingTree<W>,
            id: NodeId,
            tile: &crate::layout::tile::Tile<W>,
            percent: Option<f64>,
            geometries: &geometry::Geometry<W::Id>,
            inside_pending_wrapper: bool,
        ) -> IpcNode<W::Id> {
            let fallback_titlebar = tree.fullscreen_node().is_some()
                && tree.fullscreen_mode(id).is_none()
                && !tree.mapped_under_fullscreen.contains(&id)
                && tile.has_configured_sway_titlebar()
                && (inside_pending_wrapper
                    || !geometries.titlebars.contains_key(&id)
                    || tree.pre_layout_ipc_rects.contains_key(&id));
            let mut rect = tree
                .fullscreen_tile_slot_rect(id, geometries)
                .or_else(|| tree.pre_layout_ipc_rects.get(&id).copied())
                .or_else(|| {
                    (!inside_pending_wrapper)
                        .then(|| geometries.leaf_ipc_rects.get(&id).copied())
                        .flatten()
                })
                .unwrap_or_default();
            if fallback_titlebar {
                rect.loc.y += tree.titlebar_height;
                if inside_pending_wrapper {
                    rect.size.h -= tree.titlebar_height;
                } else {
                    rect.size.h = (rect.size.h - tree.titlebar_height).max(0.);
                }
            }
            IpcNode::Leaf {
                id,
                window: tile.window().id().clone(),
                percent,
                focused: tree.focus == Some(id),
                fullscreen_mode: tree.fullscreen_mode(id).map_or(0, |mode| mode as i32),
                rect,
                deco_rect: (!inside_pending_wrapper)
                    .then(|| geometries.titlebars.get(&id).map(|bar| bar.ipc_rect))
                    .flatten()
                    .or_else(|| {
                        fallback_titlebar.then(|| {
                            Rectangle::new(
                                Point::default(),
                                (rect.size.w, tree.titlebar_height).into(),
                            )
                        })
                    }),
                border: tile.sway_border(),
                border_edges: geometries
                    .border_edges
                    .get(&id)
                    .copied()
                    .unwrap_or_else(ResizeEdge::all),
                sticky: tile.is_sticky,
                mapped_under_fullscreen: tree.mapped_under_fullscreen.contains(&id),
            }
        }

        fn snapshot<W: LayoutElement>(
            tree: &TilingTree<W>,
            id: NodeId,
            percent: Option<f64>,
            geometries: &geometry::Geometry<W::Id>,
        ) -> IpcNode<W::Id> {
            let inside_pending_wrapper = tree.fullscreen_node().is_some()
                && tree
                    .fullscreen_layout_wrappers
                    .iter()
                    .any(|wrapper| tree.contains_node(*wrapper, id));
            match &tree.nodes[&id].value {
                TreeNode::Split {
                    layout,
                    children,
                    percents,
                } => IpcNode::Split {
                    id,
                    layout: *layout,
                    title: tree.title_formats.get(&id).cloned(),
                    percent: (tree.fullscreen_node().is_some()
                        && tree.fullscreen_layout_wrappers.contains(&id))
                    .then_some(0.)
                    .or(percent),
                    rect: if tree.fullscreen_node().is_some()
                        && tree.fullscreen_layout_wrappers.contains(&id)
                    {
                        Rectangle::default()
                    } else if let Some(slot) = tree.fullscreen_tile_slot_rect(id, geometries) {
                        slot
                    } else {
                        geometries.ipc_nodes.get(&id).copied().unwrap_or_default()
                    },
                    focus: tree
                        .focus_history
                        .iter()
                        .filter(|focused| !tree.ipc_stale_nodes.contains(focused))
                        .filter_map(|focused| {
                            children
                                .iter()
                                .copied()
                                .find(|child| tree.contains_node(*child, *focused))
                        })
                        .chain(children.iter().copied())
                        .fold(Vec::new(), |mut focus, child| {
                            if !focus.contains(&child) {
                                focus.push(child);
                            }
                            focus
                        }),
                    focused: tree.focus == Some(id),
                    fullscreen_mode: tree.fullscreen_mode(id).map_or(0, |mode| mode as i32),
                    sticky: tree.sticky_splits.contains(&id),
                    children: children
                        .iter()
                        .zip(percents)
                        .enumerate()
                        .map(|(index, (child, stored_percent))| {
                            let percent = match layout {
                                Layout::Tabbed | Layout::Stacked
                                    if tree.fullscreen_node().is_some()
                                        && tree.fullscreen_layout_wrappers.contains(&id) =>
                                {
                                    None
                                }
                                Layout::Tabbed | Layout::Stacked => Some(1.),
                                Layout::SplitH | Layout::SplitV
                                    if tree.fullscreen_node().is_some() =>
                                {
                                    let rect = |child: &NodeId| {
                                        tree.pre_layout_ipc_rects
                                            .get(child)
                                            .or_else(|| geometries.tiled_ipc_nodes.get(child))
                                            .copied()
                                    };
                                    let extent = |rect: Rectangle<f64, Logical>| match layout {
                                        Layout::SplitH => rect.size.w,
                                        Layout::SplitV => rect.size.h,
                                        _ => unreachable!(),
                                    };
                                    let parent_rect = rect(&id).unwrap_or_default();
                                    let parent_extent = extent(parent_rect).round();
                                    let allocated = if index + 1 == children.len() {
                                        parent_extent
                                            - percents[..index]
                                                .iter()
                                                .map(|percent| (parent_extent * percent).round())
                                                .sum::<f64>()
                                    } else {
                                        (parent_extent * stored_percent).round()
                                    };
                                    Some(if tree.mapped_under_fullscreen.contains(child) {
                                        0.
                                    } else if !tree.mapped_under_fullscreen.is_empty()
                                        && tree.fullscreen_node() != Some(*child)
                                    {
                                        let visible_total = children
                                            .iter()
                                            .zip(percents)
                                            .filter(|(child, _)| {
                                                !tree.mapped_under_fullscreen.contains(child)
                                            })
                                            .map(|(_, percent)| percent)
                                            .sum::<f64>();
                                        *stored_percent / visible_total
                                    } else if tree.fullscreen_node() == Some(*child) {
                                        let child_rect = tree
                                            .fullscreen_tile_slot_rect(*child, geometries)
                                            .or_else(|| geometries.ipc_nodes.get(child).copied())
                                            .unwrap_or_default();
                                        let parent_area =
                                            parent_rect.size.w.round() * parent_rect.size.h.round();
                                        let child_area =
                                            child_rect.size.w.round() * child_rect.size.h.round();
                                        if parent_area > 0. {
                                            child_area / parent_area
                                        } else {
                                            *stored_percent
                                        }
                                    } else if parent_extent > 0. {
                                        allocated / parent_extent
                                    } else {
                                        *stored_percent
                                    })
                                }
                                Layout::SplitH | Layout::SplitV => {
                                    let rounded_extent =
                                        |rect: Rectangle<f64, Logical>| match layout {
                                            Layout::SplitH => rect.size.w.round(),
                                            Layout::SplitV => rect.size.h.round(),
                                            _ => unreachable!(),
                                        };
                                    let parent_extent = geometries
                                        .ipc_nodes
                                        .get(&id)
                                        .copied()
                                        .map(rounded_extent)
                                        .unwrap_or_default();
                                    let raw_extent = |rect: Rectangle<f64, Logical>| match layout {
                                        Layout::SplitH => rect.size.w,
                                        Layout::SplitV => rect.size.h,
                                        _ => unreachable!(),
                                    };
                                    let available = children
                                        .iter()
                                        .filter_map(|child| {
                                            geometries.ipc_nodes.get(child).copied()
                                        })
                                        .map(raw_extent)
                                        .sum::<f64>();
                                    let allocated = if index + 1 == children.len() {
                                        available.round()
                                            - percents[..index]
                                                .iter()
                                                .map(|percent| (available * percent).round())
                                                .sum::<f64>()
                                    } else {
                                        (available * stored_percent).round()
                                    };
                                    Some(if parent_extent > 0. {
                                        allocated / parent_extent
                                    } else {
                                        *stored_percent
                                    })
                                }
                            };
                            let mut node = snapshot(tree, *child, percent, geometries);
                            let titlebar_rows = match layout {
                                Layout::Tabbed | Layout::Stacked
                                    if tree.fullscreen_node().is_some()
                                        && tree.fullscreen_layout_wrappers.contains(&id) =>
                                {
                                    0
                                }
                                Layout::Tabbed => 1,
                                Layout::Stacked => children.len(),
                                Layout::SplitH | Layout::SplitV => 0,
                            };
                            inset_split_by_parent_titlebar(
                                &mut node,
                                tree.titlebar_height * titlebar_rows as f64,
                            );
                            node
                        })
                        .collect(),
                },
                TreeNode::Leaf { tile } => {
                    snapshot_leaf(tree, id, tile, percent, geometries, inside_pending_wrapper)
                }
            }
        }

        let geometries = self.compute_geometry();
        let root = self.resident_root().unwrap_or(self.root);
        snapshot(self, root, None, &geometries)
    }

    /// Which decoration layers the tiling tree collects, front to back.
    ///
    /// Render elements are collected front to back, so an element pushed earlier
    /// is drawn on top. The uncovered top border sits exactly where an inactive
    /// tab's titlebar ring is drawn, so it must be collected before titlebars, or
    /// the ring paints over it and the line breaks under every inactive tab.
    pub(super) const DECORATION_LAYERS: [DecorationLayer; 3] = [
        DecorationLayer::UncoveredTopBorders,
        DecorationLayer::Titlebars,
        DecorationLayer::Tiles,
    ];

    /// Nodes in the order their render elements are collected, front to back.
    ///
    /// The focused node comes first so its decorations sit above sibling shadows. This mirrors
    /// sway's arranged tabbed and stacked scene, where only the active child's border is enabled
    /// (sway/desktop/transaction.c:313-370). Plain depth-first order lets a preceding sibling's
    /// shadow darken the focused border where the two meet.
    pub(super) fn leaf_render_order(
        &self,
        focus: Option<NodeId>,
    ) -> impl Iterator<Item = (NodeId, &TreeNode<W>)> {
        let focused = focus
            .and_then(|id| self.nodes.get(&id).map(|node| (id, &node.value)))
            .into_iter();
        focused.chain(
            self.iter_depth_first()
                .filter(move |(id, _)| Some(*id) != focus),
        )
    }

    pub fn iter_depth_first(&self) -> impl Iterator<Item = (NodeId, &TreeNode<W>)> {
        let mut ids = Vec::new();
        self.collect_depth_first(self.root, &mut ids);
        ids.into_iter()
            .filter_map(|id| self.nodes.get(&id).map(|node| (id, &node.value)))
    }

    pub fn contains_node(&self, ancestor: NodeId, mut id: NodeId) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
                return false;
            };
            id = parent;
        }
    }

    fn collect_depth_first(&self, id: NodeId, ids: &mut Vec<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        ids.push(id);
        if let TreeNode::Split { children, .. } = &node.value {
            for child in children {
                self.collect_depth_first(*child, ids);
            }
        }
    }
}

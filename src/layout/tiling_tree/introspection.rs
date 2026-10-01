use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn visible_window_count(&self) -> usize {
        self.visible_leaves().len()
    }

    pub fn geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.compute_geometry().leaf_boxes.remove(&id)
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

    /// Where a tiled drag drops: the closest edge of the hovered view within
    /// 0.3 * min(w, h) of it, else a swap (sway/input/seatop_move_tiling.c:271-308). Sway's two
    /// earlier passes are not implemented: a drop on a titlebar groups the views as tabs
    /// (L203-218) and a drop within 30 px of an ancestor's perpendicular edge inserts beside
    /// that ancestor (L220-268). Task review2-tiling-drop-target-missing-sway-passes tracks
    /// them; it is blocked on an oracle harness that can drive real pointer motion.
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
        let geometries = self.compute_geometry();
        let snapshot = IpcSnapshot::new(self, &geometries);
        let root = self.resident_root().unwrap_or(self.root);
        snapshot.node(root, None).unwrap_or_else(|| IpcNode::Split {
            id: root,
            layout: Layout::SplitH,
            title: None,
            percent: None,
            rect: Rectangle::default(),
            focus: Vec::new(),
            focused: false,
            fullscreen_mode: 0,
            sticky: false,
            children: Vec::new(),
        })
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

/// One GET_TREE serialization pass. The fullscreen node and the nodes inside a pending
/// fullscreen layout wrapper are computed once here instead of per node.
struct IpcSnapshot<'a, W: LayoutElement> {
    tree: &'a TilingTree<W>,
    geometries: &'a geometry::Geometry<W::Id>,
    fullscreen: Option<NodeId>,
    /// Nodes at or below a pending fullscreen layout wrapper (empty without fullscreen).
    in_pending_wrapper: HashSet<NodeId>,
}

impl<'a, W: LayoutElement> IpcSnapshot<'a, W> {
    fn new(tree: &'a TilingTree<W>, geometries: &'a geometry::Geometry<W::Id>) -> Self {
        let fullscreen = tree.fullscreen_node();
        let mut in_pending_wrapper = HashSet::new();
        if fullscreen.is_some() {
            let mut stack = tree
                .fullscreen_layout_wrappers
                .iter()
                .copied()
                .collect::<Vec<_>>();
            while let Some(id) = stack.pop() {
                if !tree.nodes.contains_key(&id) || !in_pending_wrapper.insert(id) {
                    continue;
                }
                if let Some(TreeNode::Split { children, .. }) =
                    tree.nodes.get(&id).map(|node| &node.value)
                {
                    stack.extend(children.iter().copied());
                }
            }
        }
        Self {
            tree,
            geometries,
            fullscreen,
            in_pending_wrapper,
        }
    }

    /// A pending fullscreen layout wrapper itself: reported with percent 0 and an empty rect.
    fn is_pending_wrapper(&self, id: NodeId) -> bool {
        self.fullscreen.is_some() && self.tree.fullscreen_layout_wrappers.contains(&id)
    }

    fn node(&self, id: NodeId, percent: Option<f64>) -> Option<IpcNode<W::Id>> {
        let inside_pending_wrapper = self.in_pending_wrapper.contains(&id);
        Some(match &self.tree.nodes.get(&id)?.value {
            TreeNode::Split {
                layout,
                children,
                percents,
            } => self.split(id, *layout, children, percents, percent),
            TreeNode::Leaf { tile } => self.leaf(id, tile, percent, inside_pending_wrapper),
        })
    }

    fn split(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
        percent: Option<f64>,
    ) -> IpcNode<W::Id> {
        let tree = self.tree;
        let pending_wrapper = self.is_pending_wrapper(id);
        let child_percents = self.child_percents(id, layout, children, percents);
        let titlebar_rows = self.titlebar_rows(layout, pending_wrapper, children.len());
        IpcNode::Split {
            id,
            layout,
            title: tree.title_formats.get(&id).cloned(),
            percent: pending_wrapper.then_some(0.).or(percent),
            rect: if pending_wrapper {
                Rectangle::default()
            } else if let Some(slot) = tree.fullscreen_tile_slot_rect(id, self.geometries) {
                slot
            } else {
                self.geometries
                    .ipc_nodes
                    .get(&id)
                    .copied()
                    .unwrap_or_default()
            },
            focus: self.focus_order(children),
            focused: tree.focus == Some(id),
            fullscreen_mode: tree.fullscreen_mode(id).map_or(0, |mode| mode as i32),
            sticky: tree.sticky_splits.contains(&id),
            children: children
                .iter()
                .zip(child_percents)
                .filter_map(|(child, percent)| {
                    let mut node = self.node(*child, percent)?;
                    inset_split_by_parent_titlebar(
                        &mut node,
                        tree.titlebar_height * titlebar_rows as f64,
                    );
                    Some(node)
                })
                .collect(),
        }
    }

    /// Children in focus order, most recent first, followed by any never focused. Stale IPC
    /// focus entries (fresh wrappers, see `ipc_stale_nodes`) do not count.
    fn focus_order(&self, children: &[NodeId]) -> Vec<NodeId> {
        let tree = self.tree;
        tree.focus_history
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
            })
    }

    /// Titlebar rows a tabbed or stacked parent reserves above each child's rect.
    fn titlebar_rows(&self, layout: Layout, pending_wrapper: bool, len: usize) -> usize {
        match layout {
            Layout::Tabbed | Layout::Stacked if pending_wrapper => 0,
            Layout::Tabbed => 1,
            Layout::Stacked => len,
            Layout::SplitH | Layout::SplitV => 0,
        }
    }

    /// The `percent` GET_TREE reports for each child, computed once per parent.
    fn child_percents(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
    ) -> Vec<Option<f64>> {
        match layout {
            Layout::Tabbed | Layout::Stacked if self.is_pending_wrapper(id) => {
                vec![None; children.len()]
            }
            Layout::Tabbed | Layout::Stacked => vec![Some(1.); children.len()],
            Layout::SplitH | Layout::SplitV if self.fullscreen.is_some() => {
                self.pending_split_percents(id, layout, children, percents)
            }
            Layout::SplitH | Layout::SplitV => self.split_percents(id, layout, children, percents),
        }
    }

    /// Split percents while a fullscreen view exists. Sway measures against the pre-fullscreen
    /// tile slots, so the rects come from `pre_layout_ipc_rects` or the tiled pass; hidden
    /// children report 0 and the others are renormalised over the visible ones, and the
    /// fullscreen child reports its slot area over the parent's.
    fn pending_split_percents(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
    ) -> Vec<Option<f64>> {
        let tree = self.tree;
        let geometries = self.geometries;
        let rect = |child: &NodeId| {
            tree.pre_layout_ipc_rects
                .get(child)
                .or_else(|| geometries.tiled_ipc_nodes.get(child))
                .copied()
        };
        let parent_rect = rect(&id).unwrap_or_default();
        let parent_extent = geometry::axis_extent(layout, parent_rect).round();
        let allocated = child_shares(parent_extent, percents);
        let excluded = tree.split_excluded();
        let visible_total = children
            .iter()
            .zip(percents)
            .filter(|(child, _)| !excluded.contains(child))
            .map(|(_, percent)| percent)
            .sum::<f64>();
        children
            .iter()
            .zip(percents)
            .zip(allocated)
            .map(|((child, stored_percent), allocated)| {
                let is_fullscreen = self.fullscreen == Some(*child);
                Some(if excluded.contains(child) && !is_fullscreen {
                    0.
                } else if !excluded.is_empty() && !is_fullscreen {
                    *stored_percent / visible_total
                } else if is_fullscreen {
                    let child_rect = tree
                        .fullscreen_tile_slot_rect(*child, geometries)
                        .or_else(|| geometries.ipc_nodes.get(child).copied())
                        .unwrap_or_default();
                    let parent_area = parent_rect.size.w.round() * parent_rect.size.h.round();
                    let child_area = child_rect.size.w.round() * child_rect.size.h.round();
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
            })
            .collect()
    }

    /// Split percents without fullscreen. Sway reports percent as
    /// (w / parent_w) * (h / parent_h) (sway/ipc-json.c:751-754) with child extents
    /// round(fraction * child_total) and the last child taking the remainder
    /// (sway/tree/arrange.c:78-88 and 160-174), so the children share the space left after
    /// inner gaps and the result is taken over the rounded parent extent.
    fn split_percents(
        &self,
        id: NodeId,
        layout: Layout,
        children: &[NodeId],
        percents: &[f64],
    ) -> Vec<Option<f64>> {
        let geometries = self.geometries;
        let parent_extent = geometries
            .ipc_nodes
            .get(&id)
            .copied()
            .map(|rect| geometry::axis_extent(layout, rect).round())
            .unwrap_or_default();
        let available = children
            .iter()
            .filter_map(|child| geometries.ipc_nodes.get(child).copied())
            .map(|rect| geometry::axis_extent(layout, rect))
            .sum::<f64>();
        let allocated = child_shares(available, percents);
        percents
            .iter()
            .zip(allocated)
            .map(|(stored_percent, allocated)| {
                Some(if parent_extent > 0. {
                    allocated / parent_extent
                } else {
                    *stored_percent
                })
            })
            .collect()
    }

    fn leaf(
        &self,
        id: NodeId,
        tile: &crate::layout::tile::Tile<W>,
        percent: Option<f64>,
        inside_pending_wrapper: bool,
    ) -> IpcNode<W::Id> {
        let tree = self.tree;
        let geometries = self.geometries;
        let fallback_titlebar = self.fullscreen.is_some()
            && tree.fullscreen_mode(id).is_none()
            && !tree.mapped_under_fullscreen.contains(&id)
            && !tree.moved_under_fullscreen.contains_key(&id)
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
                    (tree.moved_under_fullscreen.contains_key(&id)
                        && tile.has_configured_sway_titlebar())
                    .then(|| Rectangle::new(Point::default(), (0., tree.titlebar_height).into()))
                })
                .or_else(|| {
                    fallback_titlebar.then(|| {
                        Rectangle::new(Point::default(), (rect.size.w, tree.titlebar_height).into())
                    })
                }),
            border: tile.sway_border_thickness(),
            border_edges: geometries
                .border_edges
                .get(&id)
                .copied()
                .unwrap_or_else(ResizeEdge::all),
            sticky: tile.is_sticky,
            mapped_under_fullscreen: tree.mapped_under_fullscreen.contains(&id),
            moved_under_fullscreen: tree.moved_under_fullscreen.get(&id).copied(),
        }
    }
}

fn inset_split_by_parent_titlebar<I>(node: &mut IpcNode<I>, height: f64) {
    if let IpcNode::Split { rect, .. } = node {
        rect.loc.y += height;
        rect.size.h = (rect.size.h - height).max(0.);
    }
}

/// Each child's whole-pixel share of `extent`: `round(extent * percent)`, with the last child
/// taking the remainder (sway/tree/arrange.c:171-174), clamped at zero for a sub-pixel last
/// child. The prefix sum is accumulated once instead of per child.
fn child_shares(extent: f64, percents: &[f64]) -> Vec<f64> {
    let mut earlier = 0.;
    percents
        .iter()
        .enumerate()
        .map(|(index, percent)| {
            if index + 1 == percents.len() {
                (extent.round() - earlier).max(0.)
            } else {
                let share = (extent * percent).round();
                earlier += share;
                share
            }
        })
        .collect()
}

use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.compute_geometry().leaf_contents.remove(&id)
    }

    pub fn ipc_decoration_rect(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        let id = self.node_for_window(window)?;
        self.compute_geometry()
            .titlebars
            .remove(&id)
            .filter(|bar| bar.visible)
            .map(|bar| bar.ipc_rect)
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
        let (id, rect) = geometries
            .ipc_nodes
            .iter()
            .filter(|(id, rect)| self.tile(**id).is_some() && rect.contains(pos))
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

    pub fn window_for_node(&self, id: NodeId) -> Option<&W> {
        self.tile(id).map(Tile::window)
    }

    pub fn windows(&self) -> impl Iterator<Item = (NodeId, &W)> {
        self.iter_depth_first().filter_map(|(id, node)| match node {
            TreeNode::Leaf { tile } => Some((id, tile.window())),
            TreeNode::Split { .. } => None,
        })
    }

    pub fn ipc_tree(&self) -> IpcNode<W::Id> {
        fn snapshot<W: LayoutElement>(
            tree: &TilingTree<W>,
            id: NodeId,
            percent: Option<f64>,
            geometries: &geometry::Geometry<W::Id>,
        ) -> IpcNode<W::Id> {
            match &tree.nodes[&id].value {
                TreeNode::Split {
                    layout,
                    children,
                    percents,
                } => IpcNode::Split {
                    id,
                    layout: *layout,
                    percent,
                    rect: geometries.ipc_nodes.get(&id).copied().unwrap_or_default(),
                    focus: tree
                        .focus_history
                        .iter()
                        .filter_map(|focused| {
                            children
                                .iter()
                                .copied()
                                .find(|child| tree.is_descendant(*focused, *child))
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
                    children: children
                        .iter()
                        .zip(percents)
                        .map(|(child, percent)| snapshot(tree, *child, Some(*percent), geometries))
                        .collect(),
                },
                TreeNode::Leaf { tile } => IpcNode::Leaf {
                    id,
                    window: tile.window().id().clone(),
                    percent,
                    focused: tree.focus == Some(id),
                    rect: geometries
                        .leaf_contents
                        .get(&id)
                        .copied()
                        .unwrap_or_default(),
                    deco_rect: geometries
                        .titlebars
                        .get(&id)
                        .filter(|bar| bar.visible)
                        .map(|bar| bar.ipc_rect),
                    border: tile.sway_border(),
                    border_edges: geometries
                        .border_edges
                        .get(&id)
                        .copied()
                        .unwrap_or_else(ResizeEdge::all),
                },
            }
        }

        let geometries = self.compute_geometry();
        snapshot(self, self.root, None, &geometries)
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

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

    pub fn titlebar_rects(&self) -> Vec<(W::Id, Rectangle<f64, Logical>)> {
        self.compute_geometry()
            .titlebars
            .into_values()
            .map(|bar| (bar.target, bar.rect))
            .collect()
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

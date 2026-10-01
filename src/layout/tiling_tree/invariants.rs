use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn verify_invariants(&self) {
        self.check_invariants();
    }

    /// Deliberately does NOT assert that the tree holds no squashable split
    /// pair. Sway tolerates one: `cmd_layout` flattens a singleton ancestor and
    /// applies the layout, but never calls `workspace_squash`
    /// (`sway/commands/layout.c` has zero references to it, while
    /// `sway/commands/move.c` calls it at lines 137, 150 and 412 in sway 1.12). So a
    /// squashable pair legitimately survives `layout toggle split` until the
    /// next move. Compaction is therefore driven from the mutation paths in
    /// `compact_tree`, not enforced as a global invariant.
    pub fn check_invariants(&self) {
        self.check_root_and_reachability();
        self.check_focus();
        self.check_side_state();
        self.check_modes();
        self.check_resize();
    }

    fn check_root_and_reachability(&self) {
        assert_eq!(
            self.nodes.get(&self.root).and_then(|node| node.parent),
            None
        );
        assert!(matches!(
            self.nodes.get(&self.root).map(|node| &node.value),
            Some(TreeNode::Split { .. })
        ));
        let mut seen = HashSet::new();
        self.check_node(self.root, &mut seen);
        assert_eq!(seen.len(), self.nodes.len(), "unreachable nodes in arena");
    }

    fn check_focus(&self) {
        if let Some(focus) = self.focus {
            assert!(self.nodes.contains_key(&focus));
            assert!(self.windows().next().is_some());
        } else {
            assert!(self.windows().next().is_none());
        }
        assert_eq!(
            self.focus_history.iter().collect::<HashSet<_>>().len(),
            self.focus_history.len(),
            "focus_history contains duplicate node ids"
        );
    }

    fn check_side_state(&self) {
        // Every NodeId-keyed side collection must join this check and be cleared in remove().
        for (collection, id) in self
            .focus_history
            .iter()
            .map(|id| ("focus_history", id))
            .chain(
                self.ipc_stale_nodes
                    .iter()
                    .map(|id| ("ipc_stale_nodes", id)),
            )
            .chain(
                self.previous_split_layouts
                    .keys()
                    .map(|id| ("previous_split_layouts", id)),
            )
            .chain(self.title_formats.keys().map(|id| ("title_formats", id)))
            .chain(self.sticky_splits.iter().map(|id| ("sticky_splits", id)))
            .chain(self.pending_modes.keys().map(|id| ("pending_modes", id)))
            .chain(
                self.mapped_under_fullscreen
                    .iter()
                    .map(|id| ("mapped_under_fullscreen", id)),
            )
            .chain(
                self.moved_under_fullscreen
                    .keys()
                    .map(|id| ("moved_under_fullscreen", id)),
            )
            .chain(
                self.fullscreen_layout_wrappers
                    .iter()
                    .map(|id| ("fullscreen_layout_wrappers", id)),
            )
            .chain(
                self.pre_layout_ipc_rects
                    .keys()
                    .map(|id| ("pre_layout_ipc_rects", id)),
            )
            .chain(self.tab_indicators.keys().map(|id| ("tab_indicators", id)))
            .chain(self.tab_active.keys().map(|id| ("tab_active", id)))
        {
            assert!(
                self.nodes.contains_key(id),
                "{collection} contains stale node {id:?}"
            );
        }
        assert!(self.tab_active.iter().all(|(parent, child)| {
            matches!(
                self.nodes.get(parent).map(|node| &node.value),
                Some(TreeNode::Split { children, .. }) if children.contains(child)
            )
        }));
    }

    fn check_modes(&self) {
        assert!(self.pending_modes.iter().all(|(id, mode)| {
            self.nodes
                .get(id)
                .is_some_and(|node| matches!(node.value, TreeNode::Leaf { .. }) || !mode.maximized)
        }));
        assert!(
            self.pending_modes
                .values()
                .filter(|mode| mode.fullscreen.is_some())
                .count()
                <= 1,
            "multiple fullscreen nodes"
        );
    }

    fn check_resize(&self) {
        if let Some(resize) = &self.interactive_resize {
            assert_eq!(self.node_for_window(&resize.window), Some(resize.target));
            assert!(!resize.axes.is_empty());
            for axis in &resize.axes {
                assert!(self.sibling_percents(axis.first, axis.second).is_some());
            }
        }
    }

    fn check_node(&self, id: NodeId, seen: &mut HashSet<NodeId>) {
        assert!(seen.insert(id), "cycle or duplicate child at {id:?}");
        let node = self.nodes.get(&id).expect("child missing from arena");
        if let TreeNode::Split {
            children, percents, ..
        } = &node.value
        {
            assert!(
                id == self.root || !children.is_empty(),
                "non-root split must have at least one child"
            );
            assert!(
                id != self.root || !children.is_empty() || self.focus.is_none(),
                "non-empty tree has empty root"
            );
            assert_eq!(children.len(), percents.len());
            assert!(
                percents
                    .iter()
                    .all(|percent| percent.is_finite() && *percent > 0.),
                "degenerate percent in {id:?}: {percents:?}"
            );
            if !children.is_empty() {
                assert!((percents.iter().sum::<f64>() - 1.).abs() <= 1e-6);
            }
            for child in children {
                assert_eq!(
                    self.nodes.get(child).expect("child missing").parent,
                    Some(id)
                );
                self.check_node(*child, seen);
            }
        }
    }
}

use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn verify_invariants(&self) {
        self.check_invariants();
    }

    /// Deliberately does NOT assert that the tree holds no squashable split
    /// pair. Sway tolerates one: `cmd_layout` flattens a singleton ancestor and
    /// applies the layout, but never calls `workspace_squash`
    /// (`sway/commands/layout.c` has zero references to it, while
    /// `sway/commands/move.c` calls it at lines 137, 150 and 412). So a
    /// squashable pair legitimately survives `layout toggle split` until the
    /// next move. Compaction is therefore driven from the mutation paths in
    /// `compact_tree`, not enforced as a global invariant.
    pub fn check_invariants(&self) {
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
        if let Some(focus) = self.focus {
            assert!(self.nodes.contains_key(&focus));
            assert!(self.windows().next().is_some());
        } else {
            assert!(self.windows().next().is_none());
        }
        // Every NodeId-keyed side collection must join this check and be cleared in remove().
        for (collection, id) in self
            .focus_history
            .iter()
            .map(|id| ("focus_history", id))
            .chain(
                self.previous_split_layouts
                    .keys()
                    .map(|id| ("previous_split_layouts", id)),
            )
            .chain(self.pending_modes.keys().map(|id| ("pending_modes", id)))
        {
            assert!(
                self.nodes.contains_key(id),
                "{collection} contains stale node {id:?}"
            );
        }
        assert_eq!(
            self.focus_history.iter().collect::<HashSet<_>>().len(),
            self.focus_history.len(),
            "focus_history contains duplicate node ids"
        );
        assert!(self.pending_modes.iter().all(|(id, mode)| {
            self.nodes
                .get(id)
                .is_some_and(|node| matches!(node.value, TreeNode::Leaf { .. }) || !mode.maximized)
        }));
        if let Some(resize) = &self.interactive_resize {
            assert_eq!(self.node_for_window(&resize.window), Some(resize.target));
            assert!(self.sibling_percents(resize.first, resize.second).is_some());
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

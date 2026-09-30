use super::*;

/// Deepest nesting below a workspace root, counted in edges from the root.
///
/// Sway has no such limit: `container_split` wraps unconditionally
/// (sway/tree/container.c:1508-1560) and the arrange and IPC walks recurse
/// once per level (arrange_container, sway/desktop/transaction.c:392). This
/// tree's geometry, IPC snapshot and transfer walks recurse the same way, on
/// the compositor thread, and a scripted `splitt; focus parent; splitt` loop
/// reaches a stack overflow a few thousand levels down. Refusing to create
/// wrappers past this depth keeps an IPC client from aborting the session;
/// no hand-built layout comes near it.
pub(crate) const MAX_TREE_DEPTH: usize = 64;

/// Command error for an operation refused by [`MAX_TREE_DEPTH`].
pub(crate) const TOO_DEEP: &str = "Container nesting is too deep";

impl<W: LayoutElement> TilingTree<W> {
    /// Number of edges between `id` and the root, walked iteratively.
    pub(super) fn node_depth(&self, mut id: NodeId) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) {
            depth += 1;
            id = parent;
        }
        depth
    }

    /// Levels of containers at and below `id`, walked iteratively.
    pub(super) fn subtree_height(&self, id: NodeId) -> usize {
        let mut height = 0;
        let mut stack = vec![(id, 1)];
        while let Some((id, level)) = stack.pop() {
            height = height.max(level);
            if let Some(TreeNode::Split { children, .. }) =
                self.nodes.get(&id).map(|node| &node.value)
            {
                stack.extend(children.iter().map(|child| (*child, level + 1)));
            }
        }
        height
    }

    #[cfg(test)]
    pub(super) fn tree_depth(&self) -> usize {
        self.subtree_height(self.root).saturating_sub(1)
    }

    /// Whether wrapping `id` in one more container keeps the tree within
    /// [`MAX_TREE_DEPTH`].
    pub(super) fn can_wrap(&self, id: NodeId) -> bool {
        self.node_depth(id) + self.subtree_height(id) <= MAX_TREE_DEPTH
    }

    /// Whether moving the root's children into one new container stays within
    /// [`MAX_TREE_DEPTH`].
    pub(super) fn can_wrap_root_children(&self) -> bool {
        self.subtree_height(self.root) <= MAX_TREE_DEPTH
    }

    /// Whether a subtree of `height` levels fits below `parent`.
    pub(super) fn fits_below(&self, parent: NodeId, height: usize) -> bool {
        self.node_depth(parent) + height <= MAX_TREE_DEPTH
    }

    /// Whether a subtree of `height` levels can replace `id` in place.
    pub fn fits_at(&self, id: NodeId, height: usize) -> bool {
        self.node_depth(id) + height <= MAX_TREE_DEPTH + 1
    }

    /// Levels of containers at and below `id`.
    pub fn node_height(&self, id: NodeId) -> usize {
        self.subtree_height(id)
    }
}

impl<W: LayoutElement> DetachedNode<W> {
    /// Levels of containers in a detached subtree, walked iteratively.
    pub(super) fn height(&self) -> usize {
        let mut height = 0;
        let mut stack = vec![(self, 1)];
        while let Some((node, level)) = stack.pop() {
            height = height.max(level);
            if let Self::Split { children, .. } = node {
                stack.extend(children.iter().map(|child| (child, level + 1)));
            }
        }
        height
    }
}

impl<W: LayoutElement> DetachedSubtree<W> {
    pub fn height(&self) -> usize {
        self.node.height()
    }
}

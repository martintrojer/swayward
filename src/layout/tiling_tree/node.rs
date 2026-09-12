use super::Layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u64);

#[derive(Debug)]
pub enum TreeNode<W> {
    Split {
        layout: Layout,
        children: Vec<NodeId>,
        percents: Vec<f64>,
    },
    Leaf {
        window: W,
    },
}

#[derive(Debug)]
pub(crate) struct Node<W> {
    pub(crate) parent: Option<NodeId>,
    pub(crate) value: TreeNode<W>,
}

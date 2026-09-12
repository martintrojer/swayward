use super::Layout;
use crate::layout::{tile::Tile, LayoutElement};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u64);

#[derive(Debug)]
pub enum TreeNode<W: LayoutElement> {
    Split {
        layout: Layout,
        children: Vec<NodeId>,
        percents: Vec<f64>,
    },
    Leaf {
        tile: Tile<W>,
    },
}

#[derive(Debug)]
pub(crate) struct Node<W: LayoutElement> {
    pub(crate) parent: Option<NodeId>,
    pub(crate) value: TreeNode<W>,
}

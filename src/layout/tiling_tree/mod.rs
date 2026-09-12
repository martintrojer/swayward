mod geometry;
mod node;

use std::collections::{HashMap, HashSet};

use smithay::utils::{Logical, Rectangle, Size};

use super::{LayoutElement, SizingMode};
use crate::utils::transaction::Transaction;

use node::Node;
pub use node::{NodeId, TreeNode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    SplitH,
    SplitV,
    Tabbed,
    Stacked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertTarget {
    Focused,
    Node(NodeId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug)]
pub struct TilingTree<W: LayoutElement> {
    nodes: HashMap<NodeId, Node<W>>,
    root: NodeId,
    next_id: u64,
    focus: Option<NodeId>,
    pending_splits: HashMap<NodeId, Layout>,
    view_size: Size<f64, Logical>,
    gaps: f64,
}

impl<W: LayoutElement> TilingTree<W> {
    pub fn new(view_size: Size<f64, Logical>, gaps: f64) -> Self {
        let root = NodeId(0);
        let nodes = HashMap::from([(
            root,
            Node {
                parent: None,
                value: TreeNode::Split {
                    layout: Layout::SplitH,
                    children: Vec::new(),
                    percents: Vec::new(),
                },
            },
        )]);
        Self {
            nodes,
            root,
            next_id: 1,
            focus: None,
            pending_splits: HashMap::new(),
            view_size,
            gaps,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.focus.is_none()
    }

    pub fn add_window(&mut self, window: W, target: InsertTarget) -> NodeId {
        let id = self.alloc(Node {
            parent: None,
            value: TreeNode::Leaf { window },
        });
        let target = match target {
            InsertTarget::Focused => self.focus,
            InsertTarget::Node(id) => Some(id),
        };
        if let Some(target) = target.filter(|target| self.pending_splits.contains_key(target)) {
            let layout = self.pending_splits.remove(&target).unwrap();
            let parent = self
                .nodes
                .get(&target)
                .and_then(|node| node.parent)
                .unwrap_or(self.root);
            let wrapper = self.alloc(Node {
                parent: Some(parent),
                value: TreeNode::Split {
                    layout,
                    children: vec![target, id],
                    percents: vec![0.5, 0.5],
                },
            });
            if let Some(Node {
                value: TreeNode::Split { children, .. },
                ..
            }) = self.nodes.get_mut(&parent)
            {
                if let Some(index) = children.iter().position(|child| *child == target) {
                    children[index] = wrapper;
                }
            }
            self.nodes.get_mut(&target).unwrap().parent = Some(wrapper);
            self.nodes.get_mut(&id).unwrap().parent = Some(wrapper);
        } else {
            let parent = target
                .and_then(|id| {
                    self.nodes.get(&id).and_then(|node| match &node.value {
                        TreeNode::Split { .. } => Some(id),
                        TreeNode::Leaf { .. } => node.parent,
                    })
                })
                .unwrap_or(self.root);
            let after = target.filter(|target| {
                matches!(
                    self.nodes.get(target).map(|node| &node.value),
                    Some(TreeNode::Leaf { .. })
                ) && self.nodes.get(target).and_then(|node| node.parent) == Some(parent)
            });
            self.insert_child(parent, id, after);
        }
        self.focus = Some(id);
        self.request_window_sizes();
        id
    }

    pub fn remove_window(&mut self, id: NodeId) -> Option<W> {
        let node = self.nodes.remove(&id)?;
        let TreeNode::Leaf { window } = node.value else {
            self.nodes.insert(id, node);
            return None;
        };
        self.pending_splits.remove(&id);
        if let Some(parent) = node.parent {
            self.remove_child(parent, id);
            self.collapse_from(parent);
        }
        if self.focus == Some(id) {
            self.focus = self.first_leaf();
        }
        self.request_window_sizes();
        Some(window)
    }

    pub fn focus(&self) -> Option<NodeId> {
        self.focus
    }

    pub fn set_focus(&mut self, id: NodeId) {
        if matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            self.focus = Some(id);
        }
    }

    pub fn focus_direction(&mut self, dir: Direction) -> bool {
        let Some(current) = self.focus else {
            return false;
        };
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let Some(from) = geometries.get(&current) else {
            return false;
        };
        let from_center = (from.loc.x + from.size.w / 2., from.loc.y + from.size.h / 2.);
        let next = geometries
            .iter()
            .filter(|(id, _)| **id != current)
            .filter_map(|(id, rect)| {
                let center = (rect.loc.x + rect.size.w / 2., rect.loc.y + rect.size.h / 2.);
                let (primary, secondary) = match dir {
                    Direction::Left if center.0 < from_center.0 => {
                        (from_center.0 - center.0, (from_center.1 - center.1).abs())
                    }
                    Direction::Right if center.0 > from_center.0 => {
                        (center.0 - from_center.0, (from_center.1 - center.1).abs())
                    }
                    Direction::Up if center.1 < from_center.1 => {
                        (from_center.1 - center.1, (from_center.0 - center.0).abs())
                    }
                    Direction::Down if center.1 > from_center.1 => {
                        (center.1 - from_center.1, (from_center.0 - center.0).abs())
                    }
                    _ => return None,
                };
                Some((*id, primary + secondary * 2.))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        if let Some(next) = next {
            self.focus = Some(next);
            true
        } else {
            false
        }
    }

    pub fn split(&mut self, id: NodeId, layout: Layout) {
        if matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            self.pending_splits.insert(id, layout);
        } else if let Some(Node {
            value: TreeNode::Split {
                layout: current, ..
            },
            ..
        }) = self.nodes.get_mut(&id)
        {
            *current = layout;
            self.request_window_sizes();
        }
    }

    pub fn set_layout(&mut self, id: NodeId, layout: Layout) {
        if let Some(Node {
            value: TreeNode::Split {
                layout: current, ..
            },
            ..
        }) = self.nodes.get_mut(&id)
        {
            *current = layout;
            self.request_window_sizes();
        } else {
            self.split(id, layout);
        }
    }

    pub fn move_direction(&mut self, id: NodeId, direction: Direction) -> bool {
        if !self.nodes.contains_key(&id) || id == self.root || self.windows().nth(1).is_none() {
            return false;
        }
        let mut branch = id;
        let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
        let mut found_axis = false;
        while let Some(parent_id) = parent {
            let Some(Node {
                parent: grandparent,
                value: TreeNode::Split {
                    layout, children, ..
                },
            }) = self.nodes.get(&parent_id)
            else {
                return false;
            };
            let matching_axis = matches!(
                (layout, direction),
                (Layout::SplitH, Direction::Left | Direction::Right)
                    | (Layout::SplitV, Direction::Up | Direction::Down)
            );
            if matching_axis {
                found_axis = true;
                let index = children
                    .iter()
                    .position(|child| *child == branch)
                    .unwrap_or(0);
                let destination = match direction {
                    Direction::Left | Direction::Up => {
                        index.checked_sub(1).and_then(|index| children.get(index))
                    }
                    Direction::Right | Direction::Down => children.get(index + 1),
                }
                .copied();
                if let Some(destination) = destination {
                    if branch == id {
                        let new_index = match direction {
                            Direction::Left | Direction::Up => index - 1,
                            Direction::Right | Direction::Down => index + 1,
                        };
                        return self.move_subtree_to_index(id, new_index);
                    }
                    self.detach_subtree(id);
                    let Some(destination_parent) =
                        self.nodes.get(&destination).and_then(|node| node.parent)
                    else {
                        return false;
                    };
                    let Some(destination_index) = self.child_index(destination_parent, destination)
                    else {
                        return false;
                    };
                    let insert_index = match direction {
                        Direction::Left | Direction::Up => destination_index,
                        Direction::Right | Direction::Down => destination_index + 1,
                    };
                    self.insert_existing_child(destination_parent, id, insert_index, destination);
                    self.focus = self.first_leaf_in(id).or(self.focus);
                    self.request_window_sizes();
                    return true;
                }
                if parent_id == self.root && branch != id {
                    self.detach_subtree(id);
                    let Some(boundary) = (match &self.nodes.get(&self.root).unwrap().value {
                        TreeNode::Split { children, .. } => match direction {
                            Direction::Left | Direction::Up => children.first(),
                            Direction::Right | Direction::Down => children.last(),
                        },
                        TreeNode::Leaf { .. } => None,
                    })
                    .copied() else {
                        return false;
                    };
                    let insert_index = match direction {
                        Direction::Left | Direction::Up => 0,
                        Direction::Right | Direction::Down => {
                            self.split_len(self.root).unwrap_or(0)
                        }
                    };
                    self.insert_existing_child(self.root, id, insert_index, boundary);
                    self.focus = self.first_leaf_in(id).or(self.focus);
                    self.request_window_sizes();
                    return true;
                }
            }
            branch = parent_id;
            parent = *grandparent;
        }
        if found_axis {
            return false;
        }
        self.detach_subtree(id);
        self.wrap_root_for_direction(id, direction);
        self.focus = self.first_leaf_in(id).or(self.focus);
        self.request_window_sizes();
        true
    }

    pub fn move_subtree_to_first(&mut self, id: NodeId) -> bool {
        self.move_subtree_to_index(id, 0)
    }

    pub fn move_subtree_to_last(&mut self, id: NodeId) -> bool {
        self.move_subtree_to_index(id, usize::MAX)
    }

    pub fn move_subtree_to_index(&mut self, id: NodeId, index: usize) -> bool {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        let Some(old_index) = children.iter().position(|child| *child == id) else {
            return false;
        };
        let new_index = index.min(children.len() - 1);
        if old_index == new_index {
            return false;
        }
        let child = children.remove(old_index);
        let percent = percents.remove(old_index);
        children.insert(new_index, child);
        percents.insert(new_index, percent);
        self.request_window_sizes();
        true
    }

    pub fn resize_adjacent(&mut self, first: NodeId, second: NodeId, delta: f64) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let Some(parent) = self.nodes.get(&first).and_then(|node| node.parent) else {
            return false;
        };
        if self.nodes.get(&second).and_then(|node| node.parent) != Some(parent) {
            return false;
        }
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return false;
        };
        let Some(first_index) = children.iter().position(|child| *child == first) else {
            return false;
        };
        let Some(second_index) = children.iter().position(|child| *child == second) else {
            return false;
        };
        if first_index.abs_diff(second_index) != 1 {
            return false;
        }
        let first_percent = percents[first_index] + delta;
        let second_percent = percents[second_index] - delta;
        if first_percent <= 0. || second_percent <= 0. {
            return false;
        }
        percents[first_index] = first_percent;
        percents[second_index] = second_percent;
        self.request_window_sizes();
        true
    }

    pub fn geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        geometry::compute(&self.nodes, self.root, self.view_size, self.gaps).remove(&id)
    }

    pub fn windows(&self) -> impl Iterator<Item = (NodeId, &W)> {
        self.iter_depth_first().filter_map(|(id, node)| match node {
            TreeNode::Leaf { window } => Some((id, window)),
            TreeNode::Split { .. } => None,
        })
    }

    pub fn iter_depth_first(&self) -> impl Iterator<Item = (NodeId, &TreeNode<W>)> {
        let mut ids = Vec::new();
        self.collect_depth_first(self.root, &mut ids);
        ids.into_iter()
            .filter_map(|id| self.nodes.get(&id).map(|node| (id, &node.value)))
    }

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
            assert!(matches!(
                self.nodes.get(&focus).map(|node| &node.value),
                Some(TreeNode::Leaf { .. })
            ));
        } else {
            assert!(self.windows().next().is_none());
        }
    }

    fn alloc(&mut self, node: Node<W>) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        self.nodes.insert(id, node);
        id
    }

    fn insert_child(&mut self, parent: NodeId, child: NodeId, after: Option<NodeId>) {
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let after_index = after.and_then(|id| children.iter().position(|child| *child == id));
        let index = after_index.map_or(children.len(), |index| index + 1);
        let percent = if let Some(index) = after_index {
            percents[index] /= 2.;
            percents[index]
        } else if children.is_empty() {
            1.
        } else {
            let percent = percents.last().copied().unwrap_or(1.) / 2.;
            *percents.last_mut().unwrap() -= percent;
            percent
        };
        children.insert(index, child);
        percents.insert(index, percent);
        self.nodes.get_mut(&child).unwrap().parent = Some(parent);
    }

    fn child_index(&self, parent: NodeId, child: NodeId) -> Option<usize> {
        match &self.nodes.get(&parent)?.value {
            TreeNode::Split { children, .. } => children.iter().position(|id| *id == child),
            TreeNode::Leaf { .. } => None,
        }
    }

    fn insert_existing_child(
        &mut self,
        parent: NodeId,
        child: NodeId,
        index: usize,
        split_share_of: NodeId,
    ) {
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let Some(target_index) = children.iter().position(|id| *id == split_share_of) else {
            return;
        };
        percents[target_index] /= 2.;
        let percent = percents[target_index];
        let index = index.min(children.len());
        children.insert(index, child);
        percents.insert(index, percent);
        self.nodes.get_mut(&child).unwrap().parent = Some(parent);
    }

    fn detach_subtree(&mut self, id: NodeId) {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return;
        };
        self.remove_child(parent, id);
        self.nodes.get_mut(&id).unwrap().parent = None;
        self.collapse_from(parent);
    }

    fn wrap_root_for_direction(&mut self, id: NodeId, direction: Direction) {
        let layout = match direction {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        };
        let old_value = std::mem::replace(
            &mut self.nodes.get_mut(&self.root).unwrap().value,
            TreeNode::Split {
                layout,
                children: Vec::new(),
                percents: Vec::new(),
            },
        );
        let old = match old_value {
            TreeNode::Split { children, .. } if children.len() == 1 => children[0],
            old_value => {
                let old = self.alloc(Node {
                    parent: Some(self.root),
                    value: old_value,
                });
                if let TreeNode::Split { children, .. } = &self.nodes.get(&old).unwrap().value {
                    for child in children.clone() {
                        self.nodes.get_mut(&child).unwrap().parent = Some(old);
                    }
                }
                old
            }
        };
        self.nodes.get_mut(&old).unwrap().parent = Some(self.root);
        let moving_first = matches!(direction, Direction::Left | Direction::Up);
        let (children, percents) = if moving_first {
            (vec![id, old], vec![0.5, 0.5])
        } else {
            (vec![old, id], vec![0.5, 0.5])
        };
        self.nodes.get_mut(&id).unwrap().parent = Some(self.root);
        self.nodes.get_mut(&self.root).unwrap().value = TreeNode::Split {
            layout,
            children,
            percents,
        };
    }

    fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        if let Some(index) = children.iter().position(|id| *id == child) {
            children.remove(index);
            let removed = percents.remove(index);
            let remaining = 1. - removed;
            if remaining > 0. {
                for percent in percents {
                    *percent /= remaining;
                }
            }
        }
    }

    fn collapse_from(&mut self, mut id: NodeId) {
        loop {
            let (parent, only_child, empty) = match self.nodes.get(&id) {
                Some(Node {
                    parent,
                    value: TreeNode::Split { children, .. },
                }) => (*parent, children.first().copied(), children.is_empty()),
                _ => return,
            };
            if id == self.root {
                return;
            }
            let Some(parent) = parent else { return };
            if empty {
                self.nodes.remove(&id);
                self.remove_child(parent, id);
                id = parent;
                continue;
            }
            let Some(child) = only_child.filter(|_| self.split_len(id) == Some(1)) else {
                return;
            };
            let Some(Node {
                value: TreeNode::Split { children, .. },
                ..
            }) = self.nodes.get_mut(&parent)
            else {
                return;
            };
            let Some(index) = children.iter().position(|node| *node == id) else {
                return;
            };
            children[index] = child;
            self.nodes.get_mut(&child).unwrap().parent = Some(parent);
            self.nodes.remove(&id);
            id = parent;
        }
    }

    fn split_len(&self, id: NodeId) -> Option<usize> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Split { children, .. } => Some(children.len()),
            TreeNode::Leaf { .. } => None,
        }
    }

    fn first_leaf(&self) -> Option<NodeId> {
        self.first_leaf_in(self.root)
    }

    fn first_leaf_in(&self, id: NodeId) -> Option<NodeId> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Leaf { .. } => Some(id),
            TreeNode::Split { children, .. } => {
                children.iter().find_map(|child| self.first_leaf_in(*child))
            }
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

    fn check_node(&self, id: NodeId, seen: &mut HashSet<NodeId>) {
        assert!(seen.insert(id), "cycle or duplicate child at {id:?}");
        let node = self.nodes.get(&id).expect("child missing from arena");
        if let TreeNode::Split {
            children, percents, ..
        } = &node.value
        {
            assert!(
                id == self.root || children.len() >= 2,
                "non-root split must have at least two children"
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

    fn request_window_sizes(&mut self) {
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        for (id, node) in &mut self.nodes {
            if let TreeNode::Leaf { window } = &mut node.value {
                if let Some(rect) = geometries.get(id) {
                    window.request_size(
                        Size::from((rect.size.w.round() as i32, rect.size.h.round() as i32)),
                        SizingMode::Normal,
                        false,
                        None::<Transaction>,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

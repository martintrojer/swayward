mod geometry;
mod node;

use std::collections::{HashMap, HashSet};

use smithay::utils::{Logical, Point, Rectangle, Serial, Size};

use super::tile::Tile;
use super::{ConfigureIntent, InteractiveResizeData, LayoutElement, Options};
use crate::animation::Clock;
use crate::utils::transaction::Transaction;
use crate::utils::ResizeEdge;
use std::rc::Rc;
use std::time::Duration;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingMode {
    fullscreen: bool,
    maximized: bool,
}

#[derive(Debug)]
struct InteractiveResize<I> {
    window: I,
    target: NodeId,
    first: NodeId,
    second: NodeId,
    initial_first: f64,
    initial_second: f64,
    axis_size: f64,
    sign: f64,
    data: InteractiveResizeData,
}

#[derive(Debug)]
pub struct TilingTree<W: LayoutElement> {
    nodes: HashMap<NodeId, Node<W>>,
    root: NodeId,
    next_id: u64,
    focus: Option<NodeId>,
    pending_splits: HashMap<NodeId, Layout>,
    pending_modes: HashMap<NodeId, PendingMode>,
    interactive_resize: Option<InteractiveResize<W::Id>>,
    view_size: Size<f64, Logical>,
    parent_area: Rectangle<f64, Logical>,
    scale: f64,
    clock: Clock,
    options: Rc<Options>,
    gaps: f64,
}

impl<W: LayoutElement> TilingTree<W> {
    pub fn new(
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
    ) -> Self {
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
            pending_modes: HashMap::new(),
            interactive_resize: None,
            view_size,
            parent_area,
            scale,
            clock,
            gaps: options.layout.gaps,
            options,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.focus.is_none()
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        parent_area: Rectangle<f64, Logical>,
        scale: f64,
        options: Rc<Options>,
    ) {
        for tile in self.tiles_mut() {
            tile.update_config(view_size, scale, options.clone());
        }
        self.view_size = view_size;
        self.parent_area = parent_area;
        self.scale = scale;
        self.gaps = options.layout.gaps;
        self.options = options;
        self.request_window_sizes_with(None, false);
    }

    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn parent_area(&self) -> Rectangle<f64, Logical> {
        self.parent_area
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn options(&self) -> &Rc<Options> {
        &self.options
    }

    pub fn add_tile(&mut self, tile: Tile<W>, target: InsertTarget) -> NodeId {
        self.add_tile_with_activation(tile, target, true)
    }

    pub fn add_tile_with_activation(
        &mut self,
        tile: Tile<W>,
        target: InsertTarget,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        let previous_focus = self.focus;
        let id = self.alloc(Node {
            parent: None,
            value: TreeNode::Leaf { tile },
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
        self.focus = if activate {
            Some(id)
        } else {
            previous_focus.or(Some(id))
        };
        self.request_window_sizes();
        id
    }

    pub fn remove_tile_node(&mut self, id: NodeId) -> Option<Tile<W>> {
        let node = self.nodes.remove(&id)?;
        let TreeNode::Leaf { tile } = node.value else {
            self.nodes.insert(id, node);
            return None;
        };
        self.pending_splits.remove(&id);
        self.pending_modes.remove(&id);
        if self
            .interactive_resize
            .as_ref()
            .is_some_and(|resize| resize.target == id || resize.first == id || resize.second == id)
        {
            self.interactive_resize = None;
        }
        if let Some(parent) = node.parent {
            self.remove_child(parent, id);
            self.collapse_from(parent);
        }
        if self.focus == Some(id) {
            self.focus = self.first_leaf();
        }
        Some(tile)
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
        self.interactive_resize = None;
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
        self.interactive_resize = None;
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
        self.interactive_resize = None;
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
        self.interactive_resize = None;
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
        self.focus = self.first_leaf_in(id).or(self.focus);
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

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> {
        self.iter_depth_first().filter_map(|(_, node)| match node {
            TreeNode::Leaf { tile } => Some(tile),
            TreeNode::Split { .. } => None,
        })
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> {
        self.nodes
            .values_mut()
            .filter_map(|node| match &mut node.value {
                TreeNode::Leaf { tile } => Some(tile),
                TreeNode::Split { .. } => None,
            })
    }

    pub fn active_window(&self) -> Option<&W> {
        let id = self.focus?;
        self.tile(id).map(Tile::window)
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        let id = self.focus?;
        self.tile_mut(id).map(Tile::window_mut)
    }

    pub fn active_tile_mut(&mut self) -> Option<&mut Tile<W>> {
        self.focus.and_then(|id| self.tile_mut(id))
    }

    pub fn activate_window(&mut self, window: &W::Id) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        self.focus = Some(id);
        true
    }

    pub fn focus_left(&mut self) -> bool {
        self.focus_direction(Direction::Left)
    }

    pub fn focus_right(&mut self) -> bool {
        self.focus_direction(Direction::Right)
    }

    pub fn focus_up(&mut self) -> bool {
        self.focus_direction(Direction::Up)
    }

    pub fn focus_down(&mut self) -> bool {
        self.focus_direction(Direction::Down)
    }

    pub fn focus_first(&mut self) {
        self.focus = self.first_leaf();
    }

    pub fn focus_last(&mut self) {
        self.focus = self
            .iter_depth_first()
            .filter_map(|(id, node)| matches!(node, TreeNode::Leaf { .. }).then_some(id))
            .last();
    }

    pub fn focus_column(&mut self, index: usize) {
        let branch = self
            .root_children()
            .and_then(|children| children.get(index))
            .copied();
        if let Some(branch) = branch {
            self.focus = self.first_leaf_in(branch).or(self.focus);
        }
    }

    pub fn focus_column_first(&mut self) {
        self.focus_column(0);
    }

    pub fn focus_column_last(&mut self) {
        if let Some(last) = self
            .root_children()
            .and_then(|children| children.len().checked_sub(1))
        {
            self.focus_column(last);
        }
    }

    pub fn focus_top(&mut self) {
        self.focus_extreme(false);
    }

    pub fn focus_bottom(&mut self) {
        self.focus_extreme(true);
    }

    pub fn focus_up_or_left(&mut self) {
        if !self.focus_up() {
            self.focus_left();
        }
    }

    pub fn focus_up_or_right(&mut self) {
        if !self.focus_up() {
            self.focus_right();
        }
    }

    pub fn focus_down_or_left(&mut self) {
        if !self.focus_down() {
            self.focus_left();
        }
    }

    pub fn focus_down_or_right(&mut self) {
        if !self.focus_down() {
            self.focus_right();
        }
    }

    pub fn move_left(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Left))
    }

    pub fn move_right(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Right))
    }

    pub fn move_up(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Up))
    }

    pub fn move_down(&mut self) -> bool {
        self.focus
            .is_some_and(|id| self.move_direction(id, Direction::Down))
    }

    pub fn move_focused_to_first(&mut self) -> bool {
        self.focus
            .and_then(|id| self.root_branch(id))
            .is_some_and(|id| self.move_subtree_to_first(id))
    }

    pub fn move_focused_to_last(&mut self) -> bool {
        self.focus
            .and_then(|id| self.root_branch(id))
            .is_some_and(|id| self.move_subtree_to_last(id))
    }

    pub fn move_focused_to_index(&mut self, index: usize) -> bool {
        self.focus
            .and_then(|id| self.root_branch(id))
            .is_some_and(|id| self.move_subtree_to_index(id, index))
    }

    pub fn remove_tile(&mut self, window: &W::Id, transaction: Transaction) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node(id)?;
        self.request_window_sizes_with(Some(transaction), true);
        Some(tile)
    }

    pub fn set_fullscreen(&mut self, window: &W::Id, fullscreen: bool) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let mode = self.pending_modes.entry(id).or_insert(PendingMode {
            fullscreen: false,
            maximized: false,
        });
        if mode.fullscreen == fullscreen {
            return false;
        }
        mode.fullscreen = fullscreen;
        self.cancel_resize_for(id);
        self.request_window_sizes_with(Some(Transaction::new()), true);
        true
    }

    pub fn set_maximized(&mut self, window: &W::Id, maximized: bool) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let mode = self.pending_modes.entry(id).or_insert(PendingMode {
            fullscreen: false,
            maximized: false,
        });
        if mode.maximized == maximized {
            return false;
        }
        mode.maximized = maximized;
        self.cancel_resize_for(id);
        self.request_window_sizes_with(Some(Transaction::new()), true);
        true
    }

    pub fn is_active_pending_fullscreen(&self) -> bool {
        self.focus
            .and_then(|id| self.pending_modes.get(&id))
            .is_some_and(|mode| mode.fullscreen)
    }

    pub fn is_pending_fullscreen(&self, window: &W::Id) -> bool {
        self.node_for_window(window)
            .and_then(|id| self.pending_modes.get(&id))
            .is_some_and(|mode| mode.fullscreen)
    }

    pub fn is_pending_maximized(&self, window: &W::Id) -> bool {
        self.node_for_window(window)
            .and_then(|id| self.pending_modes.get(&id))
            .is_some_and(|mode| mode.maximized)
    }

    pub fn active_window_visual_rectangle(&self) -> Option<Rectangle<f64, Logical>> {
        let id = self.focus?;
        let tile = self.tile(id)?;
        let mut rect = self.geometry(id)?;
        rect.loc += tile.window_loc();
        rect.size = tile.window_size();
        Rectangle::from_size(self.view_size).intersection(rect)
    }

    pub fn popup_target_rect(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        let id = self.node_for_window(window)?;
        let tile = self.tile(id)?;
        let tile_rect = self.geometry(id)?;
        let mut target =
            Rectangle::from_size(Size::from((tile.window_size().w, self.parent_area.size.h)));
        target.loc.x = tile_rect.loc.x + tile.window_loc().x;
        target.loc.y = self.parent_area.loc.y;
        Some(target)
    }

    pub fn scroll_amount_to_activate(&self, _window: &W::Id) -> f64 {
        0.
    }

    pub fn render_above_top_layer(&self) -> bool {
        self.is_active_pending_fullscreen()
    }

    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        if self.interactive_resize.is_some() {
            return false;
        }
        let Some(id) = self.node_for_window(&window) else {
            return false;
        };
        if self
            .pending_modes
            .get(&id)
            .is_some_and(|mode| mode.fullscreen || mode.maximized)
        {
            return false;
        }
        let horizontal = edges.intersects(ResizeEdge::LEFT_RIGHT);
        let vertical = edges.intersects(ResizeEdge::TOP_BOTTOM);
        let wanted_layout = if horizontal {
            Layout::SplitH
        } else if vertical {
            Layout::SplitV
        } else {
            return false;
        };
        let toward_before = edges.intersects(ResizeEdge::LEFT | ResizeEdge::TOP);
        let Some((first, second, first_percent, second_percent, axis_size, sign)) =
            self.resize_boundary(id, wanted_layout, toward_before)
        else {
            return false;
        };
        self.interactive_resize = Some(InteractiveResize {
            window,
            target: id,
            first,
            second,
            initial_first: first_percent,
            initial_second: second_percent,
            axis_size,
            sign,
            data: InteractiveResizeData { edges },
        });
        true
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        let Some(resize) = &self.interactive_resize else {
            return false;
        };
        if &resize.window != window {
            return false;
        }
        let amount = if resize.data.edges.intersects(ResizeEdge::LEFT_RIGHT) {
            delta.x
        } else {
            delta.y
        } * resize.sign
            / resize.axis_size.max(1.);
        let (first, second) = (resize.first, resize.second);
        let current = self.sibling_percents(first, second);
        let Some((current_first, current_second)) = current else {
            return false;
        };
        let target_first = resize.initial_first + amount;
        let target_second = resize.initial_second - amount;
        let change = target_first - current_first;
        if target_first <= 0. || target_second <= 0. {
            return false;
        }
        let changed = self.resize_adjacent(first, second, change);
        debug_assert!((current_second - change - target_second).abs() <= 1e-6);
        changed
    }

    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        if window.is_none_or(|window| {
            self.interactive_resize
                .as_ref()
                .is_some_and(|resize| &resize.window == window)
        }) {
            self.interactive_resize = None;
        }
    }

    pub fn update_window(&mut self, window: &W::Id, serial: Option<Serial>) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let Some(tile) = self.tile_mut(id) else {
            return false;
        };
        if let Some(serial) = serial {
            tile.window_mut().on_commit(serial);
        }
        tile.update_window();
        true
    }

    pub fn refresh(&mut self, is_active: bool, is_focused: bool) {
        let focus = self.focus;
        let resize = self
            .interactive_resize
            .as_ref()
            .map(|resize| (resize.window.clone(), resize.data));
        let individual =
            self.options.disable_transactions || self.options.disable_resize_throttling;
        let shared_intent = if individual {
            ConfigureIntent::CanSend
        } else {
            self.tiles()
                .fold(ConfigureIntent::NotNeeded, |intent, tile| {
                    match (intent, tile.window().configure_intent()) {
                        (_, ConfigureIntent::ShouldSend) => ConfigureIntent::ShouldSend,
                        (ConfigureIntent::NotNeeded, next) => next,
                        (ConfigureIntent::CanSend, ConfigureIntent::Throttled) => {
                            ConfigureIntent::Throttled
                        }
                        (intent, _) => intent,
                    }
                })
        };
        for (id, node) in &mut self.nodes {
            let TreeNode::Leaf { tile } = &mut node.value else {
                continue;
            };
            let window = tile.window_mut();
            let focused = Some(*id) == focus;
            window.set_active_in_column(focused);
            window.set_floating(false);
            window.set_activated(
                is_active && (!self.options.deactivate_unfocused_windows || focused && is_focused),
            );
            window.set_interactive_resize(
                resize
                    .as_ref()
                    .and_then(|(target, data)| (window.id() == target).then_some(*data)),
            );
            window.set_bounds(self.parent_area.size.to_i32_floor());
            let intent = if individual {
                window.configure_intent()
            } else {
                shared_intent
            };
            if matches!(
                intent,
                ConfigureIntent::CanSend | ConfigureIntent::ShouldSend
            ) {
                window.send_pending_configure();
            }
            window.refresh();
        }
    }

    pub fn view_offset_gesture_begin(&mut self, _is_touchpad: bool) {}

    pub fn view_offset_gesture_update(
        &mut self,
        _delta_x: f64,
        _timestamp: Duration,
        _is_touchpad: bool,
    ) -> Option<bool> {
        None
    }

    pub fn view_offset_gesture_end(&mut self, _is_touchpad: Option<bool>) -> bool {
        false
    }

    pub fn dnd_scroll_gesture_begin(&mut self) {}

    pub fn dnd_scroll_gesture_scroll(&mut self, _delta: f64) -> bool {
        false
    }

    pub fn dnd_scroll_gesture_end(&mut self) {}

    pub fn geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        geometry::compute(&self.nodes, self.root, self.view_size, self.gaps).remove(&id)
    }

    pub fn windows(&self) -> impl Iterator<Item = (NodeId, &W)> {
        self.iter_depth_first().filter_map(|(id, node)| match node {
            TreeNode::Leaf { tile } => Some((id, tile.window())),
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
        assert!(self.pending_modes.keys().all(|id| matches!(
            self.nodes.get(id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        )));
        if let Some(resize) = &self.interactive_resize {
            assert_eq!(self.node_for_window(&resize.window), Some(resize.target));
            assert!(self.sibling_percents(resize.first, resize.second).is_some());
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

    fn root_children(&self) -> Option<&[NodeId]> {
        match &self.nodes.get(&self.root)?.value {
            TreeNode::Split { children, .. } => Some(children),
            TreeNode::Leaf { .. } => None,
        }
    }

    fn root_branch(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            let parent = self.nodes.get(&id)?.parent?;
            if parent == self.root {
                return Some(id);
            }
            id = parent;
        }
    }

    fn focus_extreme(&mut self, bottom: bool) {
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        self.focus = geometries
            .iter()
            .min_by(|(_, a), (_, b)| {
                let a = a.loc.y + if bottom { a.size.h } else { 0. };
                let b = b.loc.y + if bottom { b.size.h } else { 0. };
                if bottom {
                    b.total_cmp(&a)
                } else {
                    a.total_cmp(&b)
                }
            })
            .map(|(id, _)| *id)
            .or(self.focus);
    }

    fn tile(&self, id: NodeId) -> Option<&Tile<W>> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Leaf { tile } => Some(tile),
            TreeNode::Split { .. } => None,
        }
    }

    fn tile_mut(&mut self, id: NodeId) -> Option<&mut Tile<W>> {
        match &mut self.nodes.get_mut(&id)?.value {
            TreeNode::Leaf { tile } => Some(tile),
            TreeNode::Split { .. } => None,
        }
    }

    fn node_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.windows()
            .find_map(|(id, candidate)| (candidate.id() == window).then_some(id))
    }

    fn sibling_percents(&self, first: NodeId, second: NodeId) -> Option<(f64, f64)> {
        let parent = self.nodes.get(&first)?.parent?;
        if self.nodes.get(&second)?.parent != Some(parent) {
            return None;
        }
        let TreeNode::Split {
            children, percents, ..
        } = &self.nodes.get(&parent)?.value
        else {
            return None;
        };
        let first = children.iter().position(|id| *id == first)?;
        let second = children.iter().position(|id| *id == second)?;
        Some((percents[first], percents[second]))
    }

    fn resize_boundary(
        &self,
        id: NodeId,
        layout: Layout,
        toward_before: bool,
    ) -> Option<(NodeId, NodeId, f64, f64, f64, f64)> {
        let mut branch = id;
        let mut parent = self.nodes.get(&id)?.parent;
        while let Some(parent_id) = parent {
            let node = self.nodes.get(&parent_id)?;
            let TreeNode::Split {
                layout: parent_layout,
                children,
                percents,
            } = &node.value
            else {
                return None;
            };
            if *parent_layout == layout {
                let index = children.iter().position(|child| *child == branch)?;
                let neighbor_index = if toward_before {
                    index.checked_sub(1)
                } else {
                    Some(index + 1).filter(|index| *index < children.len())
                };
                if let Some(neighbor_index) = neighbor_index {
                    let neighbor = children[neighbor_index];
                    let axis_size = self.node_geometry(parent_id).map(|rect| {
                        let extent = if layout == Layout::SplitH {
                            rect.size.w
                        } else {
                            rect.size.h
                        };
                        extent - self.gaps * children.len().saturating_sub(1) as f64
                    })?;
                    let first = branch;
                    let second = neighbor;
                    let first_index = children.iter().position(|child| *child == first)?;
                    let second_index = children.iter().position(|child| *child == second)?;
                    let drag_sign = if toward_before { -1. } else { 1. };
                    return Some((
                        first,
                        second,
                        percents[first_index],
                        percents[second_index],
                        axis_size,
                        drag_sign,
                    ));
                }
            }
            branch = parent_id;
            parent = node.parent;
        }
        None
    }

    fn node_geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        if id == self.root {
            return Some(Rectangle::from_size(self.view_size));
        }
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let mut leaves = Vec::new();
        self.collect_leaf_ids(id, &mut leaves);
        let mut rect = *geometries.get(leaves.first()?)?;
        for leaf in &leaves[1..] {
            let next = geometries.get(leaf)?;
            let right = (rect.loc.x + rect.size.w).max(next.loc.x + next.size.w);
            let bottom = (rect.loc.y + rect.size.h).max(next.loc.y + next.size.h);
            rect.loc.x = rect.loc.x.min(next.loc.x);
            rect.loc.y = rect.loc.y.min(next.loc.y);
            rect.size.w = right - rect.loc.x;
            rect.size.h = bottom - rect.loc.y;
        }
        Some(rect)
    }

    fn collect_leaf_ids(&self, id: NodeId, ids: &mut Vec<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        match &node.value {
            TreeNode::Leaf { .. } => ids.push(id),
            TreeNode::Split { children, .. } => {
                for child in children {
                    self.collect_leaf_ids(*child, ids);
                }
            }
        }
    }

    fn cancel_resize_for(&mut self, id: NodeId) {
        if self
            .interactive_resize
            .as_ref()
            .is_some_and(|resize| resize.target == id || resize.first == id || resize.second == id)
        {
            self.interactive_resize = None;
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
        self.request_window_sizes_with(None, false);
    }

    fn request_window_sizes_with(&mut self, transaction: Option<Transaction>, animate: bool) {
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        for (id, node) in &mut self.nodes {
            if let TreeNode::Leaf { tile } = &mut node.value {
                let transaction = transaction.clone();
                if let Some(rect) = geometries.get(id) {
                    let mode = self.pending_modes.get(id).copied().unwrap_or(PendingMode {
                        fullscreen: false,
                        maximized: false,
                    });
                    if mode.fullscreen {
                        tile.request_fullscreen(animate, transaction);
                    } else if mode.maximized {
                        tile.request_maximized(self.parent_area.size, animate, transaction);
                    } else {
                        tile.request_tile_size(rect.size, animate, transaction);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

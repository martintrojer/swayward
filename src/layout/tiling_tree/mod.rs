mod geometry;
mod node;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use node::Node;
pub use node::{NodeId, TreeNode};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size};
use swayward_config::utils::MergeWith as _;
use swayward_config::PresetSize;
use swayward_ipc::{ColumnDisplay, SizeChange, WindowLayout};

use super::closing_window::{ClosingWindow, ClosingWindowRenderElement};
use super::scrolling::ScrollDirection;
use super::tab_indicator::{TabIndicator, TabIndicatorRenderElement, TabInfo};
use super::tile::{Tile, TileRenderElement};
use super::{ConfigureIntent, HitType, InteractiveResizeData, LayoutElement, Options, RenderLayer};
use crate::animation::Clock;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::RenderCtx;
use crate::swayward_render_elements;
use crate::utils::transaction::{Transaction, TransactionBlocker};
use crate::utils::ResizeEdge;
use crate::window::ResolvedWindowRules;

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

#[derive(Debug, Clone, PartialEq)]
pub enum IpcNode<I> {
    Split {
        id: NodeId,
        layout: Layout,
        children: Vec<IpcNode<I>>,
    },
    Leaf {
        id: NodeId,
        window: I,
        rect: Rectangle<f64, Logical>,
    },
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

swayward_render_elements! {
    TilingTreeRenderElement<R> => {
        Tile = TileRenderElement<R>,
        ClosingWindow = ClosingWindowRenderElement,
        TabIndicator = TabIndicatorRenderElement,
    }
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
    tab_indicators: HashMap<NodeId, TabIndicator>,
    tab_active: HashMap<NodeId, NodeId>,
    closing_windows: Vec<ClosingWindow>,
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
            tab_indicators: HashMap::new(),
            tab_active: HashMap::new(),
            closing_windows: Vec::new(),
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
        for indicator in self.tab_indicators.values_mut() {
            indicator.update_config(options.layout.tab_indicator);
        }
        self.view_size = view_size;
        self.parent_area = parent_area;
        self.scale = scale;
        self.gaps = options.layout.gaps;
        self.options = options;
        self.request_window_sizes_with(None, false);
    }

    pub fn update_shaders(&mut self) {
        for tile in self.tiles_mut() {
            tile.update_shaders();
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.update_shaders();
        }
    }

    pub fn advance_animations(&mut self) {
        for tile in self.tiles_mut() {
            tile.advance_animations();
        }
        for indicator in self.tab_indicators.values_mut() {
            indicator.advance_animations();
        }
        self.closing_windows.retain_mut(|closing| {
            closing.advance_animations();
            closing.are_animations_ongoing()
        });
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.tiles().any(Tile::are_animations_ongoing)
            || self
                .tab_indicators
                .values()
                .any(TabIndicator::are_animations_ongoing)
            || !self.closing_windows.is_empty()
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.tiles().any(Tile::are_transitions_ongoing)
            || self
                .tab_indicators
                .values()
                .any(TabIndicator::are_animations_ongoing)
            || !self.closing_windows.is_empty()
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

    pub fn new_window_toplevel_bounds(&self, rules: &ResolvedWindowRules) -> Size<i32, Logical> {
        let border = self.options.layout.border.merged_with(&rules.border);
        let mut size = self.parent_area.size;
        let padding = self.gaps * 2. + if border.off { 0. } else { border.width * 2. };
        size.w = (size.w - padding).max(1.);
        size.h = (size.h - padding).max(1.);
        size.to_i32_floor()
    }

    pub fn new_window_size(
        &self,
        width: Option<PresetSize>,
        height: Option<PresetSize>,
        rules: &ResolvedWindowRules,
    ) -> Size<i32, Logical> {
        let bounds = self.new_window_toplevel_bounds(rules);
        let resolve = |value: Option<PresetSize>, available: i32| match value {
            Some(PresetSize::Fixed(value)) => value.max(1),
            Some(PresetSize::Proportion(value)) => {
                (f64::from(available) * value).floor().max(1.) as i32
            }
            None => available,
        };
        // Tree leaves consume their complete allocated rectangle. The caller still passes niri's
        // default column width, but applying that before insertion would make the first leaf only
        // half-width and prevent the post-insert full-size configure from being sent until ack.
        let _ = width;
        Size::from((bounds.w, resolve(height, bounds.h)))
    }

    pub fn add_tile(&mut self, tile: Tile<W>, target: InsertTarget) -> NodeId {
        self.add_tile_with_activation(tile, target, true)
    }

    pub fn add_tile_right_of(
        &mut self,
        right_of: &W::Id,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        let target = self.node_for_window(right_of)?;
        Some(self.add_tile_with_activation(tile, InsertTarget::Node(target), activate))
    }

    pub fn add_tile_to_subtree(
        &mut self,
        subtree: NodeId,
        tile: Tile<W>,
        activate: bool,
    ) -> Option<NodeId> {
        self.nodes
            .contains_key(&subtree)
            .then(|| self.add_tile_with_activation(tile, InsertTarget::Node(subtree), activate))
    }

    pub fn add_tile_with_activation(
        &mut self,
        mut tile: Tile<W>,
        target: InsertTarget,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        let pending_mode = tile.window().pending_sizing_mode();
        let previous_focus = self.focus;
        let old_geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let id = self.alloc(Node {
            parent: None,
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
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
        if !pending_mode.is_normal() {
            self.pending_modes.insert(
                id,
                PendingMode {
                    fullscreen: pending_mode.is_fullscreen(),
                    maximized: pending_mode.is_maximized(),
                },
            );
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }

    pub fn remove_tile_node(&mut self, id: NodeId) -> Option<Tile<W>> {
        let old_geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
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
        self.animate_geometry_changes(old_geometries, None);
        Some(*tile)
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
        let old = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let changed = self.move_direction_inner(id, direction);
        if changed {
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    fn move_direction_inner(&mut self, id: NodeId, direction: Direction) -> bool {
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
                        return self.move_subtree_to_index_inner(id, new_index);
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
        let old = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let changed = self.move_subtree_to_index_inner(id, index);
        if changed {
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    fn move_subtree_to_index_inner(&mut self, id: NodeId, index: usize) -> bool {
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
        let old = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let changed = self.resize_adjacent_inner(first, second, delta);
        if changed {
            self.animate_geometry_changes(old, None);
        }
        changed
    }

    fn resize_adjacent_inner(&mut self, first: NodeId, second: NodeId, delta: f64) -> bool {
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
            TreeNode::Leaf { tile } => Some(tile.as_ref()),
            TreeNode::Split { .. } => None,
        })
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> {
        self.nodes
            .values_mut()
            .filter_map(|node| match &mut node.value {
                TreeNode::Leaf { tile } => Some(tile.as_mut()),
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

    pub fn focus_window_in_subtree(&mut self, subtree: NodeId, index: usize) {
        let leaf = self.leaf_ids_in(subtree).get(index).copied();
        if let Some(leaf) = leaf {
            self.focus = Some(leaf);
        }
    }

    pub fn focus_window_in_column(&mut self, index: u8) {
        let Some(subtree) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        self.focus_window_in_subtree(subtree, usize::from(index));
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

    pub fn move_column_to_first(&mut self) {
        self.move_focused_to_first();
    }

    pub fn move_column_to_last(&mut self) {
        self.move_focused_to_last();
    }

    pub fn move_column_to_index(&mut self, index: usize) {
        self.move_focused_to_index(index.saturating_sub(1));
    }

    pub fn consume_or_expel_window_left(&mut self, window: Option<&W::Id>) {
        let id = window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus);
        if let Some(id) = id {
            self.focus = Some(id);
            if !self.expel(id, false) {
                self.consume(id, false);
            }
        }
    }

    pub fn consume_or_expel_window_right(&mut self, window: Option<&W::Id>) {
        let id = window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus);
        if let Some(id) = id {
            self.focus = Some(id);
            if !self.expel(id, true) {
                self.consume(id, true);
            }
        }
    }

    pub fn consume_into_column(&mut self) {
        if let Some(id) = self.focus {
            self.consume(id, true);
        }
    }

    pub fn expel_from_column(&mut self) {
        if let Some(id) = self.focus {
            self.expel(id, true);
        }
    }

    pub fn swap_window_in_direction(&mut self, direction: ScrollDirection) {
        match direction {
            ScrollDirection::Left => {
                self.move_left();
            }
            ScrollDirection::Right => {
                self.move_right();
            }
        }
    }

    pub fn toggle_column_tabbed_display(&mut self) {
        let Some(parent) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        let layout = match &self.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split {
                layout: Layout::Tabbed,
                ..
            }) => Layout::SplitH,
            _ => Layout::Tabbed,
        };
        self.set_layout(parent, layout);
    }

    pub fn set_column_display(&mut self, display: ColumnDisplay) {
        let Some(parent) = self.focus.and_then(|id| self.nodes.get(&id)?.parent) else {
            return;
        };
        self.set_layout(
            parent,
            if display == ColumnDisplay::Tabbed {
                Layout::Tabbed
            } else {
                Layout::SplitV
            },
        );
    }

    pub fn center_column(&mut self) {}
    pub fn center_window(&mut self, _window: Option<&W::Id>) {}
    pub fn center_visible_columns(&mut self) {}

    pub fn toggle_width(&mut self, forwards: bool) {
        self.toggle_window_width(None, forwards);
    }

    pub fn toggle_full_width(&mut self) {
        let Some(id) = self.focus else { return };
        let Some(rect) = self.geometry(id) else {
            return;
        };
        self.resize_node_dimension(
            id,
            true,
            SizeChange::AdjustFixed((self.view_size.w - rect.size.w) as i32),
        );
    }

    pub fn set_window_width(&mut self, window: Option<&W::Id>, change: SizeChange) {
        if let Some(id) = self.resolve_node(window) {
            self.resize_node_dimension(id, true, change);
        }
    }

    pub fn set_window_height(&mut self, window: Option<&W::Id>, change: SizeChange) {
        if let Some(id) = self.resolve_node(window) {
            self.resize_node_dimension(id, false, change);
        }
    }

    pub fn reset_window_height(&mut self, window: Option<&W::Id>) {
        let Some(id) = self.resolve_node(window) else {
            return;
        };
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return;
        };
        if let Some(Node {
            value:
                TreeNode::Split {
                    layout: Layout::SplitV,
                    children,
                    percents,
                },
            ..
        }) = self.nodes.get_mut(&parent)
        {
            percents.fill(1. / children.len() as f64);
            self.request_window_sizes();
        }
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, true, forwards);
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        self.toggle_preset(window, false, forwards);
    }

    pub fn expand_column_to_available_width(&mut self) {
        self.toggle_full_width();
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

    pub fn start_open_animation(&mut self, window: &W::Id) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let Some(tile) = self.tile_mut(id) else {
            return false;
        };
        tile.start_open_animation();
        true
    }

    pub fn update_render_elements(&mut self, is_active: bool, layer: RenderLayer) {
        let focus = self.focus;
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let visible = self.visible_leaves();
        for (id, node) in &mut self.nodes {
            let TreeNode::Leaf { tile } = &mut node.value else {
                continue;
            };
            if layer.is_normal() == tile.is_moving_between_workspaces()
                || (!visible.contains(id) && tile.alpha_animation.is_none())
            {
                continue;
            }
            let Some(rect) = geometries.get(id) else {
                continue;
            };
            let mut view_rect = Rectangle::from_size(self.view_size);
            view_rect.loc -= rect.loc + tile.render_offset();
            tile.update_render_elements(is_active && Some(*id) == focus, view_rect);
        }
        self.update_tab_indicators(is_active, &geometries);
    }

    pub fn tiles_with_render_positions(
        &self,
    ) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>, bool)> {
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let visible = self.visible_leaves();
        let scale = self.scale;
        self.iter_depth_first().filter_map(move |(id, node)| {
            let TreeNode::Leaf { tile } = node else {
                return None;
            };
            let rect = geometries.get(&id)?;
            let pos = (rect.loc + tile.render_offset())
                .to_physical_precise_round(scale)
                .to_logical(scale);
            Some((tile.as_ref(), pos, visible.contains(&id)))
        })
    }

    pub fn tiles_with_render_positions_mut(
        &mut self,
        round: bool,
    ) -> impl Iterator<Item = (&mut Tile<W>, Point<f64, Logical>)> {
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let scale = self.scale;
        self.nodes.iter_mut().filter_map(move |(id, node)| {
            let TreeNode::Leaf { tile } = &mut node.value else {
                return None;
            };
            let mut pos = geometries.get(id)?.loc + tile.render_offset();
            if round {
                pos = pos.to_physical_precise_round(scale).to_logical(scale);
            }
            Some((tile.as_mut(), pos))
        })
    }

    pub fn tiles_with_ipc_layouts(&self) -> impl Iterator<Item = (&Tile<W>, WindowLayout)> {
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        self.iter_depth_first().filter_map(move |(id, node)| {
            let TreeNode::Leaf { tile } = node else {
                return None;
            };
            let mut layout = tile.ipc_layout_template();
            layout.tile_pos_in_workspace_view = geometries.get(&id).map(|rect| rect.loc.into());
            Some((tile.as_ref(), layout))
        })
    }

    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<(&W, HitType)> {
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        for (split, indicator) in &self.tab_indicators {
            let Some((area, children)) = self.tab_area(*split, &geometries) else {
                continue;
            };
            if let Some(index) = indicator.hit(area, children.len(), self.scale, pos) {
                if let Some(tile) = children.get(index).and_then(|id| self.first_tile_in(*id)) {
                    return Some((
                        tile.window(),
                        HitType::Activate {
                            is_tab_indicator: true,
                        },
                    ));
                }
            }
        }
        self.tiles_with_render_positions()
            .filter(|(_, _, visible)| *visible)
            .find_map(|(tile, tile_pos, _)| HitType::hit_tile(tile, tile_pos, pos))
    }

    pub fn start_close_animation_for_window(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &W::Id,
        blocker: TransactionBlocker,
    ) {
        let Some(id) = self.node_for_window(window) else {
            return;
        };
        let Some(pos) = self
            .geometry(id)
            .and_then(|rect| self.tile(id).map(|tile| rect.loc + tile.render_offset()))
        else {
            return;
        };
        let Some(tile) = self.tile_mut(id) else {
            return;
        };
        let Some(snapshot) = tile.take_unmap_snapshot() else {
            return;
        };
        let size = tile.tile_size();
        let anim = crate::animation::Animation::new(
            self.clock.clone(),
            0.,
            1.,
            0.,
            self.options.animations.window_close.anim,
        );
        let blocker = if self.options.disable_transactions {
            TransactionBlocker::completed()
        } else {
            blocker
        };
        match ClosingWindow::new(
            renderer,
            snapshot,
            Scale::from(self.scale),
            size,
            pos,
            blocker,
            anim,
        ) {
            Ok(closing) => self.closing_windows.push(closing),
            Err(err) => warn!("error creating a closing window animation: {err:?}"),
        }
    }

    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(TilingTreeRenderElement<R>),
    ) {
        let scale = Scale::from(self.scale);
        if layer.is_normal() {
            let view = Rectangle::from_size(self.view_size);
            for closing in self.closing_windows.iter().rev() {
                push(closing.render(ctx.as_gles(), view, scale).into());
            }
        }
        let focus = self.focus;
        for indicator in self.tab_indicators.values() {
            indicator.render(ctx.renderer, Point::default(), &mut |element| {
                push(element.into())
            });
        }
        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let visible = self.visible_leaves();
        for (id, node) in self.iter_depth_first() {
            let TreeNode::Leaf { tile } = node else {
                continue;
            };
            if (!visible.contains(&id) && tile.alpha_animation.is_none())
                || layer.is_normal() == tile.is_moving_between_workspaces()
            {
                continue;
            }
            let Some(rect) = geometries.get(&id) else {
                continue;
            };
            let tile_pos = (rect.loc + tile.render_offset())
                .to_physical_precise_round(self.scale)
                .to_logical(self.scale);
            let xray = xray_pos.offset(tile_pos);
            tile.render(
                ctx.r(),
                tile_pos,
                xray,
                focus_ring && Some(id) == focus,
                &mut |element| push(element.into()),
            );
        }
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
            let border = self
                .options
                .layout
                .border
                .merged_with(&window.rules().border);
            let padding = self.gaps * 2. + if border.off { 0. } else { border.width * 2. };
            let bounds = Size::from((
                (self.parent_area.size.w - padding).max(1.),
                (self.parent_area.size.h - padding).max(1.),
            ));
            window.set_bounds(bounds.to_i32_floor());
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

    pub fn has_view_offset_gesture(&self) -> bool {
        false
    }

    pub fn view_pos(&self) -> f64 {
        0.
    }

    pub fn active_column_idx(&self) -> usize {
        self.focus
            .and_then(|id| self.root_branch(id))
            .and_then(|branch| self.root_children()?.iter().position(|id| *id == branch))
            .unwrap_or(0)
    }

    pub fn geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        geometry::compute(&self.nodes, self.root, self.view_size, self.gaps).remove(&id)
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
            geometries: &HashMap<NodeId, Rectangle<f64, Logical>>,
        ) -> IpcNode<W::Id> {
            match &tree.nodes[&id].value {
                TreeNode::Split {
                    layout, children, ..
                } => IpcNode::Split {
                    id,
                    layout: *layout,
                    children: children
                        .iter()
                        .map(|child| snapshot(tree, *child, geometries))
                        .collect(),
                },
                TreeNode::Leaf { tile } => IpcNode::Leaf {
                    id,
                    window: tile.window().id().clone(),
                    rect: geometries[&id],
                },
            }
        }

        let geometries = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        snapshot(self, self.root, &geometries)
    }

    pub fn iter_depth_first(&self) -> impl Iterator<Item = (NodeId, &TreeNode<W>)> {
        let mut ids = Vec::new();
        self.collect_depth_first(self.root, &mut ids);
        ids.into_iter()
            .filter_map(|id| self.nodes.get(&id).map(|node| (id, &node.value)))
    }

    pub fn verify_invariants(&self) {
        self.check_invariants();
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

    fn consume(&mut self, id: NodeId, right: bool) -> bool {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        let Some(index) = self.child_index(parent, id) else {
            return false;
        };
        let sibling_index = if right {
            index + 1
        } else {
            let Some(index) = index.checked_sub(1) else {
                return false;
            };
            index
        };
        let Some(sibling) = (match &self.nodes.get(&parent).map(|node| &node.value) {
            Some(TreeNode::Split { children, .. }) => children.get(sibling_index),
            _ => None,
        })
        .copied() else {
            return false;
        };

        let old = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        if self.split_len(parent) == Some(2) {
            let TreeNode::Split {
                layout, children, ..
            } = &mut self.nodes.get_mut(&parent).unwrap().value
            else {
                return false;
            };
            *layout = Layout::SplitV;
            *children = if right {
                vec![sibling, id]
            } else {
                vec![id, sibling]
            };
            self.animate_geometry_changes(old, None);
            self.request_window_sizes();
            return true;
        }

        self.remove_child(parent, id);
        let wrapper = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Split {
                layout: Layout::SplitV,
                children: if right {
                    vec![sibling, id]
                } else {
                    vec![id, sibling]
                },
                percents: vec![0.5, 0.5],
            },
        });
        if let Some(Node {
            value: TreeNode::Split { children, .. },
            ..
        }) = self.nodes.get_mut(&parent)
        {
            if let Some(index) = children.iter().position(|child| *child == sibling) {
                children[index] = wrapper;
            }
        }
        self.nodes.get_mut(&sibling).unwrap().parent = Some(wrapper);
        self.nodes.get_mut(&id).unwrap().parent = Some(wrapper);
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        true
    }

    fn expel(&mut self, id: NodeId, after: bool) -> bool {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        let Some(grandparent) = self.nodes.get(&parent).and_then(|node| node.parent) else {
            return false;
        };
        let old = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        let Some(parent_index) = self.child_index(grandparent, parent) else {
            return false;
        };
        self.remove_child(parent, id);
        let index = parent_index + usize::from(after);
        self.insert_existing_child(grandparent, id, index, parent);
        self.collapse_from(parent);
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        true
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

    fn animate_geometry_changes(
        &mut self,
        old: HashMap<NodeId, Rectangle<f64, Logical>>,
        skip: Option<NodeId>,
    ) {
        let new = geometry::compute(&self.nodes, self.root, self.view_size, self.gaps);
        for (id, old_rect) in old {
            if skip == Some(id) {
                continue;
            }
            let Some(new_rect) = new.get(&id) else {
                continue;
            };
            let offset = old_rect.loc - new_rect.loc;
            if offset != Point::default() {
                if let Some(tile) = self.tile_mut(id) {
                    tile.animate_move_from(offset);
                }
            }
        }
    }

    fn visible_leaves(&self) -> HashSet<NodeId> {
        if let Some(focus) = self.focus {
            if self
                .pending_modes
                .get(&focus)
                .is_some_and(|mode| mode.fullscreen)
            {
                return HashSet::from([focus]);
            }
        }
        let mut visible = HashSet::new();
        self.collect_visible(self.root, &mut visible);
        visible
    }

    fn collect_visible(&self, id: NodeId, visible: &mut HashSet<NodeId>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        match &node.value {
            TreeNode::Leaf { .. } => {
                visible.insert(id);
            }
            TreeNode::Split {
                layout, children, ..
            } => {
                if matches!(layout, Layout::Tabbed | Layout::Stacked) {
                    let focused_branch = self.focus.and_then(|focus| {
                        children
                            .iter()
                            .find(|child| self.contains_node(**child, focus))
                    });
                    if let Some(child) = focused_branch.or_else(|| children.first()) {
                        self.collect_visible(*child, visible);
                    }
                } else {
                    for child in children {
                        self.collect_visible(*child, visible);
                    }
                }
            }
        }
    }

    fn contains_node(&self, ancestor: NodeId, mut id: NodeId) -> bool {
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

    fn first_tile_in(&self, id: NodeId) -> Option<&Tile<W>> {
        self.first_leaf_in(id).and_then(|id| self.tile(id))
    }

    fn tab_area(
        &self,
        id: NodeId,
        geometries: &HashMap<NodeId, Rectangle<f64, Logical>>,
    ) -> Option<(Rectangle<f64, Logical>, Vec<NodeId>)> {
        let TreeNode::Split {
            layout, children, ..
        } = &self.nodes.get(&id)?.value
        else {
            return None;
        };
        if *layout != Layout::Tabbed || children.is_empty() {
            return None;
        }
        let mut area = None;
        for child in children {
            let leaf = self.first_leaf_in(*child)?;
            let rect = *geometries.get(&leaf)?;
            area = Some(area.map_or(rect, |mut area: Rectangle<f64, Logical>| {
                let right = (area.loc.x + area.size.w).max(rect.loc.x + rect.size.w);
                let bottom = (area.loc.y + area.size.h).max(rect.loc.y + rect.size.h);
                area.loc.x = area.loc.x.min(rect.loc.x);
                area.loc.y = area.loc.y.min(rect.loc.y);
                area.size.w = right - area.loc.x;
                area.size.h = bottom - area.loc.y;
                area
            }));
        }
        Some((area?, children.clone()))
    }

    fn update_tab_indicators(
        &mut self,
        is_active: bool,
        geometries: &HashMap<NodeId, Rectangle<f64, Logical>>,
    ) {
        let tabbed: Vec<_> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| match &node.value {
                TreeNode::Split {
                    layout: Layout::Tabbed,
                    children,
                    ..
                } => Some((*id, children.clone())),
                _ => None,
            })
            .collect();
        self.tab_indicators
            .retain(|id, _| tabbed.iter().any(|(tabbed, _)| tabbed == id));
        self.tab_active
            .retain(|id, _| tabbed.iter().any(|(tabbed, _)| tabbed == id));
        for (id, children) in tabbed {
            let Some((area, _)) = self.tab_area(id, geometries) else {
                continue;
            };
            let active = self
                .focus
                .and_then(|focus| {
                    children
                        .iter()
                        .find(|child| self.contains_node(**child, focus))
                })
                .copied()
                .or_else(|| children.first().copied());
            if self.tab_active.get(&id).copied() != active {
                let movement = self.options.animations.window_movement.0;
                let previous = self.tab_active.insert(id, active.unwrap_or(id));
                for child in &children {
                    let Some(leaf) = self.first_leaf_in(*child) else {
                        continue;
                    };
                    if let Some(tile) = self.tile_mut(leaf) {
                        if Some(*child) == active {
                            tile.ensure_alpha_animates_to_1();
                        } else if previous.is_none() || previous == Some(*child) {
                            tile.animate_alpha(1., 0., movement);
                        }
                    }
                }
            }
            let config = self.options.layout.tab_indicator;
            let tabs: Vec<_> = children
                .iter()
                .filter_map(|child| {
                    let leaf = self.first_leaf_in(*child)?;
                    let tile = self.tile(leaf)?;
                    let rect = geometries.get(&leaf)?;
                    Some(TabInfo::from_tile(
                        tile,
                        rect.loc,
                        self.focus
                            .is_some_and(|focus| self.contains_node(*child, focus)),
                        tile.window().is_urgent(),
                        &config,
                    ))
                })
                .collect();
            let is_new = !self.tab_indicators.contains_key(&id);
            let indicator = self
                .tab_indicators
                .entry(id)
                .or_insert_with(|| TabIndicator::new(config));
            if is_new {
                indicator.start_open_animation(
                    self.clock.clone(),
                    self.options.animations.window_open.anim,
                );
            }
            indicator.update_render_elements(
                true,
                area,
                Rectangle::from_size(self.view_size),
                tabs.len(),
                tabs.into_iter(),
                is_active,
                self.scale,
            );
        }
    }

    fn resolve_node(&self, window: Option<&W::Id>) -> Option<NodeId> {
        window
            .and_then(|window| self.node_for_window(window))
            .or(self.focus)
    }

    fn toggle_preset(&mut self, window: Option<&W::Id>, width: bool, forwards: bool) {
        let presets = if width {
            &self.options.layout.preset_column_widths
        } else {
            &self.options.layout.preset_window_heights
        };
        if presets.is_empty() {
            return;
        }
        let index = if forwards { 0 } else { presets.len() - 1 };
        let change = match presets[index] {
            PresetSize::Fixed(value) => SizeChange::SetFixed(value),
            PresetSize::Proportion(value) => SizeChange::SetProportion(value * 100.),
        };
        if width {
            self.set_window_width(window, change);
        } else {
            self.set_window_height(window, change);
        }
    }

    fn resize_node_dimension(&mut self, id: NodeId, width: bool, change: SizeChange) {
        let wanted = if width {
            Layout::SplitH
        } else {
            Layout::SplitV
        };
        let Some(rect) = self.geometry(id) else {
            return;
        };
        let current = if width { rect.size.w } else { rect.size.h };
        let total = if width {
            self.view_size.w
        } else {
            self.view_size.h
        };
        let target = match change {
            SizeChange::SetFixed(value) => f64::from(value),
            SizeChange::SetProportion(value) => total * value / 100.,
            SizeChange::AdjustFixed(value) => current + f64::from(value),
            SizeChange::AdjustProportion(value) => current + total * value / 100.,
        };
        let mut branch = id;
        let mut parent = self.nodes.get(&id).and_then(|node| node.parent);
        while let Some(parent_id) = parent {
            let Some(Node {
                parent: grandparent,
                value: TreeNode::Split {
                    layout, children, ..
                },
            }) = self.nodes.get(&parent_id)
            else {
                return;
            };
            if *layout == wanted {
                let Some(index) = children.iter().position(|child| *child == branch) else {
                    return;
                };
                let neighbor = children.get(index + 1).copied().or_else(|| {
                    index
                        .checked_sub(1)
                        .and_then(|index| children.get(index).copied())
                });
                let Some(neighbor) = neighbor else { return };
                let extent = self
                    .node_geometry(parent_id)
                    .map(|rect| if width { rect.size.w } else { rect.size.h })
                    .unwrap_or(total)
                    .max(1.);
                let delta = (target - current) / extent;
                self.resize_adjacent(branch, neighbor, delta);
                return;
            }
            branch = parent_id;
            parent = *grandparent;
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

    fn leaf_ids_in(&self, id: NodeId) -> Vec<NodeId> {
        let mut ids = Vec::new();
        self.collect_leaf_ids(id, &mut ids);
        ids
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

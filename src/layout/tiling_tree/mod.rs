mod focus;
mod fullscreen;
mod geometry;
mod introspection;
mod invariants;
mod movement;
mod node;
mod rendering;
mod resize;
mod tree_layout;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use geometry::apply_struts;
use node::Node;
pub use node::{NodeId, TreeNode};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size};
use swayward_config::utils::MergeWith as _;
use swayward_config::PresetSize;
use swayward_ipc::command::{LayoutToggle, LayoutToggleEntry};
use swayward_ipc::{ColumnDisplay, SizeChange, WindowLayout};

use super::closing_window::{ClosingWindow, ClosingWindowRenderElement};
use super::scrolling::ScrollDirection;
use super::tab_indicator::{TabIndicator, TabIndicatorRenderElement, TabInfo};
use super::tile::{Tile, TileRenderElement};
use super::titlebar::TitlebarState;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcNodeKind {
    Split,
    Leaf,
}

#[derive(Debug)]
pub struct DetachedSubtree<W: LayoutElement> {
    node: DetachedNode<W>,
    focus_history: Vec<W::Id>,
}

#[derive(Debug, Clone, Copy)]
pub struct DetachedSlot {
    parent: NodeId,
    index: usize,
    percent: f64,
    focus_rank: usize,
    focused: bool,
}

#[derive(Debug)]
enum DetachedNode<W: LayoutElement> {
    Split {
        old_id: NodeId,
        layout: Layout,
        children: Vec<DetachedNode<W>>,
        percents: Vec<f64>,
        previous_layout: Option<Layout>,
        pending_mode: Option<PendingMode>,
    },
    Leaf {
        old_id: NodeId,
        tile: Box<Tile<W>>,
        pending_mode: Option<PendingMode>,
    },
}

impl<W: LayoutElement> DetachedNode<W> {
    fn has_fullscreen(&self) -> bool {
        match self {
            Self::Split {
                children,
                pending_mode,
                ..
            } => {
                pending_mode.is_some_and(|mode| mode.fullscreen.is_some())
                    || children.iter().any(Self::has_fullscreen)
            }
            Self::Leaf { pending_mode, .. } => {
                pending_mode.is_some_and(|mode| mode.fullscreen.is_some())
            }
        }
    }

    fn for_each_window(&self, f: &mut impl FnMut(&W)) {
        match self {
            Self::Split { children, .. } => {
                for child in children {
                    child.for_each_window(f);
                }
            }
            Self::Leaf { tile, .. } => f(tile.window()),
        }
    }
}

impl<I> IpcNode<I> {
    pub fn retain_leaves(&mut self, keep: &impl Fn(&I) -> bool) {
        if let Self::Split { children, .. } = self {
            children.retain_mut(|child| match child {
                Self::Leaf { window, .. } => keep(window),
                Self::Split { .. } => {
                    child.retain_leaves(keep);
                    !matches!(child, Self::Split { children, .. } if children.is_empty())
                }
            });
        }
    }
}

impl<W: LayoutElement> DetachedSubtree<W> {
    pub fn for_each_window(&self, mut f: impl FnMut(&W)) {
        self.node.for_each_window(&mut f);
    }

    pub fn has_fullscreen(&self) -> bool {
        self.node.has_fullscreen()
    }

    pub fn swap_root_mode(&mut self, other: &mut Self) {
        fn mode<W: LayoutElement>(node: &mut DetachedNode<W>) -> &mut Option<PendingMode> {
            match node {
                DetachedNode::Split { pending_mode, .. }
                | DetachedNode::Leaf { pending_mode, .. } => pending_mode,
            }
        }
        std::mem::swap(mode(&mut self.node), mode(&mut other.node));
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IpcNode<I> {
    Split {
        id: NodeId,
        layout: Layout,
        percent: Option<f64>,
        rect: Rectangle<f64, Logical>,
        focus: Vec<NodeId>,
        focused: bool,
        fullscreen_mode: i32,
        children: Vec<IpcNode<I>>,
    },
    Leaf {
        id: NodeId,
        window: I,
        percent: Option<f64>,
        focused: bool,
        rect: Rectangle<f64, Logical>,
        deco_rect: Option<Rectangle<f64, Logical>>,
        border: (swayward_ipc::command::BorderStyle, u16),
    },
}

impl<I> IpcNode<I> {
    pub fn nodes(&self) -> Vec<(NodeId, IpcNodeKind)> {
        fn collect<I>(node: &IpcNode<I>, nodes: &mut Vec<(NodeId, IpcNodeKind)>) {
            match node {
                IpcNode::Split { id, children, .. } => {
                    nodes.push((*id, IpcNodeKind::Split));
                    for child in children {
                        collect(child, nodes);
                    }
                }
                IpcNode::Leaf { id, .. } => nodes.push((*id, IpcNodeKind::Leaf)),
            }
        }
        let mut nodes = Vec::new();
        collect(self, &mut nodes);
        nodes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenMode {
    Workspace = 1,
    Global = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingMode {
    fullscreen: Option<FullscreenMode>,
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
        Titlebar = crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement,
    }
}

#[derive(Debug)]
pub struct TilingTree<W: LayoutElement> {
    nodes: HashMap<NodeId, Node<W>>,
    root: NodeId,
    next_id: u64,
    focus: Option<NodeId>,
    focus_history: Vec<NodeId>,
    previous_split_layouts: HashMap<NodeId, Layout>,
    pending_modes: HashMap<NodeId, PendingMode>,
    interactive_resize: Option<InteractiveResize<W::Id>>,
    tab_indicators: HashMap<NodeId, TabIndicator>,
    titlebars: super::titlebar::TitlebarRenderer,
    tab_active: HashMap<NodeId, NodeId>,
    closing_windows: Vec<ClosingWindow>,
    view_size: Size<f64, Logical>,
    parent_area: Rectangle<f64, Logical>,
    scale: f64,
    titlebar_height: f64,
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
            focus_history: Vec::new(),
            previous_split_layouts: HashMap::new(),
            pending_modes: HashMap::new(),
            interactive_resize: None,
            tab_indicators: HashMap::new(),
            titlebars: Default::default(),
            tab_active: HashMap::new(),
            closing_windows: Vec::new(),
            view_size,
            parent_area,
            scale,
            titlebar_height: super::titlebar::height(scale, &options.layout.titlebar),
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
        self.titlebar_height = super::titlebar::height(scale, &options.layout.titlebar);
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
        let mut size = self.working_area().size;
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
        let old_geometries = self.compute_geometry();
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
        let parent = target
            .and_then(|id| self.nodes.get(&id)?.parent)
            .unwrap_or(self.root);
        let after = target
            .filter(|target| self.nodes.get(target).and_then(|node| node.parent) == Some(parent));
        self.insert_child(parent, id, after);
        if parent == self.root {
            if let Some(layout) = match self.options.layout.workspace_layout {
                swayward_config::WorkspaceLayout::Default => None,
                swayward_config::WorkspaceLayout::Stacking => Some(Layout::Stacked),
                swayward_config::WorkspaceLayout::Tabbed => Some(Layout::Tabbed),
            } {
                self.wrap_node(id, layout);
            }
        }
        if activate {
            self.set_focus_id(Some(id));
        } else if let Some(previous_focus) = previous_focus {
            self.focus_history.retain(|candidate| *candidate != id);
            self.focus_history
                .insert(1.min(self.focus_history.len()), id);
            self.focus = Some(previous_focus);
        } else {
            self.set_focus_id(Some(id));
        }
        if !pending_mode.is_normal() {
            self.pending_modes.insert(
                id,
                PendingMode {
                    fullscreen: pending_mode
                        .is_fullscreen()
                        .then_some(FullscreenMode::Workspace),
                    maximized: pending_mode.is_maximized(),
                },
            );
        }
        self.compact_tree();
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }

    pub fn detach_subtree_for_swap(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, DetachedSlot)> {
        if id == self.root {
            return None;
        }
        let parent = self.nodes.get(&id)?.parent?;
        let index = self.child_index(parent, id)?;
        let percent = match &self.nodes.get(&parent)?.value {
            TreeNode::Split { percents, .. } => *percents.get(index)?,
            TreeNode::Leaf { .. } => return None,
        };
        let focus_rank = self
            .focus_history
            .iter()
            .position(|candidate| self.is_descendant(*candidate, id))
            .unwrap_or(self.focus_history.len());
        let focused = self
            .focus
            .is_some_and(|focus| self.is_descendant(focus, id));
        let (subtree, _) = self.detach_subtree(id)?;
        Some((
            subtree,
            DetachedSlot {
                parent,
                index,
                percent,
                focus_rank,
                focused,
            },
        ))
    }

    pub fn attach_subtree_for_swap(
        &mut self,
        subtree: DetachedSubtree<W>,
        slot: DetachedSlot,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        let mut remapped = Vec::new();
        let id = self.insert_detached_node(subtree.node, None, &mut remapped);
        let TreeNode::Split {
            children, percents, ..
        } = &mut self.nodes.get_mut(&slot.parent).unwrap().value
        else {
            unreachable!()
        };
        let index = slot.index.min(children.len());
        children.insert(index, id);
        percents.insert(index, slot.percent);
        self.nodes.get_mut(&id).unwrap().parent = Some(slot.parent);
        for window in subtree.focus_history.into_iter().rev() {
            if let Some(leaf) = self.node_for_window(&window) {
                self.focus_history.retain(|candidate| *candidate != leaf);
                self.focus_history
                    .insert(slot.focus_rank.min(self.focus_history.len()), leaf);
            }
        }
        if slot.focused {
            self.focus = self.focused_leaf_in(id);
        }
        self.request_window_sizes();
        (id, remapped)
    }

    pub fn detach_subtree(&mut self, id: NodeId) -> Option<(DetachedSubtree<W>, Option<NodeId>)> {
        let leaves = self.leaf_ids_in(id);
        let focus_history = self
            .focus_history
            .iter()
            .filter(|candidate| leaves.contains(candidate))
            .filter_map(|leaf| self.tile(*leaf).map(|tile| tile.window().id().clone()))
            .collect();
        let parent = if id == self.root {
            None
        } else {
            let parent = self.detach_subtree_only(id)?;
            Some(parent)
        };
        let node = if id == self.root {
            let TreeNode::Split {
                layout,
                children,
                percents,
            } = std::mem::replace(
                &mut self.nodes.get_mut(&self.root)?.value,
                TreeNode::Split {
                    layout: Layout::SplitH,
                    children: Vec::new(),
                    percents: Vec::new(),
                },
            )
            else {
                return None;
            };
            let children = children
                .into_iter()
                .map(|child| self.take_detached_node(child))
                .collect::<Option<Vec<_>>>()?;
            DetachedNode::Split {
                old_id: id,
                layout,
                children,
                percents,
                previous_layout: None,
                pending_mode: None,
            }
        } else {
            self.take_detached_node(id)?
        };
        self.focus = self.focused_leaf_in(self.root);
        self.request_window_sizes();
        Some((
            DetachedSubtree {
                node,
                focus_history,
            },
            parent,
        ))
    }

    pub fn attach_subtree(
        &mut self,
        subtree: DetachedSubtree<W>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        let target = self.focus;
        self.attach_subtree_at(subtree, target)
    }

    pub fn attach_subtree_at(
        &mut self,
        subtree: DetachedSubtree<W>,
        target: Option<NodeId>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        let focus_history = subtree.focus_history;
        let mut remapped = Vec::new();
        let id = self.insert_detached_node(subtree.node, None, &mut remapped);
        let (parent, after) = match target.and_then(|target| self.nodes.get(&target)) {
            Some(Node {
                parent: Some(parent),
                value: TreeNode::Leaf { .. },
            }) => (*parent, target),
            Some(Node {
                value: TreeNode::Split { .. },
                ..
            }) => (target.unwrap(), None),
            _ => (self.root, None),
        };
        self.insert_child(parent, id, after);
        let insertion = usize::from(self.focus.is_some());
        for window in focus_history.into_iter().rev() {
            if let Some(leaf) = self.node_for_window(&window) {
                self.focus_history.retain(|candidate| *candidate != leaf);
                self.focus_history
                    .insert(insertion.min(self.focus_history.len()), leaf);
            }
        }
        self.request_window_sizes();
        (id, remapped)
    }

    pub fn finish_subtree_detach(&mut self, old_parent: Option<NodeId>) {
        if let Some(parent) = old_parent {
            self.reap_empty_from(parent);
        }
        self.compact_tree();
        self.focus = self.focused_leaf_in(self.root);
        self.request_window_sizes();
    }

    fn take_detached_node(&mut self, id: NodeId) -> Option<DetachedNode<W>> {
        let previous_layout = self.previous_split_layouts.get(&id).copied();
        let pending_mode = self.pending_modes.get(&id).copied();
        let node = self.remove_node(id)?;
        match node.value {
            TreeNode::Split {
                layout,
                children,
                percents,
            } => Some(DetachedNode::Split {
                old_id: id,
                layout,
                children: children
                    .into_iter()
                    .map(|child| self.take_detached_node(child))
                    .collect::<Option<Vec<_>>>()?,
                percents,
                previous_layout,
                pending_mode,
            }),
            TreeNode::Leaf { tile } => Some(DetachedNode::Leaf {
                old_id: id,
                tile,
                pending_mode,
            }),
        }
    }

    fn insert_detached_node(
        &mut self,
        node: DetachedNode<W>,
        parent: Option<NodeId>,
        remapped: &mut Vec<(NodeId, NodeId)>,
    ) -> NodeId {
        match node {
            DetachedNode::Split {
                old_id,
                layout,
                children,
                percents,
                previous_layout,
                pending_mode,
            } => {
                let id = self.alloc(Node {
                    parent,
                    value: TreeNode::Split {
                        layout,
                        children: Vec::new(),
                        percents,
                    },
                });
                remapped.push((old_id, id));
                let children = children
                    .into_iter()
                    .map(|child| self.insert_detached_node(child, Some(id), remapped))
                    .collect();
                let TreeNode::Split { children: slot, .. } =
                    &mut self.nodes.get_mut(&id).unwrap().value
                else {
                    unreachable!();
                };
                *slot = children;
                if let Some(layout) = previous_layout {
                    self.previous_split_layouts.insert(id, layout);
                }
                if let Some(mode) = pending_mode {
                    self.pending_modes.insert(id, mode);
                }
                id
            }
            DetachedNode::Leaf {
                old_id,
                mut tile,
                pending_mode,
            } => {
                tile.update_config(self.view_size, self.scale, self.options.clone());
                let id = self.alloc(Node {
                    parent,
                    value: TreeNode::Leaf { tile },
                });
                remapped.push((old_id, id));
                if let Some(mode) = pending_mode {
                    self.pending_modes.insert(id, mode);
                }
                id
            }
        }
    }

    pub fn remove_tile_node(&mut self, id: NodeId) -> Option<Tile<W>> {
        self.remove_tile_node_inner(id, true)
    }

    fn remove_tile_node_preserving_parent(&mut self, id: NodeId) -> Option<Tile<W>> {
        self.remove_tile_node_inner(id, false)
    }

    fn remove_tile_node_inner(&mut self, id: NodeId, collapse: bool) -> Option<Tile<W>> {
        let old_geometries = self.compute_geometry();
        if !matches!(
            self.nodes.get(&id).map(|node| &node.value),
            Some(TreeNode::Leaf { .. })
        ) {
            return None;
        }
        let node = self.remove_node(id)?;
        let TreeNode::Leaf { tile } = node.value else {
            unreachable!();
        };
        if self
            .interactive_resize
            .as_ref()
            .is_some_and(|resize| resize.target == id || resize.first == id || resize.second == id)
        {
            self.interactive_resize = None;
        }
        if let Some(parent) = node.parent {
            self.remove_child(parent, id);
            if collapse {
                self.collapse_from(parent);
                self.compact_tree();
            }
        }
        if self.windows().next().is_none() {
            self.pending_modes.clear();
            self.set_focus_id(None);
        } else if self.focus == Some(id) {
            self.set_focus_id(
                self.fullscreen_node()
                    .and_then(|fullscreen| self.focused_leaf_in(fullscreen))
                    .or_else(|| self.focused_leaf_in(self.root)),
            );
        }
        self.animate_geometry_changes(old_geometries, None);
        Some(*tile)
    }

    fn remove_node(&mut self, id: NodeId) -> Option<Node<W>> {
        let node = self.nodes.remove(&id)?;
        self.previous_split_layouts.remove(&id);
        self.pending_modes.remove(&id);
        self.focus_history.retain(|candidate| *candidate != id);
        Some(node)
    }

    fn set_focus_id(&mut self, focus: Option<NodeId>) {
        self.focus = focus;
        if let Some(id) = focus {
            self.focus_history.retain(|candidate| *candidate != id);
            self.focus_history.insert(0, id);
        }
    }

    fn focused_child_in(&self, parent: NodeId) -> Option<NodeId> {
        let TreeNode::Split { children, .. } = &self.nodes.get(&parent)?.value else {
            return None;
        };
        self.focus_history
            .iter()
            .find_map(|focused| {
                children
                    .iter()
                    .copied()
                    .find(|child| self.is_descendant(*focused, *child))
            })
            .or_else(|| children.first().copied())
    }

    pub fn remove_tile(&mut self, window: &W::Id, transaction: Transaction) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node(id)?;
        self.request_window_sizes_with(Some(transaction), true);
        Some(tile)
    }

    pub fn remove_tile_preserving_parent(&mut self, window: &W::Id) -> Option<Tile<W>> {
        let id = self.node_for_window(window)?;
        let tile = self.remove_tile_node_preserving_parent(id)?;
        self.request_window_sizes();
        Some(tile)
    }

    pub fn add_tile_to_existing_parent(
        &mut self,
        mut tile: Tile<W>,
        parent: NodeId,
        activate: bool,
    ) -> NodeId {
        self.interactive_resize = None;
        tile.update_config(self.view_size, self.scale, self.options.clone());
        let old_geometries = self.compute_geometry();
        let id = self.alloc(Node {
            parent: Some(parent),
            value: TreeNode::Leaf {
                tile: Box::new(tile),
            },
        });
        self.insert_child(parent, id, None);
        if activate {
            self.set_focus_id(Some(id));
        }
        self.animate_geometry_changes(old_geometries, Some(id));
        self.request_window_sizes();
        id
    }

    fn working_area(&self) -> Rectangle<f64, Logical> {
        apply_struts(self.parent_area, self.scale, self.options.layout.struts)
    }

    fn compute_geometry(&self) -> geometry::Geometry<W::Id> {
        let fullscreen = self.fullscreen_node().into_iter().collect();
        geometry::compute(
            &self.nodes,
            self.root,
            self.view_size,
            self.parent_area,
            self.scale,
            self.options.layout.struts,
            self.gaps,
            self.titlebar_height,
            &fullscreen,
        )
    }

    fn is_descendant(&self, mut id: NodeId, ancestor: NodeId) -> bool {
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

        let old = self.compute_geometry();
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
        let sibling_percent = match &self.nodes[&parent].value {
            TreeNode::Split {
                children, percents, ..
            } => children
                .iter()
                .position(|child| *child == sibling)
                .map(|index| percents[index]),
            TreeNode::Leaf { .. } => None,
        };
        let Some(sibling_percent) = sibling_percent else {
            return false;
        };
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
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        {
            if let Some(index) = children.iter().position(|child| *child == sibling) {
                children[index] = wrapper;
                percents[index] = sibling_percent;
            }
        }
        self.nodes.get_mut(&sibling).unwrap().parent = Some(wrapper);
        self.nodes.get_mut(&id).unwrap().parent = Some(wrapper);
        self.compact_tree();
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
        let old = self.compute_geometry();
        let Some(parent_index) = self.child_index(grandparent, parent) else {
            return false;
        };
        self.remove_child(parent, id);
        let index = parent_index + usize::from(after);
        self.insert_existing_child(grandparent, id, index, parent);
        self.collapse_from(parent);
        self.compact_tree();
        self.animate_geometry_changes(old, None);
        self.request_window_sizes();
        true
    }

    fn insert_child(&mut self, parent: NodeId, child: NodeId, after: Option<NodeId>) {
        let index = match &self.nodes[&parent].value {
            TreeNode::Split { children, .. } => after
                .and_then(|id| children.iter().position(|child| *child == id))
                .map_or(children.len(), |index| index + 1),
            TreeNode::Leaf { .. } => return,
        };
        self.insert_child_at(parent, child, index);
    }

    fn insert_child_at(&mut self, parent: NodeId, child: NodeId, index: usize) {
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let percent = 1. / (children.len() + 1) as f64;
        for existing in percents.iter_mut() {
            *existing *= 1. - percent;
        }
        children.insert(index.min(children.len()), child);
        percents.insert(index.min(percents.len()), percent);
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

    fn detach_subtree_only(&mut self, id: NodeId) -> Option<NodeId> {
        let parent = self.nodes.get(&id).and_then(|node| node.parent)?;
        self.remove_child(parent, id);
        self.nodes.get_mut(&id)?.parent = None;
        Some(parent)
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
        let old = self.alloc(Node {
            parent: Some(self.root),
            value: old_value,
        });
        if let TreeNode::Split { children, .. } = &self.nodes.get(&old).unwrap().value {
            for child in children.clone() {
                self.nodes.get_mut(&child).unwrap().parent = Some(old);
            }
        }
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

    fn reap_empty_from(&mut self, mut id: NodeId) {
        loop {
            let (parent, empty) = match self.nodes.get(&id) {
                Some(Node {
                    parent,
                    value: TreeNode::Split { children, .. },
                }) => (*parent, children.is_empty()),
                _ => return,
            };
            if id == self.root || !empty {
                return;
            }
            let Some(parent) = parent else { return };
            if self.focus == Some(id) {
                self.set_focus_id(Some(parent));
            }
            self.remove_node(id);
            self.remove_child(parent, id);
            id = parent;
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
                if self.focus == Some(id) {
                    self.set_focus_id(Some(parent));
                }
                self.remove_node(id);
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
            if let Some(fullscreen) = self.pending_modes.get(&id).and_then(|mode| mode.fullscreen) {
                self.pending_modes
                    .entry(child)
                    .or_insert(PendingMode {
                        fullscreen: None,
                        maximized: false,
                    })
                    .fullscreen = Some(fullscreen);
            }
            if self.focus == Some(id) {
                self.set_focus_id(Some(child));
            }
            self.remove_node(id);
            id = parent;
        }
    }

    fn compact_tree(&mut self) {
        loop {
            let squashable = self
                .iter_depth_first()
                .find_map(|(id, _)| self.squashable_child(id).map(|_| id));
            let Some(id) = squashable else { return };
            self.squash(id);
        }
    }

    fn squashable_child(&self, id: NodeId) -> Option<NodeId> {
        let TreeNode::Split { children, .. } = &self.nodes.get(&id)?.value else {
            return None;
        };
        let [child] = children.as_slice() else {
            return None;
        };
        self.is_squashable(id, *child).then_some(*child)
    }

    fn is_squashable(&self, id: NodeId, child: NodeId) -> bool {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return false;
        };
        let Some(TreeNode::Split {
            layout: parent_layout,
            ..
        }) = self.nodes.get(&parent).map(|node| &node.value)
        else {
            return false;
        };
        let Some(TreeNode::Split {
            layout, children, ..
        }) = self.nodes.get(&id).map(|node| &node.value)
        else {
            return false;
        };
        let Some(TreeNode::Split {
            layout: child_layout,
            ..
        }) = self.nodes.get(&child).map(|node| &node.value)
        else {
            return false;
        };
        children.len() == 1
            && matches!(layout, Layout::SplitH | Layout::SplitV)
            && matches!(child_layout, Layout::SplitH | Layout::SplitV)
            && !Self::layouts_parallel(*layout, *child_layout)
            && Self::layouts_parallel(*parent_layout, *child_layout)
    }

    fn layouts_parallel(first: Layout, second: Layout) -> bool {
        matches!(
            (first, second),
            (
                Layout::SplitH | Layout::Tabbed,
                Layout::SplitH | Layout::Tabbed
            ) | (
                Layout::SplitV | Layout::Stacked,
                Layout::SplitV | Layout::Stacked
            )
        )
    }

    fn squash(&mut self, id: NodeId) {
        let Some(parent) = self.nodes.get(&id).and_then(|node| node.parent) else {
            return;
        };
        let Some(child) = self.squashable_child(id) else {
            return;
        };
        let (grandchildren, child_percents) = match &self.nodes[&child].value {
            TreeNode::Split {
                children, percents, ..
            } => (children.clone(), percents.clone()),
            TreeNode::Leaf { .. } => return,
        };
        let Some(Node {
            value: TreeNode::Split {
                children, percents, ..
            },
            ..
        }) = self.nodes.get_mut(&parent)
        else {
            return;
        };
        let Some(index) = children.iter().position(|candidate| *candidate == id) else {
            return;
        };
        children.remove(index);
        let percent = percents.remove(index);
        for (offset, (grandchild, child_percent)) in
            grandchildren.iter().zip(child_percents).enumerate()
        {
            children.insert(index + offset, *grandchild);
            percents.insert(index + offset, percent * child_percent);
        }
        for grandchild in &grandchildren {
            self.nodes.get_mut(grandchild).unwrap().parent = Some(parent);
        }
        let replacement = grandchildren.first().copied().unwrap_or(parent);
        if let Some(fullscreen) = [id, child]
            .into_iter()
            .find_map(|id| self.pending_modes.get(&id).and_then(|mode| mode.fullscreen))
        {
            self.pending_modes
                .entry(parent)
                .or_insert(PendingMode {
                    fullscreen: None,
                    maximized: false,
                })
                .fullscreen = Some(fullscreen);
        }
        if self.focus == Some(id) || self.focus == Some(child) {
            self.set_focus_id(Some(replacement));
        }
        self.remove_node(id);
        self.remove_node(child);
    }

    fn split_len(&self, id: NodeId) -> Option<usize> {
        match &self.nodes.get(&id)?.value {
            TreeNode::Split { children, .. } => Some(children.len()),
            TreeNode::Leaf { .. } => None,
        }
    }

    fn animate_geometry_changes(&mut self, old: geometry::Geometry<W::Id>, skip: Option<NodeId>) {
        let new = self.compute_geometry();
        for (id, old_rect) in old.leaf_contents {
            if skip == Some(id) {
                continue;
            }
            let Some(new_rect) = new.leaf_contents.get(&id) else {
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

    fn titlebar_state(&self, id: NodeId, workspace_focused: bool) -> TitlebarState {
        let urgent = self.tile(id).is_some_and(|tile| tile.window().is_urgent());
        if urgent {
            return TitlebarState::Urgent;
        }
        let Some(focus) = self.focus else {
            return TitlebarState::Unfocused;
        };
        if id == focus {
            return if workspace_focused {
                TitlebarState::Focused
            } else {
                TitlebarState::FocusedInactive
            };
        }
        let is_tab_title_with_focused_descendant = self.nodes.values().any(|node| {
            let TreeNode::Split {
                layout: Layout::Tabbed | Layout::Stacked,
                children,
                ..
            } = &node.value
            else {
                return false;
            };
            children.iter().any(|child| {
                self.first_leaf_in(*child) == Some(id) && self.contains_node(*child, focus)
            })
        });
        if is_tab_title_with_focused_descendant {
            TitlebarState::FocusedTabTitle
        } else {
            TitlebarState::Unfocused
        }
    }

    fn visible_leaves(&self) -> HashSet<NodeId> {
        if let Some(fullscreen) = self.fullscreen_node() {
            let mut visible = HashSet::new();
            self.collect_visible(fullscreen, &mut visible);
            return visible;
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

    pub(super) fn node_for_window(&self, window: &W::Id) -> Option<NodeId> {
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

    fn node_geometry(&self, id: NodeId) -> Option<Rectangle<f64, Logical>> {
        if id == self.root {
            return Some(Rectangle::from_size(self.view_size));
        }
        let geometries = self.compute_geometry();
        let mut leaves = Vec::new();
        self.collect_leaf_ids(id, &mut leaves);
        let mut rect = *geometries.leaf_contents.get(leaves.first()?)?;
        for leaf in &leaves[1..] {
            let next = geometries.leaf_contents.get(leaf)?;
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

    fn focused_leaf_in(&self, id: NodeId) -> Option<NodeId> {
        self.focus_history
            .iter()
            .copied()
            .find(|candidate| self.tile(*candidate).is_some() && self.is_descendant(*candidate, id))
            .or_else(|| self.first_leaf_in(id))
    }

    fn request_window_sizes(&mut self) {
        self.request_window_sizes_with(None, false);
    }

    fn request_window_sizes_with(&mut self, transaction: Option<Transaction>, animate: bool) {
        let geometries = self.compute_geometry();
        for (id, node) in &mut self.nodes {
            if let TreeNode::Leaf { tile } = &mut node.value {
                let transaction = transaction.clone();
                if let Some(rect) = geometries.leaf_contents.get(id) {
                    let mode = self.pending_modes.get(id).copied().unwrap_or(PendingMode {
                        fullscreen: None,
                        maximized: false,
                    });
                    if mode.fullscreen.is_some() {
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

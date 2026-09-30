mod depth;
mod focus;
mod fullscreen;
mod geometry;
mod introspection;
mod invariants;
mod movement;
mod mutation;
mod node;
mod rendering;
mod resize;
mod sizing;
mod state;
mod transfer;
mod tree_layout;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

#[cfg(test)]
pub(crate) use depth::MAX_TREE_DEPTH;
pub(crate) use depth::TOO_DEEP;
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
use super::tab_indicator::{TabIndicator, TabIndicatorRenderElement, TabInfo};
use super::tile::{DecoratedCorners, Tile, TileRenderElement};
use super::titlebar::TitlebarState;
use super::{ConfigureIntent, HitType, InteractiveResizeData, LayoutElement, Options, RenderLayer};
use crate::animation::Clock;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::RenderCtx;
use crate::swayward_render_elements;
use crate::utils::id::IdCounter;
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
    root_focused: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DetachedSlot {
    parent: NodeId,
    index: usize,
    percent: f64,
    focus_rank: usize,
    focused: bool,
}

/// The fields of [`DetachedNode::Split`], passed whole when a split is reattached.
struct DetachedSplit<W: LayoutElement> {
    old_id: NodeId,
    layout: Layout,
    children: Vec<DetachedNode<W>>,
    percents: Vec<f64>,
    previous_layout: Option<Layout>,
    title_format: Option<String>,
    pending_mode: Option<PendingMode>,
    sticky: bool,
}

#[derive(Debug)]
enum DetachedNode<W: LayoutElement> {
    Split {
        old_id: NodeId,
        layout: Layout,
        children: Vec<DetachedNode<W>>,
        percents: Vec<f64>,
        previous_layout: Option<Layout>,
        title_format: Option<String>,
        pending_mode: Option<PendingMode>,
        sticky: bool,
    },
    Leaf {
        old_id: NodeId,
        tile: Box<Tile<W>>,
        pending_mode: Option<PendingMode>,
        mapped_under_fullscreen: bool,
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

    pub fn swap_fullscreen_position(&mut self, other: &mut Self) {
        fn root_fullscreen<W: LayoutElement>(node: &DetachedNode<W>) -> Option<FullscreenMode> {
            match node {
                DetachedNode::Split { pending_mode, .. }
                | DetachedNode::Leaf { pending_mode, .. } => {
                    pending_mode.and_then(|mode| mode.fullscreen)
                }
            }
        }
        fn set_root_fullscreen<W: LayoutElement>(
            node: &mut DetachedNode<W>,
            fullscreen: Option<FullscreenMode>,
        ) {
            match node {
                DetachedNode::Split { pending_mode, .. }
                | DetachedNode::Leaf { pending_mode, .. } => {
                    pending_mode
                        .get_or_insert(PendingMode {
                            fullscreen: None,
                            maximized: false,
                        })
                        .fullscreen = fullscreen;
                }
            }
        }
        fn clear_fullscreen<W: LayoutElement>(node: &mut DetachedNode<W>) {
            match node {
                DetachedNode::Split {
                    children,
                    pending_mode,
                    ..
                } => {
                    if let Some(mode) = pending_mode {
                        mode.fullscreen = None;
                    }
                    for child in children {
                        clear_fullscreen(child);
                    }
                }
                DetachedNode::Leaf { pending_mode, .. } => {
                    if let Some(mode) = pending_mode {
                        mode.fullscreen = None;
                    }
                }
            }
        }

        let first = root_fullscreen(&self.node);
        let second = root_fullscreen(&other.node);
        clear_fullscreen(&mut self.node);
        clear_fullscreen(&mut other.node);
        set_root_fullscreen(&mut self.node, second);
        set_root_fullscreen(&mut other.node, first);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IpcNode<I> {
    Split {
        id: NodeId,
        layout: Layout,
        title: Option<String>,
        percent: Option<f64>,
        rect: Rectangle<f64, Logical>,
        focus: Vec<NodeId>,
        focused: bool,
        fullscreen_mode: i32,
        sticky: bool,
        children: Vec<IpcNode<I>>,
    },
    Leaf {
        id: NodeId,
        window: I,
        percent: Option<f64>,
        focused: bool,
        fullscreen_mode: i32,
        rect: Rectangle<f64, Logical>,
        deco_rect: Option<Rectangle<f64, Logical>>,
        border: (swayward_ipc::command::BorderStyle, u16),
        border_edges: ResizeEdge,
        sticky: bool,
        mapped_under_fullscreen: bool,
    },
}

impl<I> IpcNode<I> {
    pub fn window_for_node(&self, wanted: NodeId) -> Option<&I> {
        match self {
            IpcNode::Leaf { id, window, .. } => (*id == wanted).then_some(window),
            IpcNode::Split { children, .. } => children
                .iter()
                .find_map(|child| child.window_for_node(wanted)),
        }
    }

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
    /// One sibling boundary per resized axis, like sway's separate `h_con`
    /// and `v_con` (`sway/sway/input/seatop_resize_tiling.c:12-27`).
    axes: Vec<ResizeAxis>,
    data: InteractiveResizeData,
}

#[derive(Debug, Clone, Copy)]
struct ResizeAxis {
    horizontal: bool,
    first: NodeId,
    second: NodeId,
    initial_first: f64,
    initial_second: f64,
    axis_size: f64,
    sign: f64,
}

swayward_render_elements! {
    TilingTreeRenderElement<R> => {
        Tile = TileRenderElement<R>,
        ClosingWindow = ClosingWindowRenderElement,
        TabIndicator = TabIndicatorRenderElement,
        Titlebar = crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement,
        UncoveredTopBorder = crate::render_helpers::solid_color::SolidColorRenderElement,
    }
}

/// A group of decoration render elements collected by the tiling tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DecorationLayer {
    UncoveredTopBorders,
    Titlebars,
    Tiles,
}

static NODE_ID_COUNTER: IdCounter = IdCounter::new();

#[derive(Debug)]
pub struct TilingTree<W: LayoutElement> {
    nodes: HashMap<NodeId, Node<W>>,
    root: NodeId,
    focus: Option<NodeId>,
    ipc_stale_nodes: HashSet<NodeId>,
    has_had_tile: bool,
    empty_representation_layout: Option<Layout>,
    focus_history: Vec<NodeId>,
    previous_split_layouts: HashMap<NodeId, Layout>,
    title_formats: HashMap<NodeId, String>,
    sticky_splits: HashSet<NodeId>,
    pending_modes: HashMap<NodeId, PendingMode>,
    mapped_under_fullscreen: HashSet<NodeId>,
    fullscreen_layout_wrappers: HashSet<NodeId>,
    pre_layout_ipc_rects: HashMap<NodeId, Rectangle<f64, Logical>>,
    interactive_resize: Option<InteractiveResize<W::Id>>,
    tab_indicators: HashMap<NodeId, TabIndicator>,
    titlebars: super::titlebar::TitlebarRenderer,
    tab_active: HashMap<NodeId, NodeId>,
    closing_windows: Vec<ClosingWindow>,
    view_size: Size<f64, Logical>,
    parent_area: Rectangle<f64, Logical>,
    gaps_to_edge: bool,
    resident_root: bool,
    scale: f64,
    titlebar_height: f64,
    clock: Clock,
    options: Rc<Options>,
    gaps: f64,
    preserved_auto_layout: Option<Layout>,
}

#[cfg(test)]
mod tests;

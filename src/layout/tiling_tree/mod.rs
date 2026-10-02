// A live-session path must not panic (AGENTS.md). Outside tests, look nodes
// up with `get` and handle a miss, or state the invariant with `expect`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::indexing_slicing))]
mod arena;
mod configure;
mod depth;
mod focus;
mod fullscreen;
mod geometry;
mod introspection;
mod invariants;
mod movement;
mod mutation;
mod node;
mod normalize;
mod rendering;
mod resize;
mod state;
mod transfer;
mod tree_layout;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[cfg(test)]
pub(crate) use depth::MAX_TREE_DEPTH;
pub(crate) use depth::TOO_DEEP;
use geometry::apply_struts;
use node::Node;
pub use node::{NodeId, SplitMeta, TreeNode};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size};
use swayward_config::utils::MergeWith as _;
use swayward_config::PresetSize;
use swayward_ipc::command::{LayoutToggle, LayoutToggleEntry};
use swayward_ipc::{SizeChange, WindowLayout};

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
    /// Where sway puts a container moved onto this workspace: inside the
    /// focused container, or beside it when it is a view
    /// (`container_move_to_container`, sway/commands/move.c:241-262).
    MoveDestination,
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
    /// The workspace's children, wrapped in a container sway creates for the
    /// move (`workspace_wrap_children`, sway/commands/move.c:476-484). It was
    /// never focused, so it arrives at the tail of the focus stack.
    wrapped_workspace: bool,
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
    meta: SplitMeta,
    pending_mode: Option<PendingMode>,
}

#[derive(Debug)]
enum DetachedNode<W: LayoutElement> {
    Split {
        old_id: NodeId,
        layout: Layout,
        children: Vec<DetachedNode<W>>,
        percents: Vec<f64>,
        meta: SplitMeta,
        pending_mode: Option<PendingMode>,
    },
    Leaf {
        old_id: NodeId,
        tile: Box<Tile<W>>,
        pending_mode: Option<PendingMode>,
        mapped_under_fullscreen: bool,
    },
}

impl<W: LayoutElement> DetachedNode<W> {
    fn into_split(self) -> Result<DetachedSplit<W>, Self> {
        match self {
            Self::Split {
                old_id,
                layout,
                children,
                percents,
                meta,
                pending_mode,
            } => Ok(DetachedSplit {
                old_id,
                layout,
                children,
                percents,
                meta,
                pending_mode,
            }),
            leaf @ Self::Leaf { .. } => Err(leaf),
        }
    }

    fn pending_mode(&self) -> Option<PendingMode> {
        match self {
            Self::Split { pending_mode, .. } | Self::Leaf { pending_mode, .. } => *pending_mode,
        }
    }

    fn pending_mode_mut(&mut self) -> &mut Option<PendingMode> {
        match self {
            Self::Split { pending_mode, .. } | Self::Leaf { pending_mode, .. } => pending_mode,
        }
    }

    fn fullscreen(&self) -> Option<FullscreenMode> {
        self.pending_mode().and_then(|mode| mode.fullscreen)
    }

    fn has_fullscreen(&self) -> bool {
        self.fullscreen().is_some()
            || matches!(self, Self::Split { children, .. } if children.iter().any(Self::has_fullscreen))
    }

    /// Calls `f` on this node and then on every descendant, depth first.
    fn walk_mut(&mut self, f: &mut impl FnMut(&mut Self)) {
        f(self);
        if let Self::Split { children, .. } = self {
            for child in children {
                child.walk_mut(f);
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

    /// Swaps which subtree root holds fullscreen, clearing fullscreen everywhere below the roots.
    pub fn swap_fullscreen_position(&mut self, other: &mut Self) {
        let first = self.node.fullscreen();
        let second = other.node.fullscreen();
        for node in [&mut self.node, &mut other.node] {
            node.walk_mut(&mut |node| {
                if let Some(mode) = node.pending_mode_mut() {
                    mode.fullscreen = None;
                }
            });
        }
        self.node
            .pending_mode_mut()
            .get_or_insert_default()
            .fullscreen = second;
        other
            .node
            .pending_mode_mut()
            .get_or_insert_default()
            .fullscreen = first;
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
        /// Pre-move IPC box of a leaf moved into a fullscreen workspace.
        moved_under_fullscreen: Option<Rectangle<f64, Logical>>,
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

    pub fn any_window(&self, f: &impl Fn(&I) -> bool) -> bool {
        match self {
            IpcNode::Leaf { window, .. } => f(window),
            IpcNode::Split { children, .. } => children.iter().any(|child| child.any_window(f)),
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

impl Direction {
    /// The split layout whose children are ordered along this direction.
    pub(super) fn axis(self) -> Layout {
        match self {
            Direction::Left | Direction::Right => Layout::SplitH,
            Direction::Up | Direction::Down => Layout::SplitV,
        }
    }

    /// Whether this direction points toward lower child indices.
    pub(super) fn is_backwards(self) -> bool {
        matches!(self, Direction::Left | Direction::Up)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullscreenMode {
    Workspace = 1,
    Global = 2,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PendingMode {
    fullscreen: Option<FullscreenMode>,
    maximized: bool,
}

#[derive(Debug)]
struct InteractiveResize<I> {
    window: I,
    target: NodeId,
    /// One sibling boundary per resized axis, like sway's separate `h_con`
    /// and `v_con` (`sway/input/seatop_resize_tiling.c:12-27`).
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

/// A collection keyed by node id that must forget a node when it leaves the arena.
trait SideTable {
    fn ids(&self) -> Box<dyn Iterator<Item = NodeId> + '_>;
    fn forget(&mut self, id: NodeId);
}

impl<V> SideTable for HashMap<NodeId, V> {
    fn ids(&self) -> Box<dyn Iterator<Item = NodeId> + '_> {
        Box::new(self.keys().copied())
    }
    fn forget(&mut self, id: NodeId) {
        self.remove(&id);
    }
}

impl SideTable for HashSet<NodeId> {
    fn ids(&self) -> Box<dyn Iterator<Item = NodeId> + '_> {
        Box::new(self.iter().copied())
    }
    fn forget(&mut self, id: NodeId) {
        self.remove(&id);
    }
}

impl SideTable for Vec<NodeId> {
    fn ids(&self) -> Box<dyn Iterator<Item = NodeId> + '_> {
        Box::new(self.iter().copied())
    }
    fn forget(&mut self, id: NodeId) {
        self.retain(|candidate| *candidate != id);
    }
}

/// Every NodeId-keyed side table of a TilingTree, named for invariant messages. remove_node
/// forgets a node in each, and check_side_state checks each for stale ids, so a table added
/// here gets both. Call as `side_tables!(tree, &)` or `side_tables!(tree, &mut)`.
macro_rules! side_tables {
    ($tree:expr, $($ref:tt)+) => {
        [
            ("focus_history", $($ref)+ $tree.focus_history as $($ref)+ dyn SideTable),
            ("ipc_stale_nodes", $($ref)+ $tree.ipc_stale_nodes),
            ("last_entered_by", $($ref)+ $tree.last_entered_by),
            ("pending_modes", $($ref)+ $tree.pending_modes),
            ("mapped_under_fullscreen", $($ref)+ $tree.mapped_under_fullscreen),
            ("moved_under_fullscreen", $($ref)+ $tree.moved_under_fullscreen),
            ("fullscreen_layout_wrappers", $($ref)+ $tree.fullscreen_layout_wrappers),
            ("pre_layout_ipc_rects", $($ref)+ $tree.pre_layout_ipc_rects),
            ("tab_indicators", $($ref)+ $tree.tab_indicators),
            ("tab_active", $($ref)+ $tree.tab_active),
        ]
    };
}
use side_tables;

#[derive(Debug)]
pub struct TilingTree<W: LayoutElement> {
    nodes: HashMap<NodeId, Node<W>>,
    root: NodeId,
    focus: Option<NodeId>,
    ipc_stale_nodes: HashSet<NodeId>,
    /// The leaf whose focus last raised each container in the focus stack.
    /// The IPC focus list ranks a container by when focus last entered it,
    /// which outlives that leaf moving away (`seat_set_raw_focus`,
    /// sway/input/seat.c).
    last_entered_by: HashMap<NodeId, NodeId>,
    has_had_tile: bool,
    empty_representation_layout: Option<Layout>,
    focus_history: Vec<NodeId>,
    pending_modes: HashMap<NodeId, PendingMode>,
    mapped_under_fullscreen: HashSet<NodeId>,
    /// Leaves moved into this tree while it was fullscreen. Like mapped ones
    /// they get no share of their parent's split, but they keep their border
    /// and titlebar (`container_move_to_workspace`, sway/commands/move.c:220-229).
    moved_under_fullscreen: HashMap<NodeId, Rectangle<f64, Logical>>,
    /// The IPC focus list follows `focus_history` rather than window focus
    /// timestamps, because the seat stack was reordered without focusing a
    /// window (`workspace_focus_fullscreen`, sway/commands/move.c:96-110).
    ipc_focus_follows_history: bool,
    /// The fullscreen node reports its tiled slot over IPC. Sway's
    /// `arrange_container(parent)` gives it the slot's pending box until the
    /// next workspace arrange restores the output box.
    fullscreen_tile_slot: bool,
    /// The fullscreen node arrived in this tree already fullscreen. Sway gives
    /// a moved container a zero width fraction and arranges only the
    /// fullscreen node (`container_move_to_workspace`,
    /// sway/commands/move.c:220-229; `arrange_workspace`,
    /// sway/tree/arrange.c:310-316), so its siblings keep their old shares
    /// until fullscreen ends.
    fullscreen_arrived: bool,
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

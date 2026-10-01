use std::collections::{HashMap, HashSet};

use smithay::utils::{Logical, Point, Rectangle, Size};
use swayward_config::{HideEdgeBorders, SmartBorders, Struts};

use super::{Layout, Node, NodeId, TreeNode};
use crate::layout::tile::DecoratedCorners;
use crate::layout::titlebar::{Titlebar, TitlebarState};
use crate::layout::LayoutElement;
use crate::utils::ResizeEdge;

pub(crate) struct Geometry<I> {
    pub leaf_boxes: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub leaf_contents: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub leaf_ipc_rects: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub ipc_nodes: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub tiled_ipc_nodes: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub titlebars: HashMap<NodeId, Titlebar<I>>,
    pub titlebar_leaves: HashMap<NodeId, NodeId>,
    pub titlebar_attached: HashSet<NodeId>,
    pub titlebar_owned_by_parent: HashSet<NodeId>,
    pub border_edges: HashMap<NodeId, ResizeEdge>,
    pub border_visible: HashSet<NodeId>,
    pub border_corners: HashMap<NodeId, DecoratedCorners>,
    pub titlebar_corners: HashMap<NodeId, DecoratedCorners>,
    pub uncovered_top_borders: HashMap<NodeId, Vec<Rectangle<f64, Logical>>>,
}

struct AssignContext<'a, W: LayoutElement> {
    nodes: &'a HashMap<NodeId, Node<W>>,
    title_formats: &'a HashMap<NodeId, String>,
    gaps: f64,
    titlebar_height: f64,
    fullscreen: &'a HashSet<NodeId>,
    mapped_under_fullscreen: &'a HashSet<NodeId>,
    workspace_area: Rectangle<f64, Logical>,
    gaps_to_edge: bool,
    hide_edge_borders: HideEdgeBorders,
    smart_borders: SmartBorders,
    only_visible_view: bool,
    draw_uncovered_top_border: bool,
}

struct Assignment {
    id: NodeId,
    rect: Rectangle<f64, Logical>,
    covering_titlebar: Option<Rectangle<f64, Logical>>,
    decorated_by_parent: bool,
    decorated_corners: DecoratedCorners,
    suppress_gaps: bool,
    ipc_origin: Point<f64, Logical>,
}

/// Everything one tree geometry pass reads, borrowed from the tree and its layout options.
pub(crate) struct GeometryInput<'a, W: LayoutElement> {
    pub nodes: &'a HashMap<NodeId, Node<W>>,
    pub title_formats: &'a HashMap<NodeId, String>,
    pub root: NodeId,
    pub view_size: Size<f64, Logical>,
    pub parent_area: Rectangle<f64, Logical>,
    pub scale: f64,
    pub struts: Struts,
    pub gaps: f64,
    pub outer_gaps_configured: bool,
    pub gaps_to_edge: bool,
    pub titlebar_height: f64,
    pub fullscreen: &'a HashSet<NodeId>,
    pub mapped_under_fullscreen: &'a HashSet<NodeId>,
    pub hide_edge_borders: HideEdgeBorders,
    pub smart_borders: SmartBorders,
    pub visible_leaves: &'a HashSet<NodeId>,
    pub draw_uncovered_top_border: bool,
}

pub(crate) fn compute<W: LayoutElement>(input: GeometryInput<'_, W>) -> Geometry<W::Id> {
    let gaps = input.gaps.max(0.);
    let workspace_area = workspace_area(&input, gaps);
    let mut context = AssignContext {
        nodes: input.nodes,
        title_formats: input.title_formats,
        gaps,
        titlebar_height: input.titlebar_height,
        fullscreen: &HashSet::new(),
        mapped_under_fullscreen: input.mapped_under_fullscreen,
        workspace_area,
        gaps_to_edge: input.gaps_to_edge,
        hide_edge_borders: input.hide_edge_borders,
        smart_borders: input.smart_borders,
        only_visible_view: input.visible_leaves.len() == 1,
        draw_uncovered_top_border: input.draw_uncovered_top_border,
    };
    let mut result = Geometry::default();
    assign(
        &context,
        Assignment {
            id: input.root,
            rect: workspace_area,
            covering_titlebar: None,
            decorated_by_parent: false,
            decorated_corners: DecoratedCorners::ALL,
            suppress_gaps: false,
            ipc_origin: Point::default(),
        },
        &mut result,
    );
    result.tiled_ipc_nodes = result.ipc_nodes.clone();
    if let Some(fullscreen_root) = input.fullscreen.iter().copied().next() {
        context.fullscreen = input.fullscreen;
        apply_fullscreen_pass(&context, fullscreen_root, input.view_size, &mut result);
    }
    result.drop_titlebars(|id| input.mapped_under_fullscreen.contains(&id));
    result.border_visible.extend(
        result
            .leaf_boxes
            .keys()
            .filter(|id| input.visible_leaves.contains(id)),
    );
    result
        .uncovered_top_borders
        .retain(|id, _| input.visible_leaves.contains(id));
    hide_invisible_strips(input.nodes, input.visible_leaves, &mut result);
    result
}

/// The tiled area: the parent area less struts, inset by the inner gap on every side unless
/// outer gaps were configured separately.
fn workspace_area<W: LayoutElement>(
    input: &GeometryInput<'_, W>,
    gaps: f64,
) -> Rectangle<f64, Logical> {
    let mut area = apply_struts(input.parent_area, input.scale, input.struts);
    if !input.outer_gaps_configured {
        area.loc.x += gaps;
        area.loc.y += gaps;
        area.size.w = (area.size.w - gaps * 2.).max(0.);
        area.size.h = (area.size.h - gaps * 2.).max(0.);
    }
    area
}

/// Lays the fullscreen subtree out over the whole output on top of the tiled pass. Titlebars
/// inside it are not drawn.
fn apply_fullscreen_pass<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    fullscreen_root: NodeId,
    view_size: Size<f64, Logical>,
    result: &mut Geometry<W::Id>,
) {
    assign(
        context,
        Assignment {
            id: fullscreen_root,
            rect: Rectangle::from_size(view_size),
            covering_titlebar: None,
            decorated_by_parent: false,
            decorated_corners: DecoratedCorners::NONE,
            suppress_gaps: false,
            ipc_origin: Point::default(),
        },
        result,
    );
    let fullscreen = context.fullscreen;
    result.titlebars.retain(|id, _| !fullscreen.contains(id));
    result
        .titlebar_attached
        .retain(|id| !fullscreen.contains(id));
    result
        .titlebar_owned_by_parent
        .retain(|id| !fullscreen.contains(id));
}

/// A tabbed or stacked container shows only its active child; sway sends the rest to
/// disable_container, which hides the whole subtree, strips included
/// (sway/desktop/transaction.c:313-323). Titlebars are emitted for every strip in the tree, so
/// hide those whose branch is not shown. A branch is shown exactly when it holds a visible
/// leaf, because visible_leaves already walks only the active child of each tab level.
///
/// A strip entry belongs to the container that draws the strip, so it is shown when that
/// container is: an inactive tab keeps its title in a visible strip even though its own
/// subtree is hidden. A leaf's own titlebar belongs to the leaf.
fn hide_invisible_strips<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    visible_leaves: &HashSet<NodeId>,
    result: &mut Geometry<W::Id>,
) {
    for (id, titlebar) in &mut result.titlebars {
        let owner = if is_strip_entry(nodes, *id) {
            nodes.get(id).and_then(|node| node.parent).unwrap_or(*id)
        } else {
            *id
        };
        if !subtree_has_visible_leaf(nodes, owner, visible_leaves) {
            titlebar.visible = false;
        }
    }
}

// Written out because `#[derive(Default)]` would require `I: Default`.
impl<I> Default for Geometry<I> {
    fn default() -> Self {
        Self {
            leaf_boxes: HashMap::new(),
            leaf_contents: HashMap::new(),
            leaf_ipc_rects: HashMap::new(),
            ipc_nodes: HashMap::new(),
            tiled_ipc_nodes: HashMap::new(),
            titlebars: HashMap::new(),
            titlebar_leaves: HashMap::new(),
            titlebar_attached: HashSet::new(),
            titlebar_owned_by_parent: HashSet::new(),
            border_edges: HashMap::new(),
            border_visible: HashSet::new(),
            border_corners: HashMap::new(),
            titlebar_corners: HashMap::new(),
            uncovered_top_borders: HashMap::new(),
        }
    }
}

impl<I> Geometry<I> {
    /// Removes the titlebars of `drop` from every titlebar map together, so the maps stay
    /// consistent with each other.
    fn drop_titlebars(&mut self, drop: impl Fn(NodeId) -> bool) {
        self.titlebars.retain(|id, _| !drop(*id));
        self.titlebar_leaves.retain(|id, _| !drop(*id));
        self.titlebar_attached.retain(|id| !drop(*id));
        self.titlebar_owned_by_parent.retain(|id| !drop(*id));
    }
}

/// Whether `id` is a child of a tabbed or stacked container, i.e. its titlebar
/// is drawn by the parent's strip rather than by the node itself.
fn is_strip_entry<W: LayoutElement>(nodes: &HashMap<NodeId, Node<W>>, id: NodeId) -> bool {
    nodes
        .get(&id)
        .and_then(|node| node.parent)
        .and_then(|parent| nodes.get(&parent))
        .is_some_and(|parent| {
            matches!(
                parent.value,
                TreeNode::Split {
                    layout: Layout::Tabbed | Layout::Stacked,
                    ..
                }
            )
        })
}

fn subtree_has_visible_leaf<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    id: NodeId,
    visible_leaves: &HashSet<NodeId>,
) -> bool {
    match nodes.get(&id).map(|node| &node.value) {
        Some(TreeNode::Leaf { .. }) => visible_leaves.contains(&id),
        Some(TreeNode::Split { children, .. }) => children
            .iter()
            .any(|child| subtree_has_visible_leaf(nodes, *child, visible_leaves)),
        None => false,
    }
}

pub(super) fn apply_struts(
    parent_area: Rectangle<f64, Logical>,
    scale: f64,
    struts: Struts,
) -> Rectangle<f64, Logical> {
    let mut working_area = parent_area;
    working_area.size.w = (working_area.size.w - struts.left.0 - struts.right.0).max(0.);
    working_area.loc.x += struts.left.0;
    working_area.size.h = (working_area.size.h - struts.top.0 - struts.bottom.0).max(0.);
    working_area.loc.y += struts.top.0;

    let loc = working_area
        .loc
        .to_physical_precise_ceil(scale)
        .to_logical(scale);
    let loc_delta = loc - working_area.loc;
    let mut size_diff = Size::from((loc_delta.x.max(0.), loc_delta.y.max(0.)));
    size_diff.w = working_area.size.w.min(size_diff.w);
    size_diff.h = working_area.size.h.min(size_diff.h);
    working_area.size -= size_diff;
    working_area.size.w = working_area.size.w.max(0.);
    working_area.size.h = working_area.size.h.max(0.);
    working_area.loc = loc;
    working_area
}

fn assign_leaf<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    tile: &crate::layout::tile::Tile<W>,
    assignment: Assignment,
    result: &mut Geometry<W::Id>,
) {
    let Assignment {
        id,
        rect,
        covering_titlebar,
        decorated_by_parent,
        decorated_corners,
        ipc_origin,
        ..
    } = assignment;
    let titlebar_height = context.titlebar_height;
    let fullscreen = context.fullscreen;
    let edges = border_edges(context, rect);
    result.border_edges.insert(id, edges);
    // Keep one outer box for rendering, hit testing, movement, sizing, and IPC. Sway's
    // arrange_container() likewise derives the content and each border from the container
    // dimensions (sway/desktop/transaction.c:392-472).
    result.leaf_boxes.insert(id, rect);
    let decorated_by_parent = decorated_by_parent && fullscreen.is_empty();
    if decorated_by_parent {
        result.titlebar_attached.insert(id);
        result.titlebar_owned_by_parent.insert(id);
    }

    let has_titlebar =
        !decorated_by_parent && !fullscreen.contains(&id) && tile.has_sway_titlebar();
    if has_titlebar {
        emit_leaf_titlebar(
            context,
            tile,
            id,
            rect,
            decorated_corners,
            ipc_origin,
            result,
        );
    }
    // A titlebar takes over the box's top corners; IPC reports the view below it.
    let mut border_corners = decorated_corners;
    let mut ipc_rect = rect;
    if has_titlebar {
        border_corners.top_left = false;
        border_corners.top_right = false;
        ipc_rect.loc.y += titlebar_height;
        ipc_rect.size.h = (ipc_rect.size.h - titlebar_height).max(0.);
    }
    result.border_corners.insert(id, border_corners);
    result.leaf_ipc_rects.insert(id, ipc_rect);

    let width = tile.configured_border_width();
    if context.draw_uncovered_top_border
        && decorated_by_parent
        && edges.contains(ResizeEdge::TOP)
        && width > 0.
    {
        record_uncovered_top_border(id, rect, width, covering_titlebar, result);
    }
    let top = if has_titlebar {
        titlebar_height
    } else if decorated_by_parent {
        0.
    } else {
        width * f64::from(edges.contains(ResizeEdge::TOP))
    };
    result
        .leaf_contents
        .insert(id, content_rect(rect, edges, width, top));
}

/// The border edges a leaf draws. `hide_edge_borders` drops the edges that touch the
/// workspace's outer edge on the chosen axes, and `smart_borders` drops every edge when only
/// one view is visible (with `no_gaps`, only when gaps do not reach the edge), as sway's
/// view_autoconfigure does (sway/tree/view.c:377-401).
fn border_edges<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    rect: Rectangle<f64, Logical>,
) -> ResizeEdge {
    let workspace_area = context.workspace_area;
    let mut edges = ResizeEdge::all();
    if matches!(
        context.hide_edge_borders,
        HideEdgeBorders::Vertical | HideEdgeBorders::Both
    ) {
        edges.set(ResizeEdge::LEFT, rect.loc.x != workspace_area.loc.x);
        edges.set(
            ResizeEdge::RIGHT,
            rect.loc.x + rect.size.w != workspace_area.loc.x + workspace_area.size.w,
        );
    }
    if matches!(
        context.hide_edge_borders,
        HideEdgeBorders::Horizontal | HideEdgeBorders::Both
    ) {
        edges.set(ResizeEdge::TOP, rect.loc.y != workspace_area.loc.y);
        edges.set(
            ResizeEdge::BOTTOM,
            rect.loc.y + rect.size.h != workspace_area.loc.y + workspace_area.size.h,
        );
    }
    let smart = context.smart_borders == SmartBorders::On
        || context.smart_borders == SmartBorders::NoGaps && !context.gaps_to_edge;
    if smart && context.only_visible_view {
        edges = ResizeEdge::empty();
    }
    edges
}

/// The part of a tab or stack child's top border that the parent's strip does not cover.
fn record_uncovered_top_border<I>(
    id: NodeId,
    rect: Rectangle<f64, Logical>,
    width: f64,
    covering_titlebar: Option<Rectangle<f64, Logical>>,
    result: &mut Geometry<I>,
) {
    let top = Rectangle::new(
        rect.loc - Point::from((0., width)),
        Size::from((rect.size.w, width)),
    );
    let uncovered = subtract_horizontal(top, covering_titlebar);
    if !uncovered.is_empty() {
        result.uncovered_top_borders.insert(id, uncovered);
    }
}

/// Records a leaf's own titlebar across the top of its box.
fn emit_leaf_titlebar<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    tile: &crate::layout::tile::Tile<W>,
    id: NodeId,
    rect: Rectangle<f64, Logical>,
    decorated_corners: DecoratedCorners,
    ipc_origin: Point<f64, Logical>,
    result: &mut Geometry<W::Id>,
) {
    let titlebar = Rectangle::new(rect.loc, (rect.size.w, context.titlebar_height).into());
    result.titlebar_leaves.insert(id, id);
    result.titlebar_corners.insert(
        id,
        DecoratedCorners {
            top_left: decorated_corners.top_left,
            top_right: decorated_corners.top_right,
            ..DecoratedCorners::NONE
        },
    );
    result.titlebars.insert(
        id,
        Titlebar {
            target: tile.window().id().clone(),
            rect: titlebar,
            ipc_rect: Rectangle::new(titlebar.loc - ipc_origin, titlebar.size),
            title: tile.window().title(),
            marks: tile.window().marks(),
            state: TitlebarState::Unfocused,
            visible: true,
        },
    );
    result.titlebar_attached.insert(id);
}

/// A leaf's content inside its borders. The titlebar occupies the top slot and the side
/// borders begin below it, matching arrange_container() (sway/desktop/transaction.c:409-446).
fn content_rect(
    mut rect: Rectangle<f64, Logical>,
    edges: ResizeEdge,
    width: f64,
    top: f64,
) -> Rectangle<f64, Logical> {
    let left = width * f64::from(edges.contains(ResizeEdge::LEFT));
    let right = width * f64::from(edges.contains(ResizeEdge::RIGHT));
    let bottom = width * f64::from(edges.contains(ResizeEdge::BOTTOM));
    rect.loc += Point::from((left, top));
    rect.size.w = (rect.size.w - left - right).max(0.);
    rect.size.h = (rect.size.h - top - bottom).max(0.);
    rect
}

/// Each child's share of a linear split. Children hidden under a fullscreen view get none and
/// the others are renormalised over the visible ones.
fn visible_shares(
    mapped_under_fullscreen: &HashSet<NodeId>,
    children: &[NodeId],
    percents: &[f64],
) -> Vec<f64> {
    if mapped_under_fullscreen.is_empty() {
        return percents.to_vec();
    }
    let visible_total = children
        .iter()
        .zip(percents)
        .filter(|(child, _)| !mapped_under_fullscreen.contains(child))
        .map(|(_, percent)| percent)
        .sum::<f64>();
    children
        .iter()
        .zip(percents)
        .map(|(child, percent)| {
            if mapped_under_fullscreen.contains(child) {
                0.
            } else {
                *percent / visible_total
            }
        })
        .collect()
}

/// The rounded corners a linear split's child may draw. With gaps every child is separate and
/// draws all four; without, only the corners on the split's outer edge stay rounded.
fn child_corners(
    parent: DecoratedCorners,
    index: usize,
    len: usize,
    layout: Layout,
    suppress_gaps: bool,
) -> DecoratedCorners {
    if !suppress_gaps {
        return DecoratedCorners::ALL;
    }
    let mut corners = parent;
    let first = index == 0;
    let last = index + 1 == len;
    if layout == Layout::SplitH {
        corners.top_left &= first;
        corners.bottom_left &= first;
        corners.top_right &= last;
        corners.bottom_right &= last;
    } else {
        corners.top_left &= first;
        corners.top_right &= first;
        corners.bottom_left &= last;
        corners.bottom_right &= last;
    }
    corners
}

/// A horizontal or vertical split container being laid out.
struct LinearSplit<'a> {
    layout: Layout,
    children: &'a [NodeId],
    percents: &'a [f64],
}

/// A rect's extent along a linear split's axis: width for SplitH, height for SplitV.
pub(super) fn axis_extent(layout: Layout, rect: Rectangle<f64, Logical>) -> f64 {
    match layout {
        Layout::SplitH => rect.size.w,
        Layout::SplitV => rect.size.h,
        Layout::Tabbed | Layout::Stacked => unreachable!("only linear splits have an axis"),
    }
}

fn assign_linear_split<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    split: LinearSplit<'_>,
    assignment: Assignment,
    result: &mut Geometry<W::Id>,
) {
    let LinearSplit {
        layout,
        children,
        percents,
    } = split;
    let Assignment {
        rect,
        decorated_corners,
        suppress_gaps,
        ..
    } = assignment;
    let extent = axis_extent(layout, rect);
    let gap = if suppress_gaps {
        0.
    } else {
        split_gap(
            context.gaps,
            extent,
            children.len(),
            if layout == Layout::SplitH {
                MIN_SANE_W
            } else {
                MIN_SANE_H
            },
        )
    };
    let available = extent - gap * children.len().saturating_sub(1) as f64;
    let mut cursor = match layout {
        Layout::SplitH => rect.loc.x,
        Layout::SplitV => rect.loc.y,
        _ => unreachable!(),
    };
    let shares = visible_shares(context.mapped_under_fullscreen, children, percents);
    for (index, (child, percent)) in children.iter().zip(shares).enumerate() {
        let extent = available.max(0.) * percent;
        let child_rect = match layout {
            Layout::SplitH => Rectangle::new(
                Point::from((cursor, rect.loc.y)),
                Size::from((extent, rect.size.h)),
            ),
            Layout::SplitV => Rectangle::new(
                Point::from((rect.loc.x, cursor)),
                Size::from((rect.size.w, extent)),
            ),
            _ => unreachable!(),
        };
        let child_corners = child_corners(
            decorated_corners,
            index,
            children.len(),
            layout,
            suppress_gaps,
        );
        assign(
            context,
            Assignment {
                id: *child,
                rect: child_rect,
                covering_titlebar: None,
                decorated_by_parent: false,
                decorated_corners: child_corners,
                suppress_gaps,
                ipc_origin: rect.loc,
            },
            result,
        );
        cursor += extent + gap;
    }
}

fn assign_strip<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    layout: Layout,
    children: &[NodeId],
    rect: Rectangle<f64, Logical>,
    decorated_corners: DecoratedCorners,
    result: &mut Geometry<W::Id>,
) {
    let nodes = context.nodes;
    let title_formats = context.title_formats;
    let titlebar_height = context.titlebar_height;
    let fullscreen = context.fullscreen;

    let count = children.len();
    let total_height = if !fullscreen.is_empty() {
        0.
    } else if layout == Layout::Stacked {
        titlebar_height * count as f64
    } else {
        titlebar_height
    };
    let mut content = rect;
    content.loc.y += total_height;
    content.size.h = (content.size.h - total_height).max(0.);
    for (index, child) in children.iter().enumerate() {
        if let Some(entry) = first_window(nodes, title_formats, *child) {
            emit_strip_titlebar(
                context,
                layout,
                rect,
                StripSlot {
                    child: *child,
                    index,
                    count,
                },
                entry,
                decorated_corners,
                result,
            );
        }
        assign(
            context,
            Assignment {
                id: *child,
                rect: content,
                covering_titlebar: result.titlebars.get(child).map(|titlebar| titlebar.rect),
                decorated_by_parent: true,
                decorated_corners: DecoratedCorners {
                    bottom_left: decorated_corners.bottom_left,
                    bottom_right: decorated_corners.bottom_right,
                    ..DecoratedCorners::NONE
                },
                suppress_gaps: true,
                ipc_origin: rect.loc,
            },
            result,
        );
    }
}

/// One entry's position in a tabbed or stacked strip.
struct StripSlot {
    child: NodeId,
    index: usize,
    count: usize,
}

/// Records a strip entry: tabs share the strip's width, stacked entries take one row each, and
/// only the strip's outer top corners stay rounded.
fn emit_strip_titlebar<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    layout: Layout,
    rect: Rectangle<f64, Logical>,
    slot: StripSlot,
    (leaf, target, title): (NodeId, W::Id, String),
    decorated_corners: DecoratedCorners,
    result: &mut Geometry<W::Id>,
) {
    let StripSlot {
        child,
        index,
        count,
    } = slot;
    let titlebar_height = context.titlebar_height;
    let title_rect = if layout == Layout::Tabbed {
        let width = rect.size.w / count.max(1) as f64;
        Rectangle::new(
            Point::from((rect.loc.x + width * index as f64, rect.loc.y)),
            Size::from((width, titlebar_height)),
        )
    } else {
        Rectangle::new(
            Point::from((rect.loc.x, rect.loc.y + titlebar_height * index as f64)),
            Size::from((rect.size.w, titlebar_height)),
        )
    };
    let titlebar_corners = match layout {
        Layout::Tabbed => DecoratedCorners {
            top_left: index == 0 && decorated_corners.top_left,
            top_right: index + 1 == count && decorated_corners.top_right,
            ..DecoratedCorners::NONE
        },
        Layout::Stacked if index == 0 => DecoratedCorners {
            top_left: decorated_corners.top_left,
            top_right: decorated_corners.top_right,
            ..DecoratedCorners::NONE
        },
        Layout::Stacked => DecoratedCorners::NONE,
        Layout::SplitH | Layout::SplitV => unreachable!("only tabbed and stacked draw strips"),
    };
    result.titlebar_leaves.insert(child, leaf);
    result.titlebar_corners.insert(child, titlebar_corners);
    result.titlebars.insert(
        child,
        Titlebar {
            target,
            rect: title_rect,
            ipc_rect: Rectangle::new(title_rect.loc - rect.loc, title_rect.size),
            title,
            marks: context
                .nodes
                .get(&leaf)
                .and_then(|node| match &node.value {
                    TreeNode::Leaf { tile } => Some(tile.window().marks()),
                    TreeNode::Split { .. } => None,
                })
                .unwrap_or_default(),
            state: TitlebarState::Unfocused,
            visible: context.fullscreen.is_empty(),
        },
    );
}

fn assign<W: LayoutElement>(
    context: &AssignContext<'_, W>,
    assignment: Assignment,
    result: &mut Geometry<W::Id>,
) {
    let Some(node) = context.nodes.get(&assignment.id) else {
        return;
    };
    result.ipc_nodes.insert(assignment.id, assignment.rect);
    match &node.value {
        TreeNode::Leaf { tile } => assign_leaf(context, tile, assignment, result),
        TreeNode::Split {
            layout,
            children,
            percents,
        } => match layout {
            Layout::SplitH | Layout::SplitV => assign_linear_split(
                context,
                LinearSplit {
                    layout: *layout,
                    children,
                    percents,
                },
                assignment,
                result,
            ),
            Layout::Tabbed | Layout::Stacked => assign_strip(
                context,
                *layout,
                children,
                assignment.rect,
                assignment.decorated_corners,
                result,
            ),
        },
    }
}

fn subtract_horizontal(
    rect: Rectangle<f64, Logical>,
    covered: Option<Rectangle<f64, Logical>>,
) -> Vec<Rectangle<f64, Logical>> {
    let Some(covered) = covered else {
        return vec![rect];
    };
    let covered_left = covered.loc.x.max(rect.loc.x);
    let covered_right = (covered.loc.x + covered.size.w).min(rect.loc.x + rect.size.w);
    if covered_left >= covered_right {
        return vec![rect];
    }
    let left = covered_left - rect.loc.x;
    let right = rect.loc.x + rect.size.w - covered_right;
    [
        Rectangle::new(rect.loc, Size::from((left, rect.size.h))),
        Rectangle::new(
            Point::from((covered_right, rect.loc.y)),
            Size::from((right, rect.size.h)),
        ),
    ]
    .into_iter()
    .filter(|part| part.size.w > 0.)
    .collect()
}

/// Sway's smallest sane container width and height (include/sway/tree/node.h:8-9).
const MIN_SANE_W: f64 = 100.;
const MIN_SANE_H: f64 = 60.;

/// The inner gap between a split's children, shrunk so that every child keeps at least
/// `minimum_child_extent`, and floored to whole pixels (`apply_horiz_layout` and
/// `apply_vert_layout`, sway/tree/arrange.c:70-73 and 155-158).
fn split_gap(requested: f64, extent: f64, children: usize, minimum_child_extent: f64) -> f64 {
    let separators = children.saturating_sub(1);
    if separators == 0 {
        return 0.;
    }
    let total = (requested * separators as f64)
        .min((extent - minimum_child_extent * children as f64).max(0.));
    (total / separators as f64).floor()
}

fn first_window<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    title_formats: &HashMap<NodeId, String>,
    id: NodeId,
) -> Option<(NodeId, W::Id, String)> {
    match &nodes.get(&id)?.value {
        TreeNode::Leaf { tile } => Some((id, tile.window().id().clone(), tile.window().title())),
        TreeNode::Split {
            layout, children, ..
        } => {
            let (leaf, target, _) = first_window(nodes, title_formats, *children.first()?)?;
            Some((
                leaf,
                target,
                format_representation(
                    title_formats.get(&id).map(String::as_str),
                    &tree_representation(nodes, title_formats, *layout, children),
                ),
            ))
        }
    }
}

/// A split's `representation`, such as `H[a V[b c]]` (`container_build_representation`,
/// sway/tree/container.c:702-748).
fn tree_representation<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    title_formats: &HashMap<NodeId, String>,
    layout: Layout,
    children: &[NodeId],
) -> String {
    let prefix = match layout {
        Layout::SplitH => 'H',
        Layout::SplitV => 'V',
        Layout::Tabbed => 'T',
        Layout::Stacked => 'S',
    };
    let children = children
        .iter()
        .filter_map(|child| match &nodes.get(child)?.value {
            TreeNode::Leaf { tile } => Some(tile.window().title()),
            TreeNode::Split {
                layout, children, ..
            } => Some(format_representation(
                title_formats.get(child).map(String::as_str),
                &tree_representation(nodes, title_formats, *layout, children),
            )),
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{prefix}[{children}]")
}

/// A split's titlebar text: its `title_format` with `%title` replaced by the representation, as
/// for a container without a view (`parse_title_format` and `container_update_representation`,
/// sway/tree/container.c:632-695 and 750-773).
fn format_representation(format: Option<&str>, representation: &str) -> String {
    format.filter(|format| *format != "%title").map_or_else(
        || representation.to_owned(),
        |format| format.replace("%title", representation),
    )
}

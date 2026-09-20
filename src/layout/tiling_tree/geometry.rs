use std::collections::{HashMap, HashSet};

use smithay::utils::{Logical, Point, Rectangle, Size};
use swayward_config::{HideEdgeBorders, SmartBorders, Struts};

use super::{Layout, Node, NodeId, TreeNode};
use crate::layout::titlebar::{Titlebar, TitlebarState};
use crate::layout::LayoutElement;
use crate::utils::ResizeEdge;

pub(crate) struct Geometry<I> {
    pub leaf_contents: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub ipc_nodes: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub titlebars: HashMap<NodeId, Titlebar<I>>,
    pub titlebar_attached: HashSet<NodeId>,
    pub border_edges: HashMap<NodeId, ResizeEdge>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compute<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    root: NodeId,
    view_size: Size<f64, Logical>,
    parent_area: Rectangle<f64, Logical>,
    scale: f64,
    struts: Struts,
    gaps: f64,
    outer_gaps_configured: bool,
    gaps_to_edge: bool,
    titlebar_height: f64,
    fullscreen: &HashSet<NodeId>,
    hide_edge_borders: HideEdgeBorders,
    smart_borders: SmartBorders,
    only_visible_view: bool,
) -> Geometry<W::Id> {
    let mut result = Geometry {
        leaf_contents: HashMap::new(),
        ipc_nodes: HashMap::new(),
        titlebars: HashMap::new(),
        titlebar_attached: HashSet::new(),
        border_edges: HashMap::new(),
    };
    let gaps = gaps.max(0.);
    let mut area = if fullscreen.is_empty() {
        apply_struts(parent_area, scale, struts)
    } else {
        Rectangle::from_size(view_size)
    };
    let fullscreen_root = fullscreen.iter().copied().next();
    if !outer_gaps_configured {
        area.loc.x += gaps;
        area.loc.y += gaps;
        area.size.w = (area.size.w - gaps * 2.).max(0.);
        area.size.h = (area.size.h - gaps * 2.).max(0.);
    }
    let workspace_area = area;
    assign(
        nodes,
        root,
        area,
        gaps,
        titlebar_height,
        fullscreen,
        false,
        false,
        Point::default(),
        workspace_area,
        gaps_to_edge,
        hide_edge_borders,
        smart_borders,
        only_visible_view,
        &mut result,
    );
    if let Some(fullscreen_root) = fullscreen_root {
        assign(
            nodes,
            fullscreen_root,
            area,
            gaps,
            titlebar_height,
            fullscreen,
            false,
            false,
            Point::default(),
            workspace_area,
            gaps_to_edge,
            hide_edge_borders,
            smart_borders,
            only_visible_view,
            &mut result,
        );
    }
    result
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
    let mut size_diff = (loc - working_area.loc).to_size();
    size_diff.w = working_area.size.w.min(size_diff.w);
    size_diff.h = working_area.size.h.min(size_diff.h);
    working_area.size -= size_diff;
    working_area.loc = loc;
    working_area
}

#[allow(clippy::too_many_arguments)]
fn assign<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    id: NodeId,
    mut rect: Rectangle<f64, Logical>,
    gaps: f64,
    titlebar_height: f64,
    fullscreen: &HashSet<NodeId>,
    decorated_by_parent: bool,
    suppress_gaps: bool,
    ipc_origin: Point<f64, Logical>,
    workspace_area: Rectangle<f64, Logical>,
    gaps_to_edge: bool,
    hide_edge_borders: HideEdgeBorders,
    smart_borders: SmartBorders,
    only_visible_view: bool,
    result: &mut Geometry<W::Id>,
) {
    let Some(node) = nodes.get(&id) else { return };
    result.ipc_nodes.insert(id, rect);
    match &node.value {
        TreeNode::Leaf { tile } => {
            let mut edges = ResizeEdge::all();
            if matches!(
                hide_edge_borders,
                HideEdgeBorders::Vertical | HideEdgeBorders::Both
            ) {
                edges.set(ResizeEdge::LEFT, rect.loc.x != workspace_area.loc.x);
                edges.set(
                    ResizeEdge::RIGHT,
                    rect.loc.x + rect.size.w != workspace_area.loc.x + workspace_area.size.w,
                );
            }
            if matches!(
                hide_edge_borders,
                HideEdgeBorders::Horizontal | HideEdgeBorders::Both
            ) {
                edges.set(ResizeEdge::TOP, rect.loc.y != workspace_area.loc.y);
                edges.set(
                    ResizeEdge::BOTTOM,
                    rect.loc.y + rect.size.h != workspace_area.loc.y + workspace_area.size.h,
                );
            }
            let smart = smart_borders == SmartBorders::On
                || smart_borders == SmartBorders::NoGaps && !gaps_to_edge;
            if smart && only_visible_view {
                edges = ResizeEdge::empty();
            }
            result.border_edges.insert(id, edges);
            if decorated_by_parent && fullscreen.is_empty() {
                result.titlebar_attached.insert(id);
            }

            if !decorated_by_parent && !fullscreen.contains(&id) && tile.has_sway_titlebar() {
                let titlebar = Rectangle::new(rect.loc, (rect.size.w, titlebar_height).into());
                result.titlebars.insert(
                    id,
                    Titlebar {
                        target: tile.window().id().clone(),
                        rect: titlebar,
                        ipc_rect: Rectangle::new(titlebar.loc - ipc_origin, titlebar.size),
                        title: tile.window().title(),
                        state: TitlebarState::Unfocused,
                        visible: true,
                    },
                );
                rect.loc.y += titlebar_height;
                rect.size.h = (rect.size.h - titlebar_height).max(0.);
            }
            result.leaf_contents.insert(id, rect);
        }
        TreeNode::Split {
            layout,
            children,
            percents,
        } => match layout {
            Layout::SplitH | Layout::SplitV => {
                let extent = match layout {
                    Layout::SplitH => rect.size.w,
                    Layout::SplitV => rect.size.h,
                    _ => unreachable!(),
                };
                let gap = if suppress_gaps {
                    0.
                } else {
                    split_gap(
                        gaps,
                        extent,
                        children.len(),
                        if *layout == Layout::SplitH { 100. } else { 60. },
                    )
                };
                let available = extent - gap * children.len().saturating_sub(1) as f64;
                let mut cursor = match layout {
                    Layout::SplitH => rect.loc.x,
                    Layout::SplitV => rect.loc.y,
                    _ => unreachable!(),
                };
                for (index, (child, percent)) in children.iter().zip(percents).enumerate() {
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
                    // Only a child whose own top edge is the container's top
                    // edge meets the tab strip. A vertical split stacks its
                    // children, so everything below the first one is a row
                    // further down the window and keeps its rounded corners.
                    let child_decorated_by_parent =
                        decorated_by_parent && (*layout == Layout::SplitH || index == 0);
                    assign(
                        nodes,
                        *child,
                        child_rect,
                        gaps,
                        titlebar_height,
                        fullscreen,
                        child_decorated_by_parent,
                        suppress_gaps,
                        rect.loc,
                        workspace_area,
                        gaps_to_edge,
                        hide_edge_borders,
                        smart_borders,
                        only_visible_view,
                        result,
                    );
                    cursor += extent + gap;
                }
            }
            Layout::Tabbed | Layout::Stacked => {
                let count = children.len();
                let total_height = if !fullscreen.is_empty() {
                    0.
                } else if *layout == Layout::Stacked {
                    titlebar_height * count as f64
                } else {
                    titlebar_height
                };
                let mut content = rect;
                content.loc.y += total_height;
                content.size.h = (content.size.h - total_height).max(0.);
                for (index, child) in children.iter().enumerate() {
                    if let Some((leaf, target, title)) = first_window(nodes, *child) {
                        let title_rect = if *layout == Layout::Tabbed {
                            let width = rect.size.w / count.max(1) as f64;
                            Rectangle::new(
                                Point::from((rect.loc.x + width * index as f64, rect.loc.y)),
                                Size::from((width, titlebar_height)),
                            )
                        } else {
                            Rectangle::new(
                                Point::from((
                                    rect.loc.x,
                                    rect.loc.y + titlebar_height * index as f64,
                                )),
                                Size::from((rect.size.w, titlebar_height)),
                            )
                        };
                        result.titlebars.insert(
                            leaf,
                            Titlebar {
                                target,
                                rect: title_rect,
                                ipc_rect: Rectangle::new(
                                    title_rect.loc - rect.loc,
                                    title_rect.size,
                                ),
                                title,
                                state: TitlebarState::Unfocused,
                                visible: fullscreen.is_empty(),
                            },
                        );
                    }
                    assign(
                        nodes,
                        *child,
                        content,
                        gaps,
                        titlebar_height,
                        fullscreen,
                        true,
                        true,
                        rect.loc,
                        workspace_area,
                        gaps_to_edge,
                        hide_edge_borders,
                        smart_borders,
                        only_visible_view,
                        result,
                    );
                }
            }
        },
    }
}

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
    id: NodeId,
) -> Option<(NodeId, W::Id, String)> {
    match &nodes.get(&id)?.value {
        TreeNode::Leaf { tile } => Some((id, tile.window().id().clone(), tile.window().title())),
        TreeNode::Split {
            layout, children, ..
        } => {
            let (leaf, target, _) = first_window(nodes, *children.first()?)?;
            Some((leaf, target, tree_representation(nodes, *layout, children)))
        }
    }
}

fn tree_representation<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
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
            } => Some(tree_representation(nodes, *layout, children)),
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{prefix}[{children}]")
}

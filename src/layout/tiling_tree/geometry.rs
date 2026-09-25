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

#[allow(clippy::too_many_arguments)]
pub(crate) fn compute<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    title_formats: &HashMap<NodeId, String>,
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
    mapped_under_fullscreen: &HashSet<NodeId>,
    hide_edge_borders: HideEdgeBorders,
    smart_borders: SmartBorders,
    visible_leaves: &HashSet<NodeId>,
    draw_uncovered_top_border: bool,
) -> Geometry<W::Id> {
    let only_visible_view = visible_leaves.len() == 1;
    let mut result = Geometry {
        leaf_boxes: HashMap::new(),
        leaf_contents: HashMap::new(),
        leaf_ipc_rects: HashMap::new(),
        ipc_nodes: HashMap::new(),
        titlebars: HashMap::new(),
        titlebar_leaves: HashMap::new(),
        titlebar_attached: HashSet::new(),
        titlebar_owned_by_parent: HashSet::new(),
        border_edges: HashMap::new(),
        border_visible: HashSet::new(),
        border_corners: HashMap::new(),
        titlebar_corners: HashMap::new(),
        uncovered_top_borders: HashMap::new(),
    };
    let gaps = gaps.max(0.);
    let mut area = apply_struts(parent_area, scale, struts);
    if !outer_gaps_configured {
        area.loc.x += gaps;
        area.loc.y += gaps;
        area.size.w = (area.size.w - gaps * 2.).max(0.);
        area.size.h = (area.size.h - gaps * 2.).max(0.);
    }
    let fullscreen_root = fullscreen.iter().copied().next();
    let workspace_area = area;
    assign(
        nodes,
        title_formats,
        root,
        area,
        gaps,
        titlebar_height,
        &HashSet::new(),
        mapped_under_fullscreen,
        None,
        false,
        DecoratedCorners::ALL,
        false,
        Point::default(),
        workspace_area,
        gaps_to_edge,
        hide_edge_borders,
        smart_borders,
        only_visible_view,
        draw_uncovered_top_border,
        &mut result,
    );
    if let Some(fullscreen_root) = fullscreen_root {
        let area = Rectangle::from_size(view_size);
        assign(
            nodes,
            title_formats,
            fullscreen_root,
            area,
            gaps,
            titlebar_height,
            fullscreen,
            mapped_under_fullscreen,
            None,
            false,
            DecoratedCorners::NONE,
            false,
            Point::default(),
            workspace_area,
            gaps_to_edge,
            hide_edge_borders,
            smart_borders,
            only_visible_view,
            draw_uncovered_top_border,
            &mut result,
        );
        result
            .titlebars
            .retain(|id, _| !contains_node(nodes, fullscreen_root, *id));
        result
            .titlebar_attached
            .retain(|id| !contains_node(nodes, fullscreen_root, *id));
        result
            .titlebar_owned_by_parent
            .retain(|id| !contains_node(nodes, fullscreen_root, *id));
    }
    for id in mapped_under_fullscreen {
        result.titlebars.remove(id);
        result.titlebar_leaves.remove(id);
        result.titlebar_attached.remove(id);
        result.titlebar_owned_by_parent.remove(id);
    }
    result.border_visible.extend(
        result
            .leaf_boxes
            .keys()
            .filter(|id| visible_leaves.contains(id)),
    );
    result
        .uncovered_top_borders
        .retain(|id, _| visible_leaves.contains(id));
    // A tabbed or stacked container shows only its active child; sway sends
    // the rest to disable_container, which hides the whole subtree, strips
    // included (sway/desktop/transaction.c:316-321). Titlebars are emitted for
    // every strip in the tree, so hide those whose branch is not shown. A
    // branch is shown exactly when it holds a visible leaf, because
    // visible_leaves already walks only the active child of each tab level.
    //
    // A strip entry belongs to the container that draws the strip, so it is
    // shown when that container is: an inactive tab keeps its title in a
    // visible strip even though its own subtree is hidden. A leaf's own
    // titlebar belongs to the leaf.
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
    result
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

fn contains_node<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    root: NodeId,
    id: NodeId,
) -> bool {
    root == id
        || match nodes.get(&root).map(|node| &node.value) {
            Some(TreeNode::Split { children, .. }) => children
                .iter()
                .any(|child| contains_node(nodes, *child, id)),
            _ => false,
        }
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
    title_formats: &HashMap<NodeId, String>,
    id: NodeId,
    mut rect: Rectangle<f64, Logical>,
    gaps: f64,
    titlebar_height: f64,
    fullscreen: &HashSet<NodeId>,
    mapped_under_fullscreen: &HashSet<NodeId>,
    covering_titlebar: Option<Rectangle<f64, Logical>>,
    decorated_by_parent: bool,
    decorated_corners: DecoratedCorners,
    suppress_gaps: bool,
    ipc_origin: Point<f64, Logical>,
    workspace_area: Rectangle<f64, Logical>,
    gaps_to_edge: bool,
    hide_edge_borders: HideEdgeBorders,
    smart_borders: SmartBorders,
    only_visible_view: bool,
    draw_uncovered_top_border: bool,
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
            // This is the leaf's geometric identity. Rendering, hit testing, movement,
            // sizing and IPC start from this box; content is derived below in one pass.
            result.leaf_boxes.insert(id, rect);
            let decorated_by_parent = decorated_by_parent && fullscreen.is_empty();
            if decorated_by_parent {
                result.titlebar_attached.insert(id);
                result.titlebar_owned_by_parent.insert(id);
            }

            let has_titlebar =
                !decorated_by_parent && !fullscreen.contains(&id) && tile.has_sway_titlebar();
            if has_titlebar {
                let titlebar = Rectangle::new(rect.loc, (rect.size.w, titlebar_height).into());
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

            let border_corners = if has_titlebar {
                DecoratedCorners {
                    top_left: false,
                    top_right: false,
                    ..decorated_corners
                }
            } else {
                decorated_corners
            };
            result.border_corners.insert(id, border_corners);

            let mut ipc_rect = rect;
            if has_titlebar {
                ipc_rect.loc.y += titlebar_height;
                ipc_rect.size.h = (ipc_rect.size.h - titlebar_height).max(0.);
            }
            result.leaf_ipc_rects.insert(id, ipc_rect);

            // Match sway's arrange_container table: the titlebar occupies the top slot,
            // while side borders begin below it at the content rectangle.
            let width = tile.configured_border_width();
            if draw_uncovered_top_border
                && decorated_by_parent
                && edges.contains(ResizeEdge::TOP)
                && width > 0.
            {
                let top = Rectangle::new(
                    rect.loc - Point::from((0., width)),
                    Size::from((rect.size.w, width)),
                );
                let uncovered = subtract_horizontal(top, covering_titlebar);
                if !uncovered.is_empty() {
                    result.uncovered_top_borders.insert(id, uncovered);
                }
            }
            let left = width * f64::from(edges.contains(ResizeEdge::LEFT));
            let right = width * f64::from(edges.contains(ResizeEdge::RIGHT));
            let top = if has_titlebar {
                titlebar_height
            } else if decorated_by_parent {
                0.
            } else {
                width * f64::from(edges.contains(ResizeEdge::TOP))
            };
            let bottom = width * f64::from(edges.contains(ResizeEdge::BOTTOM));
            rect.loc += Point::from((left, top));
            rect.size.w = (rect.size.w - left - right).max(0.);
            rect.size.h = (rect.size.h - top - bottom).max(0.);
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
                let visible_total = children
                    .iter()
                    .zip(percents)
                    .filter(|(child, _)| !mapped_under_fullscreen.contains(child))
                    .map(|(_, percent)| percent)
                    .sum::<f64>();
                for (index, (child, percent)) in children.iter().zip(percents).enumerate() {
                    let percent = if mapped_under_fullscreen.contains(child) {
                        0.
                    } else if mapped_under_fullscreen.is_empty() {
                        *percent
                    } else {
                        *percent / visible_total
                    };
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
                    let mut child_corners = if suppress_gaps {
                        decorated_corners
                    } else {
                        DecoratedCorners::ALL
                    };
                    if suppress_gaps && *layout == Layout::SplitH {
                        child_corners.top_left &= index == 0;
                        child_corners.bottom_left &= index == 0;
                        child_corners.top_right &= index + 1 == children.len();
                        child_corners.bottom_right &= index + 1 == children.len();
                    } else if suppress_gaps {
                        child_corners.top_left &= index == 0;
                        child_corners.top_right &= index == 0;
                        child_corners.bottom_left &= index + 1 == children.len();
                        child_corners.bottom_right &= index + 1 == children.len();
                    }
                    assign(
                        nodes,
                        title_formats,
                        *child,
                        child_rect,
                        gaps,
                        titlebar_height,
                        fullscreen,
                        mapped_under_fullscreen,
                        None,
                        false,
                        child_corners,
                        suppress_gaps,
                        rect.loc,
                        workspace_area,
                        gaps_to_edge,
                        hide_edge_borders,
                        smart_borders,
                        only_visible_view,
                        draw_uncovered_top_border,
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
                    if let Some((leaf, target, title)) = first_window(nodes, title_formats, *child)
                    {
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
                            _ => unreachable!(),
                        };
                        result.titlebar_leaves.insert(*child, leaf);
                        result.titlebar_corners.insert(*child, titlebar_corners);
                        result.titlebars.insert(
                            *child,
                            Titlebar {
                                target,
                                rect: title_rect,
                                ipc_rect: Rectangle::new(
                                    title_rect.loc - rect.loc,
                                    title_rect.size,
                                ),
                                title,
                                marks: nodes
                                    .get(&leaf)
                                    .and_then(|node| match &node.value {
                                        TreeNode::Leaf { tile } => Some(tile.window().marks()),
                                        TreeNode::Split { .. } => None,
                                    })
                                    .unwrap_or_default(),
                                state: TitlebarState::Unfocused,
                                visible: fullscreen.is_empty(),
                            },
                        );
                    }
                    assign(
                        nodes,
                        title_formats,
                        *child,
                        content,
                        gaps,
                        titlebar_height,
                        fullscreen,
                        mapped_under_fullscreen,
                        result.titlebars.get(child).map(|titlebar| titlebar.rect),
                        true,
                        DecoratedCorners {
                            bottom_left: decorated_corners.bottom_left,
                            bottom_right: decorated_corners.bottom_right,
                            ..DecoratedCorners::NONE
                        },
                        true,
                        rect.loc,
                        workspace_area,
                        gaps_to_edge,
                        hide_edge_borders,
                        smart_borders,
                        only_visible_view,
                        draw_uncovered_top_border,
                        result,
                    );
                }
            }
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

fn format_representation(format: Option<&str>, representation: &str) -> String {
    format.filter(|format| *format != "%title").map_or_else(
        || representation.to_owned(),
        |format| format.replace("%title", representation),
    )
}

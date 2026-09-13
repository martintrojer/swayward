use std::collections::{HashMap, HashSet};

use smithay::utils::{Logical, Point, Rectangle, Size};
use swayward_config::Struts;

use super::{Layout, Node, NodeId, TreeNode};
use crate::layout::titlebar::Titlebar;
use crate::layout::LayoutElement;

pub(crate) struct Geometry<I> {
    pub nodes: HashMap<NodeId, Rectangle<f64, Logical>>,
    pub titlebars: HashMap<NodeId, Titlebar<I>>,
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
    titlebar_height: f64,
    fullscreen: &HashSet<NodeId>,
) -> Geometry<W::Id> {
    let mut result = Geometry {
        nodes: HashMap::new(),
        titlebars: HashMap::new(),
    };
    let gaps = gaps.max(0.);
    let mut area = if fullscreen.is_empty() {
        apply_struts(parent_area, scale, struts)
    } else {
        Rectangle::from_size(view_size)
    };
    area.loc.x += gaps;
    area.loc.y += gaps;
    area.size.w = (area.size.w - gaps * 2.).max(0.);
    area.size.h = (area.size.h - gaps * 2.).max(0.);
    assign(
        nodes,
        root,
        area,
        gaps,
        titlebar_height,
        fullscreen,
        false,
        Point::default(),
        &mut result,
    );
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
    ipc_origin: Point<f64, Logical>,
    result: &mut Geometry<W::Id>,
) {
    let Some(node) = nodes.get(&id) else { return };
    match &node.value {
        TreeNode::Leaf { tile } => {
            if !decorated_by_parent
                && !fullscreen.contains(&id)
                && tile.effective_border_width().is_some()
            {
                let titlebar = Rectangle::new(rect.loc, (rect.size.w, titlebar_height).into());
                result.titlebars.insert(
                    id,
                    Titlebar {
                        target: tile.window().id().clone(),
                        rect: titlebar,
                        ipc_rect: Rectangle::new(titlebar.loc - ipc_origin, titlebar.size),
                        title: tile.window().title(),
                        active: false,
                        visible: true,
                    },
                );
                rect.loc.y += titlebar_height;
                rect.size.h = (rect.size.h - titlebar_height).max(0.);
            }
            result.nodes.insert(id, rect);
        }
        TreeNode::Split {
            layout,
            children,
            percents,
        } => match layout {
            Layout::SplitH | Layout::SplitV => {
                let available = match layout {
                    Layout::SplitH => rect.size.w,
                    Layout::SplitV => rect.size.h,
                    _ => unreachable!(),
                } - gaps * children.len().saturating_sub(1) as f64;
                let mut cursor = match layout {
                    Layout::SplitH => rect.loc.x,
                    Layout::SplitV => rect.loc.y,
                    _ => unreachable!(),
                };
                for (child, percent) in children.iter().zip(percents) {
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
                    assign(
                        nodes,
                        *child,
                        child_rect,
                        gaps,
                        titlebar_height,
                        fullscreen,
                        false,
                        rect.loc,
                        result,
                    );
                    cursor += extent + gaps;
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
                                active: false,
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
                        rect.loc,
                        result,
                    );
                }
            }
        },
    }
}

fn first_window<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    id: NodeId,
) -> Option<(NodeId, W::Id, String)> {
    match &nodes.get(&id)?.value {
        TreeNode::Leaf { tile } => Some((id, tile.window().id().clone(), tile.window().title())),
        TreeNode::Split { children, .. } => first_window(nodes, *children.first()?),
    }
}

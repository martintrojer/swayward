use std::collections::HashMap;

use smithay::utils::{Logical, Point, Rectangle, Size};

use super::{Layout, Node, NodeId, TreeNode};
use crate::layout::LayoutElement;

pub(crate) fn compute<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    root: NodeId,
    view_size: Size<f64, Logical>,
    gaps: f64,
) -> HashMap<NodeId, Rectangle<f64, Logical>> {
    let mut result = HashMap::new();
    assign(
        nodes,
        root,
        Rectangle::from_size(view_size),
        gaps.max(0.),
        &mut result,
    );
    result
}

fn assign<W: LayoutElement>(
    nodes: &HashMap<NodeId, Node<W>>,
    id: NodeId,
    rect: Rectangle<f64, Logical>,
    gaps: f64,
    result: &mut HashMap<NodeId, Rectangle<f64, Logical>>,
) {
    let Some(node) = nodes.get(&id) else { return };
    match &node.value {
        TreeNode::Leaf { .. } => {
            result.insert(id, rect);
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
                    assign(nodes, *child, child_rect, gaps, result);
                    cursor += extent + gaps;
                }
            }
            Layout::Tabbed | Layout::Stacked => {
                for child in children {
                    assign(nodes, *child, rect, gaps, result);
                }
            }
        },
    }
}

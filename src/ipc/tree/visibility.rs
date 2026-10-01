use super::*;

pub(super) fn newest_focus_timestamp(
    node: &Node,
    focus_timestamps: &std::collections::HashMap<i64, std::time::Duration>,
) -> Option<std::time::Duration> {
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .filter_map(|child| newest_focus_timestamp(child, focus_timestamps))
        .chain(focus_timestamps.get(&node.id).copied())
        .max()
}

pub(super) fn set_tabbed_percentages(layout: NodeLayout, children: &mut [Node], parent_rect: Rect) {
    // Percent is computed from sway's pending container boxes, before the
    // serializer exposes the content rectangles below nested titlebars.
    let titlebar_height = children
        .iter()
        .flat_map(|child| child.nodes.iter())
        .map(|child| child.deco_rect.height)
        .chain(children.iter().map(|child| child.deco_rect.height))
        .max()
        .unwrap_or_default();
    let offset = match layout {
        NodeLayout::Tabbed => titlebar_height,
        NodeLayout::Stacked => i32::try_from(children.len())
            .map_or(i32::MAX, |count| titlebar_height.saturating_mul(count)),
        _ => 0,
    };
    // A tiny output scale makes logical rectangles large enough that an i32
    // area overflows, so compute areas in f64.
    let area = |rect: Rect| f64::from(rect.width) * f64::from(rect.height);
    let parent_area = area(parent_rect);
    for child in children {
        let mut pending_rect = parent_rect;
        if offset > 0 && !child.nodes.is_empty() {
            pending_rect.y = pending_rect.y.saturating_add(offset);
            pending_rect.height = pending_rect.height.saturating_sub(offset).max(0);
            child.percent = Some(if parent_area == 0. {
                1.
            } else {
                area(pending_rect) / parent_area
            });
        }
        set_tabbed_percentages(child.layout, &mut child.nodes, pending_rect);
    }
}

pub(super) fn clear_focused(node: &mut Node) {
    node.focused = false;
    for child in node.nodes.iter_mut().chain(&mut node.floating_nodes) {
        clear_focused(child);
    }
}

/// Marks every window in a workspace subtree as shown or hidden.
///
/// Sway reports `visible` per window: a window on a workspace that is not its
/// output's active one is not visible (`view_is_visible`,
/// `sway/sway/tree/view.c:1157-1203`, and the
/// captured `sway-ipc/fixtures/two_workspaces.tree.json` in the pinned oracle shows
/// `visible: false` for the window on the background workspace). Waybar's
/// `hasFlag` recurses into child nodes, so a window wrongly claiming to be
/// visible marks its whole workspace button visible.
///
/// A window on an inactive tab is not visible either: sway walks up from the
/// view and, at every tabbed or stacked ancestor, requires the seat's active
/// tiling child to be on its path (`view_is_visible`, `sway/sway/tree/view.c:1180-1193`).
/// The active tiling child is the first tiling entry of that container's
/// `focus` list, which the reply already carries.
pub(super) fn set_windows_visible(node: &mut Node, visible: bool) {
    if let swayward_ipc::NodeProperties::View(properties) = &mut node.properties {
        properties.visible = visible;
    }
    set_child_windows_visible(node.layout, &node.focus, &mut node.nodes, visible);
    for child in &mut node.floating_nodes {
        set_windows_visible(child, visible);
    }
}

pub(super) fn apply_fullscreen_state(nodes: &mut [Node], workspace_visible: bool) -> bool {
    fn apply(nodes: &mut [Node], workspace_visible: bool, set_full_percent: bool) -> bool {
        let Some(fullscreen) = nodes.iter().position(contains_fullscreen) else {
            return false;
        };
        for (index, node) in nodes.iter_mut().enumerate() {
            if index == fullscreen {
                let pending_tab_wrapper = node.fullscreen_mode == 0
                    && node.percent == Some(0.)
                    && matches!(node.layout, NodeLayout::Tabbed | NodeLayout::Stacked);
                if set_full_percent && node.fullscreen_mode != 0 && !pending_tab_wrapper {
                    node.percent = Some(1.);
                }
                if node.fullscreen_mode == 0 {
                    if pending_tab_wrapper {
                        for child in &mut node.nodes {
                            child.percent = None;
                        }
                    }
                    apply(&mut node.nodes, workspace_visible, false);
                } else {
                    set_windows_visible(node, workspace_visible);
                }
            } else {
                set_windows_visible(node, false);
            }
        }
        true
    }

    apply(nodes, workspace_visible, true)
}

pub(super) fn contains_fullscreen(node: &Node) -> bool {
    node.fullscreen_mode != 0 || node.nodes.iter().any(contains_fullscreen)
}

pub(super) fn set_child_windows_visible(
    layout: NodeLayout,
    focus: &[i64],
    children: &mut [Node],
    visible: bool,
) {
    let active_tab = matches!(layout, NodeLayout::Tabbed | NodeLayout::Stacked)
        .then(|| {
            focus
                .iter()
                .copied()
                .find(|id| children.iter().any(|child| child.id == *id))
                .or_else(|| children.first().map(|child| child.id))
        })
        .flatten();
    for child in children {
        let shown = active_tab.is_none_or(|active| active == child.id);
        set_windows_visible(child, visible && shown);
    }
}

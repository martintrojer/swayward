use std::cmp::Reverse;

use smithay::desktop::{Space, Window};
use smithay::utils::{Logical, Rectangle};
use swayward_ipc::{
    IdleInhibitors, Node, NodeBorder, NodeLayout, NodeProperties, NodeType, Output, OutputFeatures,
    OutputMode, OutputProperties, Rect, ViewProperties, Workspace,
};

use crate::layout::tiling_tree::{IpcNode, Layout as TreeLayout, NodeId};
use crate::layout::workspace::WorkspaceId;
use crate::layout::{Layout, LayoutElement as _};
use crate::swayward::{ContainerMarks, WindowMarks};
use crate::utils::{with_toplevel_role, ResizeEdge};
use crate::window::mapped::MappedId;
use crate::window::Mapped;

mod geometry;
mod ids;
mod node;
mod outputs;
mod visibility;
mod workspaces;

use geometry::*;
pub(crate) use ids::{container_id, window_id, window_id_from_raw, workspace_id};
use ids::{ipc_layout, orientation, output_id};
pub(crate) use node::describe_tiling;
use node::*;
use outputs::describe_output_node;
pub use outputs::{describe_outputs, describe_outputs_with_power};
#[cfg(test)]
pub(crate) use outputs::{sway_subpixel_hinting, sway_transform};
use visibility::*;
use workspaces::describe_workspace_node;
pub use workspaces::describe_workspaces;
pub(crate) use workspaces::describe_workspaces_with_marks;

const ROOT_ID: i64 = 1;
const SCRATCH_OUTPUT_ID: i64 = i32::MAX as i64;
const SCRATCH_WORKSPACE_ID: i64 = SCRATCH_OUTPUT_ID - 1;
const ID_NAMESPACE_SIZE: i64 = 100_000_000;
const OUTPUT_ID_BASE: i64 = ID_NAMESPACE_SIZE;
const WORKSPACE_ID_BASE: i64 = 2 * ID_NAMESPACE_SIZE;
const CONTAINER_ID_BASE: i64 = 3 * ID_NAMESPACE_SIZE;
const WINDOW_ID_BASE: i64 = 4 * ID_NAMESPACE_SIZE;

pub fn describe_tree(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    marks: &WindowMarks,
    container_marks: &ContainerMarks,
) -> Node {
    describe_tree_with_power(
        layout,
        global_space,
        marks,
        container_marks,
        &std::collections::HashMap::new(),
    )
}

/// [`describe_tree`] with runtime output power, which only the GET_TREE reply
/// observes. Event payloads never carry output nodes.
pub fn describe_tree_with_power(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    marks: &WindowMarks,
    container_marks: &ContainerMarks,
    output_power: &std::collections::HashMap<String, bool>,
) -> Node {
    let outputs: Vec<_> = layout.monitors().collect();
    let root_rect = outputs
        .iter()
        .filter_map(|monitor| global_space.output_geometry(monitor.output()))
        .reduce(|a, b| a.merge(b))
        .map(rect_from_rectangle)
        .unwrap_or_default();
    let mut nodes = vec![scratch_output(layout, root_rect, marks, container_marks)];
    nodes.extend(outputs.iter().map(|monitor| {
        describe_output_node(
            layout,
            global_space,
            monitor,
            output_power,
            root_rect,
            marks,
            container_marks,
        )
    }));
    let active_output = layout.active_monitor_ref().map(|monitor| monitor.output());
    let mut focused_outputs = outputs.clone();
    focused_outputs.sort_by_key(|monitor| {
        (
            monitor.output() != active_output.unwrap_or(monitor.output()),
            Reverse(
                monitor
                    .windows()
                    .filter_map(|window| window.focus_timestamp())
                    .max(),
            ),
        )
    });
    let focus = focused_outputs
        .into_iter()
        .map(|monitor| output_id(monitor.output_name()))
        .collect();
    common_node(CommonNodeContext {
        id: ROOT_ID,
        node_type: NodeType::Root,
        layout: NodeLayout::SplitH,
        orientation: "horizontal",
        name: Some("root"),
        rect: root_rect,
        nodes,
        floating_nodes: vec![],
        focus,
        focused: false,
        properties: NodeProperties::None {},
    })
}

fn scratch_output(
    layout: &Layout<Mapped>,
    rect: Rect,
    marks: &WindowMarks,
    container_marks: &ContainerMarks,
) -> Node {
    let mut floating_nodes = layout
        .scratchpad_trees()
        .filter_map(|(tree, sticky)| {
            let mut node = describe_tiling(
                tree,
                &|window| {
                    layout
                        .windows()
                        .find(|(_, mapped)| mapped.window == *window)
                        .map(|(_, mapped)| mapped)
                },
                Rect::default(),
                marks,
                // A hidden group keeps its marks, as sway's GET_TREE and
                // GET_MARKS walk hidden scratchpad containers
                // (`sway/sway/tree/root.c:250-257`).
                container_marks,
            )?;
            node.node_type = NodeType::FloatingCon;
            node.floating = Some("user_on".into());
            node.scratchpad_state = Some("fresh".into());
            node.sticky = sticky;
            set_windows_visible(&mut node, false);
            Some(node)
        })
        .collect::<Vec<_>>();
    fn contains_id(node: &Node, id: i64) -> bool {
        node.id == id
            || node
                .nodes
                .iter()
                .chain(&node.floating_nodes)
                .any(|child| contains_id(child, id))
    }
    let tree_window_ids = layout
        .scratchpad_windows()
        .filter(|mapped| {
            floating_nodes
                .iter()
                .any(|node| contains_id(node, window_id(mapped.id())))
        })
        .map(|mapped| mapped.id())
        .collect::<Vec<_>>();
    floating_nodes.extend(
        layout
            .scratchpad_windows()
            .filter(|mapped| !tree_window_ids.contains(&mapped.id()))
            .map(|mapped| {
                let (border, border_width) = layout
                    .window_border(&mapped.window)
                    .map_or((NodeBorder::Normal, 2), |border| {
                        (ipc_border(border.0), i32::from(border.1))
                    });
                let mut node = describe_window(WindowNodeContext {
                    mapped,
                    rect: Rect::default(),
                    border,
                    border_width,
                    // A hidden window has no content box: zero-sized, inset
                    // by the default 2px side border.
                    window_rect: Rect {
                        x: 2,
                        ..Rect::default()
                    },
                    node_type: NodeType::FloatingCon,
                    floating: "user_on",
                    parent: None,
                    marks,
                    in_scratchpad: true,
                    visible: false,
                });
                node.sticky = layout
                    .scratchpad_tiles()
                    .find_map(|(window, sticky)| (window.id() == mapped.id()).then_some(sticky))
                    .unwrap_or(false);
                node
            }),
    );
    let focus = floating_nodes.iter().rev().map(|node| node.id).collect();
    let mut workspace = common_node(CommonNodeContext {
        id: SCRATCH_WORKSPACE_ID,
        node_type: NodeType::Workspace,
        layout: NodeLayout::SplitH,
        orientation: "horizontal",
        name: Some("__i3_scratch"),
        rect,
        nodes: vec![],
        floating_nodes,
        focus,
        focused: false,
        properties: NodeProperties::None {},
    });
    // Sway reports 1 on the scratch workspace (`sway/sway/ipc-json.c:473`).
    workspace.fullscreen_mode = 1;
    common_node(CommonNodeContext {
        id: SCRATCH_OUTPUT_ID,
        node_type: NodeType::Output,
        layout: NodeLayout::Output,
        orientation: "horizontal",
        name: Some("__i3"),
        rect,
        nodes: vec![workspace],
        floating_nodes: vec![],
        focus: vec![SCRATCH_WORKSPACE_ID],
        focused: false,
        properties: NodeProperties::None {},
    })
}

#[cfg(test)]
mod tests;

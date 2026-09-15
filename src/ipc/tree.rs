use std::cmp::Reverse;

use smithay::desktop::{Space, Window};
use smithay::utils::{Logical, Rectangle};
use swayward_ipc::{
    IdleInhibitors, Node, NodeBorder, NodeLayout, NodeProperties, NodeType, Output, OutputMode,
    OutputProperties, Rect, ViewProperties, Workspace,
};

use crate::layout::tiling_tree::{IpcNode, Layout as TreeLayout, NodeId};
use crate::layout::workspace::WorkspaceId;
use crate::layout::{Layout, LayoutElement as _};
use crate::utils::with_toplevel_role;
use crate::window::mapped::MappedId;
use crate::window::Mapped;

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
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
) -> Node {
    let outputs: Vec<_> = layout.monitors().collect();
    let root_rect = outputs
        .iter()
        .filter_map(|monitor| global_space.output_geometry(monitor.output()))
        .reduce(|a, b| a.merge(b))
        .map(rect_from_rectangle)
        .unwrap_or_default();
    let mut nodes = vec![scratch_output(layout, root_rect, marks)];
    nodes.extend(outputs.iter().map(|monitor| {
        describe_output_node(layout, global_space, monitor, marks, container_marks)
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
    common_node(
        ROOT_ID,
        NodeType::Root,
        NodeLayout::SplitH,
        "horizontal",
        Some("root"),
        root_rect,
        nodes,
        vec![],
        focus,
        false,
        NodeProperties::None {},
    )
}

pub fn describe_workspaces(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
) -> Vec<Workspace> {
    layout
        .workspaces()
        .filter_map(|(monitor, index, workspace)| {
            let monitor = monitor?;
            if !workspace.has_windows_or_name()
                && !workspace.has_sway_identity()
                && monitor.active_workspace_ref().id() != workspace.id()
            {
                return None;
            }
            let name = workspace
                .sway_name()
                .unwrap_or_else(|| (index + 1).to_string());
            let rect = output_rect(global_space, monitor.output());
            let visible = monitor.active_workspace_idx() == index;
            let focused = visible
                && layout
                    .active_monitor_ref()
                    .is_some_and(|active| active.output() == monitor.output());
            let focus = workspace
                .active_window()
                .map(|window| window_id(window.id()))
                .into_iter()
                .collect();
            let tiling = describe_tiling(
                workspace.ipc_tiling_tree(),
                &|window| workspace.windows().find(|mapped| mapped.window == *window),
                rect,
                &Default::default(),
                &Default::default(),
                workspace.id(),
            );
            let (layout, orientation, representation) =
                tiling.map_or((NodeLayout::SplitV, "vertical".into(), None), |node| {
                    let representation = (!node.nodes.is_empty())
                        .then(|| tree_representation(node.layout, &node.nodes));
                    (node.layout, node.orientation, representation)
                });
            Some(Workspace {
                border: NodeBorder::None,
                current_border_width: 0,
                deco_rect: Rect::default(),
                floating: None,
                floating_nodes: vec![],
                focus,
                focused,
                fullscreen_mode: i32::from(workspace.is_active_pending_fullscreen()),
                geometry: Rect::default(),
                id: workspace_id(workspace.id().get()),
                layout,
                marks: vec![],
                name,
                nodes: vec![],
                num: workspace.number().unwrap_or_else(|| {
                    workspace.name().map_or_else(
                        || i32::try_from(index + 1).unwrap_or(-1),
                        |name| crate::layout::sway_workspace_num(name),
                    )
                }),
                orientation,
                output: monitor.output_name().clone(),
                percent: None,
                rect,
                representation,
                scratchpad_state: None,
                sticky: false,
                node_type: NodeType::Workspace,
                urgent: workspace.is_urgent(),
                visible,
                window: None,
                window_rect: Rect::default(),
            })
        })
        .collect()
}

pub fn describe_outputs(layout: &Layout<Mapped>, global_space: &Space<Window>) -> Vec<Output> {
    layout
        .monitors()
        .map(|monitor| {
            let output = monitor.output();
            let mode = output.current_mode();
            let physical = output.physical_properties();
            let current_mode = mode.map_or(
                OutputMode {
                    width: 0,
                    height: 0,
                    refresh: 0,
                },
                |mode| OutputMode {
                    width: mode.size.w,
                    height: mode.size.h,
                    refresh: mode.refresh,
                },
            );
            Output {
                active: true,
                adaptive_sync_status: "disabled".into(),
                allow_tearing: false,
                border: NodeBorder::None,
                current_border_width: 0,
                current_mode,
                current_workspace: Some(
                    monitor
                        .active_workspace_ref()
                        .sway_name()
                        .unwrap_or_else(|| (monitor.active_workspace_idx() + 1).to_string()),
                ),
                deco_rect: Rect::default(),
                dpms: true,
                floating: None,
                floating_nodes: vec![],
                focus: vec![workspace_id(monitor.active_workspace_ref().id().get())],
                focused: layout
                    .active_monitor_ref()
                    .is_some_and(|active| active.output() == output),
                fullscreen_mode: 0,
                geometry: Rect::default(),
                id: output_id(monitor.output_name()),
                layout: NodeLayout::Output,
                make: physical.make.clone(),
                marks: vec![],
                max_render_time: 0,
                model: physical.model.clone(),
                modes: mode
                    .into_iter()
                    .map(|mode| OutputMode {
                        width: mode.size.w,
                        height: mode.size.h,
                        refresh: mode.refresh,
                    })
                    .collect(),
                name: monitor.output_name().clone(),
                nodes: vec![],
                non_desktop: false,
                orientation: "none".into(),
                percent: Some(1.),
                power: true,
                primary: false,
                rect: output_rect(global_space, output),
                scale: output.current_scale().fractional_scale(),
                scale_filter: "nearest".into(),
                scratchpad_state: None,
                serial: physical.serial_number.clone(),
                sticky: false,
                subpixel_hinting: "unknown".into(),
                transform: "normal".into(),
                node_type: NodeType::Output,
                urgent: false,
                window: None,
                window_rect: Rect::default(),
            }
        })
        .collect()
}

fn describe_output_node(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    monitor: &crate::layout::monitor::Monitor<Mapped>,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
) -> Node {
    let rect = output_rect(global_space, monitor.output());
    let workspaces = layout
        .workspaces()
        .filter(|(candidate, _, workspace)| {
            (workspace.has_windows_or_name()
                || monitor.active_workspace_ref().id() == workspace.id())
                && candidate.is_some_and(|candidate| candidate.output() == monitor.output())
        })
        .map(|(_, index, workspace)| {
            describe_workspace_node(
                layout,
                workspace,
                monitor.output_name(),
                index,
                rect,
                marks,
                container_marks,
            )
        })
        .collect::<Vec<_>>();
    let active_workspace_id = workspace_id(monitor.active_workspace_ref().id().get());
    let focus = workspaces
        .iter()
        .find(|workspace| workspace.id == active_workspace_id)
        .map(|workspace| workspace.id)
        .into_iter()
        .collect();
    let output = describe_outputs(layout, global_space)
        .into_iter()
        .find(|output| output.name == *monitor.output_name())
        .unwrap();
    let mut node = common_node(
        output.id,
        NodeType::Output,
        NodeLayout::Output,
        "none",
        Some(&output.name),
        rect,
        workspaces,
        vec![],
        focus,
        false,
        NodeProperties::Output(OutputProperties {
            active: output.active,
            adaptive_sync_status: output.adaptive_sync_status,
            allow_tearing: output.allow_tearing,
            current_mode: output.current_mode,
            current_workspace: output.current_workspace,
            dpms: output.dpms,
            make: output.make,
            max_render_time: output.max_render_time,
            model: output.model,
            modes: output.modes,
            non_desktop: output.non_desktop,
            power: output.power,
            primary: output.primary,
            scale: output.scale,
            scale_filter: output.scale_filter,
            serial: output.serial,
            transform: output.transform,
        }),
    );
    node.percent = Some(1.);
    node
}

fn describe_workspace_node(
    compositor_layout: &Layout<Mapped>,
    workspace: &crate::layout::workspace::Workspace<Mapped>,
    output: &str,
    index: usize,
    rect: Rect,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
) -> Node {
    let mut tiled = describe_tiling(
        workspace.ipc_tiling_tree(),
        &|window| workspace.windows().find(|mapped| mapped.window == *window),
        rect,
        marks,
        container_marks,
        workspace.id(),
    )
    .unwrap_or_else(|| empty_tiling_node(rect));
    let workspace_focused = compositor_layout
        .active_monitor_ref()
        .is_some_and(|monitor| {
            monitor.output_name() == output && monitor.active_workspace_ref().id() == workspace.id()
        });
    if !workspace_focused || workspace.floating_is_active() {
        clear_focused(&mut tiled);
    }
    let Node {
        layout,
        orientation,
        nodes,
        focus,
        focused,
        ..
    } = &mut tiled;
    let (layout, orientation, nodes, mut focus, focused) = (
        *layout,
        orientation.clone(),
        std::mem::take(nodes),
        std::mem::take(focus),
        *focused || workspace_focused && workspace.active_window().is_none(),
    );
    let active_window = workspace_focused
        .then(|| workspace.active_window().map(|window| window.id()))
        .flatten();
    let mut floating_nodes = workspace
        .tiles_with_ipc_layouts()
        .filter(|(tile, _)| workspace.is_floating_for_ipc(&tile.window().window))
        .map(|(tile, layout)| {
            let (x, y) = layout.tile_pos_in_workspace_view.unwrap_or_default();
            let mut node = describe_window(
                tile.window(),
                offset_rect(
                    Rectangle::new(
                        (x, y).into(),
                        (layout.tile_size.0, layout.tile_size.1).into(),
                    ),
                    rect,
                ),
                NodeType::FloatingCon,
                "user_on",
                Some(rect),
                marks,
                compositor_layout.is_scratchpad_window(&tile.window().window),
                true,
            );
            node.focused =
                workspace.floating_is_active() && active_window == Some(tile.window().id());
            let border = tile.sway_border();
            node.border = ipc_border(border.0);
            node.current_border_width = i32::from(border.1);
            node.sticky = workspace.is_window_sticky(&tile.window().window);
            node
        })
        .collect::<Vec<_>>();
    floating_nodes.reverse();
    let floating_focus = floating_nodes.iter().rev().map(|node| node.id);
    if workspace.floating_is_active() {
        focus.splice(0..0, floating_focus);
    } else {
        focus.extend(floating_focus);
    }
    let representation = (!nodes.is_empty()).then(|| tree_representation(layout, &nodes));
    common_node(
        workspace_id(workspace.id().get()),
        NodeType::Workspace,
        layout,
        &orientation,
        Some(
            &workspace
                .sway_name()
                .unwrap_or_else(|| (index + 1).to_string()),
        ),
        rect,
        nodes,
        floating_nodes,
        focus,
        focused,
        NodeProperties::Workspace(swayward_ipc::WorkspaceProperties {
            num: workspace.number().unwrap_or_else(|| {
                workspace.name().map_or_else(
                    || i32::try_from(index + 1).unwrap_or(-1),
                    |name| crate::layout::sway_workspace_num(name),
                )
            }),
            output: output.into(),
            representation,
        }),
    )
}

fn clear_focused(node: &mut Node) {
    node.focused = false;
    for child in node.nodes.iter_mut().chain(&mut node.floating_nodes) {
        clear_focused(child);
    }
}

pub(crate) fn describe_tiling<'a, I>(
    node: IpcNode<I>,
    find_window: &impl Fn(&I) -> Option<&'a Mapped>,
    workspace_rect: Rect,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
    workspace_id: WorkspaceId,
) -> Option<Node> {
    match node {
        IpcNode::Split {
            id,
            layout,
            percent,
            rect,
            focus,
            focused,
            fullscreen_mode,
            children,
        } => {
            let children = children
                .into_iter()
                .filter_map(|child| {
                    let id = match &child {
                        IpcNode::Split { id, .. } | IpcNode::Leaf { id, .. } => *id,
                    };
                    describe_tiling(
                        child,
                        find_window,
                        workspace_rect,
                        marks,
                        container_marks,
                        workspace_id,
                    )
                    .map(|node| (id, node))
                })
                .collect::<Vec<_>>();
            let focus = focus
                .iter()
                .filter_map(|id| {
                    children
                        .iter()
                        .find_map(|(child_id, node)| (child_id == id).then_some(node.id))
                })
                .collect();
            let children = children.into_iter().map(|(_, node)| node).collect();
            let mut node = common_node(
                container_id(id),
                NodeType::Con,
                ipc_layout(layout),
                orientation(layout),
                None,
                offset_rect(rect, workspace_rect),
                children,
                vec![],
                focus,
                focused,
                NodeProperties::None {},
            );
            node.percent = percent;
            node.fullscreen_mode = fullscreen_mode;
            node.marks = container_marks
                .get(&(workspace_id, id))
                .cloned()
                .unwrap_or_default();
            Some(node)
        }
        IpcNode::Leaf {
            window,
            percent,
            focused,
            rect,
            deco_rect,
            border,
            ..
        } => {
            let Some(mapped) = find_window(&window) else {
                warn!("omitting stale tree leaf from IPC output");
                return None;
            };
            let mut node = describe_window(
                mapped,
                offset_rect(rect, workspace_rect),
                NodeType::Con,
                "auto_off",
                None,
                marks,
                false,
                true,
            );
            node.border = ipc_border(border.0);
            node.current_border_width = i32::from(border.1);
            node.percent = percent;
            node.focused = focused;
            let has_titlebar = deco_rect.is_some();
            node.deco_rect = deco_rect.map_or_else(Rect::default, |rect| {
                rect_from(rect.loc.x, rect.loc.y, rect.size.w, rect.size.h)
            });
            let border_width = match (node.border, has_titlebar) {
                (NodeBorder::Normal | NodeBorder::Pixel, true) | (NodeBorder::Pixel, false) => {
                    node.current_border_width
                }
                _ => 0,
            };
            let top = if has_titlebar { 0 } else { border_width };
            node.window_rect = Rect {
                x: border_width,
                y: top,
                width: (node.rect.width - border_width * 2).max(0),
                height: (node.rect.height - border_width - top).max(0),
            };
            Some(node)
        }
    }
}

fn empty_tiling_node(rect: Rect) -> Node {
    common_node(
        0,
        NodeType::Con,
        NodeLayout::SplitH,
        "horizontal",
        None,
        rect,
        vec![],
        vec![],
        vec![],
        false,
        NodeProperties::None {},
    )
}

fn ipc_border(style: swayward_ipc::command::BorderStyle) -> NodeBorder {
    match style {
        swayward_ipc::command::BorderStyle::Normal => NodeBorder::Normal,
        swayward_ipc::command::BorderStyle::None => NodeBorder::None,
        swayward_ipc::command::BorderStyle::Pixel => NodeBorder::Pixel,
        swayward_ipc::command::BorderStyle::Csd => NodeBorder::Csd,
        swayward_ipc::command::BorderStyle::Toggle => unreachable!(),
    }
}

// Serialising a sway tree node genuinely needs this much context. Bundling it
// into a struct would only move the argument list.
#[allow(clippy::too_many_arguments)]
fn describe_window(
    mapped: &Mapped,
    rect: Rect,
    node_type: NodeType,
    floating: &str,
    parent: Option<Rect>,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    in_scratchpad: bool,
    visible: bool,
) -> Node {
    let properties = with_toplevel_role(mapped.toplevel(), |role| ViewProperties {
        allow_tearing: false,
        app_id: role.app_id.clone(),
        foreign_toplevel_identifier: Some(mapped.id().to_protocol_identifier()),
        idle_inhibitors: IdleInhibitors {
            application: "none".into(),
            user: "none".into(),
        },
        inhibit_idle: false,
        max_render_time: 0,
        pid: mapped
            .credentials()
            .map(|credentials| i64::from(credentials.pid)),
        sandbox_app_id: None,
        sandbox_engine: None,
        sandbox_instance_id: None,
        shell: Some("xdg_shell".into()),
        visible,
    });
    // Sway serializes the container's formatted title here, not the client's raw title.
    let title = mapped.formatted_title();
    let percent = parent.and_then(|parent| {
        (parent.width != 0 && parent.height != 0).then(|| {
            f64::from(rect.width) / f64::from(parent.width) * f64::from(rect.height)
                / f64::from(parent.height)
        })
    });
    let mut node = common_node(
        window_id(mapped.id()),
        node_type,
        NodeLayout::None,
        "none",
        Some(&title),
        rect,
        vec![],
        vec![],
        vec![],
        mapped.is_focused(),
        NodeProperties::View(properties),
    );
    node.border = NodeBorder::Normal;
    node.current_border_width = 2;
    node.floating = Some(floating.into());
    node.percent = percent;
    node.scratchpad_state = Some(if in_scratchpad { "fresh" } else { "none" }.into());
    node.fullscreen_mode = i32::from(mapped.pending_sizing_mode().is_fullscreen());
    node.urgent = mapped.is_urgent();
    node.geometry = rect_from(0., 0., mapped.size().w.into(), mapped.size().h.into());
    node.marks = marks.get(&mapped.id()).cloned().unwrap_or_default();
    node.window_rect = Rect {
        x: 2,
        y: 0,
        width: (rect.width - 4).max(0),
        height: (rect.height - 2).max(0),
    };
    node
}

#[allow(clippy::too_many_arguments)]
fn common_node(
    id: i64,
    node_type: NodeType,
    layout: NodeLayout,
    orientation: &str,
    name: Option<&str>,
    rect: Rect,
    nodes: Vec<Node>,
    floating_nodes: Vec<Node>,
    focus: Vec<i64>,
    focused: bool,
    properties: NodeProperties,
) -> Node {
    Node {
        border: NodeBorder::None,
        current_border_width: 0,
        deco_rect: Rect::default(),
        floating: None,
        floating_nodes,
        focus,
        focused,
        fullscreen_mode: 0,
        geometry: Rect::default(),
        id,
        layout,
        marks: vec![],
        name: name.map(Into::into),
        nodes,
        orientation: orientation.into(),
        percent: None,
        rect,
        scratchpad_state: None,
        sticky: false,
        node_type,
        urgent: false,
        window: None,
        window_rect: Rect::default(),
        properties,
    }
}

fn tree_representation(layout: NodeLayout, children: &[Node]) -> String {
    let prefix = match layout {
        NodeLayout::SplitH => 'V',
        NodeLayout::SplitV => 'H',
        NodeLayout::Tabbed => 'T',
        NodeLayout::Stacked => 'S',
        _ => 'D',
    };
    let children = children
        .iter()
        .map(|child| {
            if child.nodes.is_empty() {
                child.name.as_deref().unwrap_or("(null)").to_owned()
            } else {
                tree_representation(child.layout, &child.nodes)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{prefix}[{children}]")
}

fn scratch_output(
    layout: &Layout<Mapped>,
    rect: Rect,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
) -> Node {
    let floating_nodes = layout
        .scratchpad_windows()
        .map(|mapped| {
            let mut node = describe_window(
                mapped,
                Rect::default(),
                NodeType::FloatingCon,
                "user_on",
                None,
                marks,
                true,
                false,
            );
            if let Some(border) = layout.window_border(&mapped.window) {
                node.border = ipc_border(border.0);
                node.current_border_width = i32::from(border.1);
            }
            node
        })
        .collect();
    let workspace = common_node(
        SCRATCH_WORKSPACE_ID,
        NodeType::Workspace,
        NodeLayout::SplitH,
        "horizontal",
        Some("__i3_scratch"),
        rect,
        vec![],
        floating_nodes,
        vec![],
        false,
        NodeProperties::None {},
    );
    common_node(
        SCRATCH_OUTPUT_ID,
        NodeType::Output,
        NodeLayout::Output,
        "horizontal",
        Some("__i3"),
        rect,
        vec![workspace],
        vec![],
        vec![SCRATCH_WORKSPACE_ID],
        false,
        NodeProperties::None {},
    )
}

fn ipc_layout(layout: TreeLayout) -> NodeLayout {
    match layout {
        TreeLayout::SplitH => NodeLayout::SplitH,
        TreeLayout::SplitV => NodeLayout::SplitV,
        TreeLayout::Tabbed => NodeLayout::Tabbed,
        TreeLayout::Stacked => NodeLayout::Stacked,
    }
}
fn orientation(layout: TreeLayout) -> &'static str {
    match layout {
        TreeLayout::SplitH => "horizontal",
        TreeLayout::SplitV => "vertical",
        TreeLayout::Tabbed | TreeLayout::Stacked => "none",
    }
}
fn output_id(name: &str) -> i64 {
    OUTPUT_ID_BASE + stable_hash(name)
}
pub(crate) fn workspace_id(id: u64) -> i64 {
    WORKSPACE_ID_BASE + i64::try_from(id % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
pub(crate) fn container_id(id: NodeId) -> i64 {
    CONTAINER_ID_BASE + i64::try_from(id.0 % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
pub(crate) fn window_id(id: MappedId) -> i64 {
    window_id_from_raw(id.get())
}
pub(crate) fn window_id_from_raw(id: u64) -> i64 {
    WINDOW_ID_BASE + i64::try_from(id % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
fn stable_hash(value: &str) -> i64 {
    value.bytes().fold(0i64, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(i64::from(byte))
    }) % ID_NAMESPACE_SIZE
}
fn rect_from(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x: x.round() as i32,
        y: y.round() as i32,
        width: width.round() as i32,
        height: height.round() as i32,
    }
}
fn offset_rect(rect: Rectangle<f64, Logical>, output: Rect) -> Rect {
    rect_from(
        rect.loc.x + f64::from(output.x),
        rect.loc.y + f64::from(output.y),
        rect.size.w,
        rect.size.h,
    )
}
fn rect_from_rectangle(rect: Rectangle<i32, Logical>) -> Rect {
    Rect {
        x: rect.loc.x,
        y: rect.loc.y,
        width: rect.size.w,
        height: rect.size.h,
    }
}

fn output_rect(global_space: &Space<Window>, output: &smithay::output::Output) -> Rect {
    global_space
        .output_geometry(output)
        .map(rect_from_rectangle)
        .unwrap_or_default()
}

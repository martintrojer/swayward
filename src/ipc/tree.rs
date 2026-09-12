use smithay::desktop::{Space, Window};
use smithay::utils::{Logical, Rectangle};
use swayward_ipc::{
    IdleInhibitors, Node, NodeBorder, NodeLayout, NodeProperties, NodeType, Output, OutputMode,
    OutputProperties, Rect, ViewProperties, Workspace,
};

use crate::layout::tiling_tree::{IpcNode, Layout as TreeLayout, NodeId};
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

pub fn describe_tree(layout: &Layout<Mapped>, global_space: &Space<Window>) -> Node {
    let outputs: Vec<_> = layout.monitors().collect();
    let root_rect = outputs
        .iter()
        .filter_map(|monitor| global_space.output_geometry(monitor.output()))
        .reduce(|a, b| a.merge(b))
        .map(rect_from_rectangle)
        .unwrap_or_default();
    let mut nodes = vec![scratch_output(root_rect)];
    nodes.extend(
        outputs
            .iter()
            .map(|monitor| describe_output_node(layout, global_space, monitor)),
    );
    let focus = outputs
        .iter()
        .find(|monitor| monitor.active_workspace_ref().active_window().is_some())
        .or_else(|| outputs.first())
        .map(|monitor| output_id(monitor.output_name()))
        .into_iter()
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
                && monitor.active_workspace_ref().id() != workspace.id()
            {
                return None;
            }
            let name = workspace
                .name()
                .cloned()
                .unwrap_or_else(|| (index + 1).to_string());
            let rect = output_rect(global_space, monitor.output());
            let focused = monitor.active_workspace_idx() == index;
            let focus = workspace
                .active_window()
                .map(|window| window_id(window.id()))
                .into_iter()
                .collect();
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
                layout: NodeLayout::SplitH,
                marks: vec![],
                name,
                nodes: vec![],
                num: i32::try_from(index + 1).unwrap_or(-1),
                orientation: "horizontal".into(),
                output: monitor.output_name().clone(),
                percent: None,
                rect,
                representation: None,
                scratchpad_state: None,
                sticky: false,
                node_type: NodeType::Workspace,
                urgent: workspace.is_urgent(),
                visible: focused,
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
                        .name()
                        .cloned()
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
            describe_workspace_node(workspace, monitor.output_name(), index, rect)
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
    common_node(
        output.id,
        NodeType::Output,
        NodeLayout::Output,
        "none",
        Some(&output.name),
        rect,
        workspaces,
        vec![],
        focus,
        output.focused,
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
    )
}

fn describe_workspace_node(
    workspace: &crate::layout::workspace::Workspace<Mapped>,
    output: &str,
    index: usize,
    rect: Rect,
) -> Node {
    let mut tiled = describe_tiling(
        workspace.ipc_tiling_tree(),
        &|window| workspace.windows().find(|mapped| mapped.window == *window),
        rect,
    )
    .unwrap_or_else(|| empty_tiling_node(rect));
    let Node {
        layout,
        orientation,
        nodes,
        focus,
        ..
    } = &mut tiled;
    let (layout, orientation, nodes, focus) = (
        *layout,
        orientation.clone(),
        std::mem::take(nodes),
        std::mem::take(focus),
    );
    let focused = workspace
        .active_window()
        .is_some_and(|window| window.is_focused());
    let floating_nodes = workspace
        .tiles_with_render_positions()
        .filter(|(tile, _, _)| workspace.is_floating(&tile.window().window))
        .map(|(tile, pos, _)| {
            describe_window(
                tile.window(),
                rect_from(pos.x, pos.y, tile.tile_size().w, tile.tile_size().h),
                NodeType::FloatingCon,
                "user_on",
                None,
            )
        })
        .collect();
    common_node(
        workspace_id(workspace.id().get()),
        NodeType::Workspace,
        layout,
        &orientation,
        workspace
            .name()
            .map(String::as_str)
            .or(Some(&(index + 1).to_string())),
        rect,
        nodes,
        floating_nodes,
        focus,
        focused,
        NodeProperties::Workspace(swayward_ipc::WorkspaceProperties {
            num: i32::try_from(index + 1).unwrap_or(-1),
            output: output.into(),
            representation: None,
        }),
    )
}

pub(crate) fn describe_tiling<'a, I>(
    node: IpcNode<I>,
    find_window: &impl Fn(&I) -> Option<&'a Mapped>,
    workspace_rect: Rect,
) -> Option<Node> {
    match node {
        IpcNode::Split {
            id,
            layout,
            percent,
            children,
        } => {
            let children: Vec<_> = children
                .into_iter()
                .filter_map(|child| describe_tiling(child, find_window, workspace_rect))
                .collect();
            let focus = children
                .iter()
                .filter(|child| child.focused)
                .map(|child| child.id)
                .chain(children.iter().map(|child| child.id))
                .take(1)
                .collect();
            let mut node = common_node(
                container_id(id),
                NodeType::Con,
                ipc_layout(layout),
                orientation(layout),
                None,
                workspace_rect,
                children,
                vec![],
                focus,
                false,
                NodeProperties::None {},
            );
            node.percent = percent;
            Some(node)
        }
        IpcNode::Leaf {
            window,
            percent,
            rect,
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
            );
            node.percent = percent;
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

fn describe_window(
    mapped: &Mapped,
    rect: Rect,
    node_type: NodeType,
    floating: &str,
    parent: Option<Rect>,
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
        visible: true,
    });
    let title = with_toplevel_role(mapped.toplevel(), |role| role.title.clone());
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
        title.as_deref(),
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
    node.scratchpad_state = Some("none".into());
    node.fullscreen_mode = i32::from(mapped.pending_sizing_mode().is_fullscreen());
    node.geometry = rect_from(0., 0., mapped.size().w.into(), mapped.size().h.into());
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

fn scratch_output(rect: Rect) -> Node {
    let workspace = common_node(
        SCRATCH_WORKSPACE_ID,
        NodeType::Workspace,
        NodeLayout::SplitH,
        "horizontal",
        Some("__i3_scratch"),
        rect,
        vec![],
        vec![],
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
fn workspace_id(id: u64) -> i64 {
    WORKSPACE_ID_BASE + i64::try_from(id % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
fn container_id(id: NodeId) -> i64 {
    CONTAINER_ID_BASE + i64::try_from(id.0 % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
}
fn window_id(id: MappedId) -> i64 {
    WINDOW_ID_BASE + i64::try_from(id.get() % ID_NAMESPACE_SIZE as u64).unwrap_or_default()
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

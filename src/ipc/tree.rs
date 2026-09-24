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
use crate::utils::{with_toplevel_role, ResizeEdge};
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
        describe_output_node(
            layout,
            global_space,
            monitor,
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
    describe_workspaces_with_marks(
        layout,
        global_space,
        &Default::default(),
        &Default::default(),
    )
}

pub(crate) fn describe_workspaces_with_marks(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
) -> Vec<Workspace> {
    layout
        .workspaces()
        .filter_map(|(monitor, index, workspace)| {
            let monitor = monitor?;
            if !workspace.must_be_kept() && monitor.active_workspace_ref().id() != workspace.id() {
                return None;
            }
            let workspace_focused = layout
                .active_monitor_ref()
                .is_some_and(|active| active.output() == monitor.output())
                && monitor.active_workspace_idx() == index;
            let Node {
                border,
                current_border_width,
                deco_rect,
                floating,
                floating_nodes,
                focus,
                focused: _,
                fullscreen_mode,
                geometry,
                id,
                layout,
                marks,
                name,
                orientation,
                percent,
                rect,
                scratchpad_state,
                sticky,
                node_type,
                urgent,
                window,
                window_rect,
                properties,
                ..
            } = describe_workspace_node(
                layout,
                workspace,
                monitor.output_name(),
                index,
                workspace_rect(global_space, monitor.output()),
                output_rect(global_space, monitor.output()),
                marks,
                container_marks,
            );
            let NodeProperties::Workspace(properties) = properties else {
                unreachable!()
            };
            Some(Workspace {
                border,
                current_border_width,
                deco_rect,
                floating,
                floating_nodes,
                focus,
                focused: workspace_focused,
                fullscreen_mode,
                geometry,
                id,
                layout,
                marks,
                name: name.unwrap_or_default(),
                nodes: vec![],
                num: properties.num,
                orientation,
                output: properties.output,
                percent,
                rect,
                representation: properties.representation,
                scratchpad_state,
                sticky,
                node_type,
                urgent,
                visible: monitor.active_workspace_idx() == index,
                window,
                window_rect,
            })
        })
        .collect()
}

pub(crate) fn sway_transform(transform: smithay::utils::Transform) -> &'static str {
    match transform {
        // Sway reports clockwise rotations; Smithay's transform is
        // counter-clockwise, so 90 and 270 are inverted.
        smithay::utils::Transform::Normal => "normal",
        smithay::utils::Transform::_90 => "270",
        smithay::utils::Transform::_180 => "180",
        smithay::utils::Transform::_270 => "90",
        smithay::utils::Transform::Flipped => "flipped",
        smithay::utils::Transform::Flipped90 => "flipped-270",
        smithay::utils::Transform::Flipped180 => "flipped-180",
        smithay::utils::Transform::Flipped270 => "flipped-90",
    }
}

pub(crate) fn sway_subpixel_hinting(subpixel: smithay::output::Subpixel) -> &'static str {
    match subpixel {
        smithay::output::Subpixel::Unknown => "unknown",
        smithay::output::Subpixel::None => "none",
        smithay::output::Subpixel::HorizontalRgb => "rgb",
        smithay::output::Subpixel::HorizontalBgr => "bgr",
        smithay::output::Subpixel::VerticalRgb => "vrgb",
        smithay::output::Subpixel::VerticalBgr => "vbgr",
    }
}

pub fn describe_outputs(layout: &Layout<Mapped>, global_space: &Space<Window>) -> Vec<Output> {
    describe_outputs_with_power(layout, global_space, &std::collections::HashMap::new())
}

pub fn describe_outputs_with_power(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    output_power: &std::collections::HashMap<String, bool>,
) -> Vec<Output> {
    layout
        .monitors()
        .map(|monitor| {
            let output = monitor.output();
            let mode = output.current_mode();
            let physical = output.physical_properties();
            let transform = sway_transform(output.current_transform());
            let subpixel_hinting = sway_subpixel_hinting(physical.subpixel);
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
            let powered = output_power
                .get(monitor.output_name())
                .copied()
                .unwrap_or(true);
            // This serializer receives the active layout, not backend connector
            // state. Therefore active is true and primary is false. Runtime
            // power state supplies sway's identical dpms and power fields.
            // Swayward has no scale-filter setting and reports nearest. Backend
            // adaptive-sync, tearing, HDR, and render-time capability/state do
            // not reach this query, so those fields conservatively report their
            // disabled defaults. Keep GET_OUTPUTS documented as Partial until
            // that state is plumbed in.
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
                dpms: powered,
                features: OutputFeatures {
                    adaptive_sync: false,
                    hdr: false,
                },
                floating: None,
                floating_nodes: vec![],
                focus: vec![workspace_id(monitor.active_workspace_ref().id().get())],
                focused: layout
                    .active_monitor_ref()
                    .is_some_and(|active| active.output() == output),
                fullscreen_mode: 0,
                geometry: Rect::default(),
                hdr: false,
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
                power: powered,
                primary: false,
                rect: output_rect(global_space, output),
                scale: output.current_scale().fractional_scale(),
                scale_filter: "nearest".into(),
                scratchpad_state: None,
                serial: physical.serial_number.clone(),
                sticky: false,
                subpixel_hinting: subpixel_hinting.into(),
                transform: transform.into(),
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
    root_rect: Rect,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
) -> Node {
    let rect = output_rect(global_space, monitor.output());
    // A workspace covers the usable area, not the whole output: sway subtracts
    // every layer-shell exclusive zone, so a bar's strip belongs to the output
    // rect and not to the workspace below it. See the GET_TREE example in
    // sway/sway-ipc.7.scd, where the output is y=0 h=1080 while its workspace
    // is y=23 h=1057 under a 23px bar. Scripts size floating windows from this
    // rect, so reporting the full output puts them under the bar.
    let workspace_rect = workspace_rect(global_space, monitor.output());
    let workspaces = layout
        .workspaces()
        .filter(|(candidate, _, workspace)| {
            (workspace.must_be_kept() || monitor.active_workspace_ref().id() == workspace.id())
                && candidate.is_some_and(|candidate| candidate.output() == monitor.output())
        })
        .map(|(_, index, workspace)| {
            describe_workspace_node(
                layout,
                workspace,
                monitor.output_name(),
                index,
                workspace_rect,
                rect,
                marks,
                container_marks,
            )
        })
        .collect::<Vec<_>>();
    let active_workspace_id = workspace_id(monitor.active_workspace_ref().id().get());
    let previous_workspace_id = monitor
        .previous_workspace_id()
        .map(|id| workspace_id(id.get()));
    let mut focus = workspaces
        .iter()
        .map(|workspace| workspace.id)
        .collect::<Vec<_>>();
    focus.sort_by_key(|id| {
        if *id == active_workspace_id {
            0
        } else if Some(*id) == previous_workspace_id {
            1
        } else {
            2
        }
    });
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
            features: output.features,
            hdr: output.hdr,
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
    let root_area = i64::from(root_rect.width) * i64::from(root_rect.height);
    let output_area = i64::from(rect.width) * i64::from(rect.height);
    node.percent = (root_area != 0).then(|| output_area as f64 / root_area as f64);
    node
}

// The flat arguments mirror the distinct workspace fields being serialized;
// bundling them would only move this one call site's context into another type.
#[allow(clippy::too_many_arguments)]
fn describe_workspace_node(
    compositor_layout: &Layout<Mapped>,
    workspace: &crate::layout::workspace::Workspace<Mapped>,
    output: &str,
    index: usize,
    rect: Rect,
    output_origin: Rect,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
) -> Node {
    // Layout geometry is output-relative and already carries the working-area
    // origin: tiled leaves come from `parent_area`, which is the gap-inset
    // working area, and floating positions come from `scale_by_working_area`.
    // Children are therefore translated by the output origin, never by the
    // workspace rect, which would add the exclusive zone and the gaps twice.
    // Only the workspace node itself is reported at `rect`.
    let mut tiled = describe_tiling(
        workspace.ipc_tiling_tree(),
        &|window| workspace.windows().find(|mapped| mapped.window == *window),
        output_origin,
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
    // Visibility is per output, not per seat: the active workspace of every
    // output is on screen, while only one of them holds keyboard focus.
    let workspace_visible = compositor_layout.monitors().any(|monitor| {
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
            let outer_rect = offset_rect(
                Rectangle::new(
                    (x, y).into(),
                    (layout.tile_size.0, layout.tile_size.1).into(),
                ),
                output_origin,
            );
            let mut node = describe_window(
                tile.window(),
                outer_rect,
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
            let deco_rect = workspace.floating().ipc_decoration_rect(tile, &layout);
            let has_titlebar = deco_rect.is_some();
            node.deco_rect =
                deco_rect.map_or_else(Rect::default, |rect| offset_rect(rect, output_origin));
            let border_width = match (node.border, has_titlebar) {
                (NodeBorder::Normal | NodeBorder::Pixel, true) | (NodeBorder::Pixel, false) => {
                    node.current_border_width
                }
                _ => 0,
            };
            let top = if has_titlebar { 0 } else { border_width };
            node.rect = outer_rect;
            node.window_rect = Rect {
                x: border_width,
                y: top,
                width: (outer_rect.width - border_width * 2).max(0),
                height: (outer_rect.height - border_width - top).max(0),
            };
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
    let mut nodes = nodes;
    set_child_windows_visible(layout, &focus, &mut nodes, workspace_visible);
    for node in &mut floating_nodes {
        set_windows_visible(node, workspace_visible);
    }
    let mut node = common_node(
        workspace_id(workspace.id().get()),
        NodeType::Workspace,
        layout,
        &orientation,
        Some(&workspace.sway_display_name(index)),
        rect,
        nodes,
        floating_nodes,
        focus,
        focused,
        NodeProperties::Workspace(swayward_ipc::WorkspaceProperties {
            num: workspace.sway_display_number(index),
            output: output.into(),
            representation,
        }),
    );
    // Sway reports 1 for every workspace node, independent of whether a child
    // is fullscreen (`ipc_json_describe_workspace`, sway 1.12).
    node.fullscreen_mode = 1;
    node.urgent = workspace.is_urgent();
    node
}

fn clear_focused(node: &mut Node) {
    node.focused = false;
    for child in node.nodes.iter_mut().chain(&mut node.floating_nodes) {
        clear_focused(child);
    }
}

/// Marks every window in a workspace subtree as shown or hidden.
///
/// Sway reports `visible` per window: a window on a workspace that is not its
/// output's active one is not visible (`sway/tree/container.c`, and the
/// captured `tests/fixtures/sway/two_workspaces.tree.json` shows
/// `visible: false` for the window on the background workspace). Waybar's
/// `hasFlag` recurses into child nodes, so a window wrongly claiming to be
/// visible marks its whole workspace button visible.
///
/// A window on an inactive tab is not visible either: sway walks up from the
/// view and, at every tabbed or stacked ancestor, requires the seat's active
/// tiling child to be on its path (`view_is_visible`, `sway/tree/view.c:1180-1193`).
/// The active tiling child is the first tiling entry of that container's
/// `focus` list, which the reply already carries.
fn set_windows_visible(node: &mut Node, visible: bool) {
    if let swayward_ipc::NodeProperties::View(properties) = &mut node.properties {
        properties.visible = visible;
    }
    set_child_windows_visible(node.layout, &node.focus, &mut node.nodes, visible);
    for child in &mut node.floating_nodes {
        set_windows_visible(child, visible);
    }
}

fn set_child_windows_visible(
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
            title: _,
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
            let mut children = children
                .into_iter()
                .map(|(_, node)| node)
                .collect::<Vec<_>>();
            if matches!(layout, TreeLayout::Tabbed | TreeLayout::Stacked) {
                for child in &mut children {
                    child.percent = Some(1.);
                }
            }
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
            node.floating = Some("auto_off".into());
            node.percent = percent;
            node.scratchpad_state = Some("none".into());
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
            fullscreen_mode,
            rect,
            deco_rect,
            border,
            border_edges,
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
            node.fullscreen_mode = fullscreen_mode;
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
            let left = border_width * i32::from(border_edges.contains(ResizeEdge::LEFT));
            let right = border_width * i32::from(border_edges.contains(ResizeEdge::RIGHT));
            let top = if has_titlebar {
                0
            } else {
                border_width * i32::from(border_edges.contains(ResizeEdge::TOP))
            };
            let bottom = border_width * i32::from(border_edges.contains(ResizeEdge::BOTTOM));
            node.window_rect = Rect {
                x: left,
                y: top,
                width: (node.rect.width - left - right).max(0),
                height: (node.rect.height - top - bottom).max(0),
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
        // Toggle is an operation, not a state. Treat an invalid stored value as
        // no border rather than panicking on the live GET_TREE path.
        swayward_ipc::command::BorderStyle::Toggle => NodeBorder::None,
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
        tag: None,
        visible,
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
    node.scratchpad_state = Some(if in_scratchpad { "fresh" } else { "none" }.into());
    node.fullscreen_mode = i32::from(mapped.pending_sizing_mode().is_fullscreen());
    node.urgent = mapped.is_urgent();
    let natural_size = mapped.natural_size();
    node.geometry = rect_from(0., 0., natural_size.w.into(), natural_size.h.into());
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
        NodeLayout::SplitH => 'H',
        NodeLayout::SplitV => 'V',
        NodeLayout::Tabbed => 'T',
        NodeLayout::Stacked => 'S',
        _ => 'D',
    };
    let children = children
        .iter()
        .map(|child| {
            if let NodeProperties::View(properties) = &child.properties {
                properties.app_id.clone().unwrap_or_else(|| "(null)".into())
            } else if child.nodes.is_empty() {
                "(null)".to_owned()
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
    let mut scratchpad_windows = layout.scratchpad_windows().collect::<Vec<_>>();
    scratchpad_windows.reverse();
    let floating_nodes = scratchpad_windows
        .into_iter()
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
        .collect::<Vec<_>>();
    let focus = floating_nodes.iter().map(|node| node.id).collect();
    let mut workspace = common_node(
        SCRATCH_WORKSPACE_ID,
        NodeType::Workspace,
        NodeLayout::SplitH,
        "horizontal",
        Some("__i3_scratch"),
        rect,
        vec![],
        floating_nodes,
        focus,
        false,
        NodeProperties::None {},
    );
    workspace.fullscreen_mode = 1;
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

/// The output's usable area, in global coordinates.
///
/// This is the output rect minus every layer-shell exclusive zone, which is
/// what sway reports as a workspace's rect. Outer gaps are deliberately not
/// applied: sway's own example shows a window sharing its workspace's rect
/// exactly, so gaps live inside this area rather than shrinking it.
fn workspace_rect(global_space: &Space<Window>, output: &smithay::output::Output) -> Rect {
    let Some(geometry) = global_space.output_geometry(output) else {
        return Rect::default();
    };
    let usable = crate::layout::workspace::compute_working_area(output);
    rect_from_rectangle(
        Rectangle::new(geometry.loc.to_f64() + usable.loc, usable.size).to_i32_round(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representation_letters_match_split_orientation() {
        assert_eq!(tree_representation(NodeLayout::SplitH, &[]), "H[]");
        assert_eq!(tree_representation(NodeLayout::SplitV, &[]), "V[]");
    }

    #[test]
    fn ipc_border_is_total_for_command_only_styles() {
        assert_eq!(
            ipc_border(swayward_ipc::command::BorderStyle::Toggle),
            NodeBorder::None
        );
    }
}

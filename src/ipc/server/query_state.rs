use super::*;

#[derive(Default)]
pub(super) struct QueryState {
    pub(super) loaded_config_file_name: String,
    pub(super) tree: String,
    pub(super) event_tree: String,
    pub(super) workspaces: String,
    pub(super) outputs: String,
    pub(super) marks: String,
    pub(super) binding_modes: String,
    pub(super) binding_state: String,
    pub(super) inputs: String,
    pub(super) seats: String,
    /// GET_TREE's focused node id, which is also the seat's focus.
    pub(super) seat_focus: i64,
    /// The serialised input devices, kept to rebuild `seats` when focus moves.
    pub(super) seat_devices: Vec<serde_json::Value>,
    pub(super) seat_capabilities: u32,
}

/// Recompute every cached query reply from live compositor state.
pub(crate) fn ipc_outputs_snapshot(state: &State) -> crate::backend::IpcOutputMap {
    state
        .backend
        .ipc_outputs()
        .lock()
        .unwrap_or_else(|poisoned| {
            warn!("backend IPC output state mutex was poisoned; using its last state");
            poisoned.into_inner()
        })
        .clone()
}

pub(super) fn refresh_all_query_state(state: &mut State) {
    let Some(server) = &state.swayward.ipc_server else {
        return;
    };
    let mut query_state = server.query_state.borrow_mut();
    query_state.binding_modes = binding_modes(&state.swayward.config.borrow());
    query_state.binding_state = binding_state(&state.swayward.binding_mode);
    refresh_input_query_state(&state.swayward, &mut query_state);
    let ipc_outputs = ipc_outputs_snapshot(state);
    refresh_query_state(
        &state.swayward.layout,
        &state.swayward.global_space,
        &state.swayward.output_power,
        &ipc_outputs,
        &state.swayward.marks_by_window,
        &state.swayward.marks_by_container,
        &mut query_state,
    );
}

pub(super) fn binding_modes(config: &swayward_config::Config) -> String {
    let modes = std::iter::once("default")
        .chain(config.binding_modes.iter().map(|mode| mode.name.as_str()))
        .collect::<Vec<_>>();
    serde_json::to_string(&modes).unwrap_or_else(|_| "[]".into())
}

pub(super) fn binding_state(mode: &str) -> String {
    serde_json::json!({"name": mode}).to_string()
}

pub(crate) fn keyboard_layouts(state: &mut State) -> Option<KeyboardLayouts> {
    let keyboard = state.swayward.seat.get_keyboard()?;
    keyboard.with_xkb_state(state, |context| {
        let Ok(xkb) = context.xkb().lock() else {
            error!("cannot refresh IPC keyboard layouts: XKB state lock is poisoned");
            return None;
        };
        Some(KeyboardLayouts {
            names: xkb
                .layouts()
                .map(|layout| xkb.layout_name(layout).to_owned())
                .collect(),
            current_idx: xkb.active_layout().0,
        })
    })
}

pub(super) fn serialize_outcomes(outcomes: &[CommandOutcome]) -> String {
    serde_json::to_string(outcomes)
        .unwrap_or_else(|_| r#"[{"success":false,"error":"serialization failed"}]"#.into())
}

#[derive(serde::Serialize)]
pub(super) struct IpcSeat<'a> {
    name: &'a str,
    capabilities: u32,
    focus: i64,
    devices: &'a [serde_json::Value],
}

pub(super) fn describe_input(
    swayward: &crate::swayward::Swayward,
    device: &crate::input::IpcInputDevice,
) -> serde_json::Value {
    let mut value = serde_json::to_value(device).unwrap_or_default();
    if device.device_type == "pointer" {
        if let Some(object) = value.as_object_mut() {
            let config = swayward.config.borrow();
            let factor = config
                .input
                .mouse
                .scroll_factor
                .and_then(|factor| {
                    let (horizontal, vertical) = factor.h_v_factors();
                    (horizontal == vertical).then_some(horizontal)
                })
                .unwrap_or(1.);
            object.insert("scroll_factor".into(), factor.into());
        }
    } else if device.device_type == "keyboard" {
        if let Some(object) = value.as_object_mut() {
            let config = swayward.config.borrow();
            object.insert(
                "repeat_delay".into(),
                serde_json::json!(config.input.keyboard.repeat_delay),
            );
            object.insert(
                "repeat_rate".into(),
                serde_json::json!(config.input.keyboard.repeat_rate),
            );
        }
        let layouts = swayward.ipc_server.as_ref().and_then(|server| {
            server
                .event_stream_state
                .borrow()
                .keyboard_layouts
                .keyboard_layouts
                .clone()
        });
        if let (Some(layouts), Some(object)) = (layouts, value.as_object_mut()) {
            object.insert("xkb_layout_names".into(), serde_json::json!(layouts.names));
            object.insert(
                "xkb_active_layout_index".into(),
                serde_json::json!(layouts.current_idx),
            );
            object.insert(
                "xkb_active_layout_name".into(),
                layouts
                    .names
                    .get(layouts.current_idx as usize)
                    .map_or(serde_json::Value::Null, |name| serde_json::json!(name)),
            );
        }
    }
    value
}

pub(super) fn describe_inputs(swayward: &crate::swayward::Swayward) -> Vec<serde_json::Value> {
    let mut input_devices = swayward.ipc_input_devices.values().collect::<Vec<_>>();
    input_devices.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    input_devices
        .into_iter()
        .map(|device| describe_input(swayward, device))
        .collect()
}

pub(super) fn refresh_input_query_state(
    swayward: &crate::swayward::Swayward,
    state: &mut QueryState,
) {
    let devices = describe_inputs(swayward);
    state.inputs = serde_json::to_string(&devices).unwrap_or_else(|_| "[]".into());

    let capabilities = seat_capabilities(&devices);
    state.seat_devices = devices;
    state.seat_capabilities = capabilities;
    refresh_seats(state);
}

/// The seat's `wl_seat` capability bits, folded from its input devices.
///
/// Sway gives a tablet tool the pointer capability, while switches and tablet
/// pads add none (`sway/sway/input/seat.c:604-625`).
fn seat_capabilities(devices: &[serde_json::Value]) -> u32 {
    devices.iter().fold(0, |capabilities, device| {
        capabilities
            | match device["type"].as_str() {
                Some("pointer" | "tablet_tool") => 1,
                Some("keyboard") => 2,
                Some("touch") => 4,
                _ => 0,
            }
    })
}

/// Rebuild GET_SEATS from the cached devices and the tree's focused node.
fn refresh_seats(state: &mut QueryState) {
    state.seats = serde_json::to_string(&[IpcSeat {
        name: "seat0",
        capabilities: state.seat_capabilities,
        focus: state.seat_focus,
        devices: &state.seat_devices,
    }])
    .unwrap_or_else(|_| "[]".into());
}

/// The id of the single node GET_TREE marks focused.
///
/// Sway reports `seat_get_focus(seat)` for the seat, which is any node: a
/// view, a split container after `focus parent`, or the workspace when it is
/// empty (`sway/sway/ipc-json.c:1238`). GET_TREE marks the same node
/// `focused`, so the two replies cannot disagree.
fn tree_focus(node: &swayward_ipc::Node) -> Option<i64> {
    if node.focused {
        return Some(node.id);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(tree_focus)
}

pub(crate) fn find_node_by_id(value: &serde_json::Value, id: i64) -> Option<&serde_json::Value> {
    if value.get("id").and_then(serde_json::Value::as_i64) == Some(id) {
        return Some(value);
    }
    ["nodes", "floating_nodes"].into_iter().find_map(|key| {
        value
            .get(key)?
            .as_array()?
            .iter()
            .find_map(|child| find_node_by_id(child, id))
    })
}

pub(super) fn find_parent_of_node(
    value: &serde_json::Value,
    id: i64,
) -> Option<&serde_json::Value> {
    ["nodes", "floating_nodes"].into_iter().find_map(|key| {
        let children = value.get(key)?.as_array()?;
        if children
            .iter()
            .any(|child| child.get("id").and_then(serde_json::Value::as_i64) == Some(id))
        {
            return Some(value);
        }
        children
            .iter()
            .find_map(|child| find_parent_of_node(child, id))
    })
}

pub(super) fn find_workspace_by_tree_id(
    node: &swayward_ipc::Node,
    id: i64,
) -> Option<&swayward_ipc::Node> {
    if node.node_type == swayward_ipc::NodeType::Workspace && node.id == id {
        return Some(node);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(|child| find_workspace_by_tree_id(child, id))
}

pub(super) fn find_focused_node(node: &swayward_ipc::Node) -> Option<&swayward_ipc::Node> {
    if node.focused && node.node_type != swayward_ipc::NodeType::Workspace {
        return Some(node);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(find_focused_node)
}

pub(super) fn clear_workspace_focus(node: &mut swayward_ipc::Node, hide: bool) {
    node.focused = false;
    if hide {
        if let swayward_ipc::NodeProperties::View(properties) = &mut node.properties {
            if !node.sticky {
                properties.visible = false;
            }
        }
    }
    for child in node.nodes.iter_mut().chain(&mut node.floating_nodes) {
        clear_workspace_focus(child, hide);
    }
}

pub(super) fn find_workspace_by_id(
    node: &swayward_ipc::Node,
    id: u64,
) -> Option<&swayward_ipc::Node> {
    if node.node_type == swayward_ipc::NodeType::Workspace
        && node.id == crate::ipc::tree::workspace_id(id)
    {
        return Some(node);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(|child| find_workspace_by_id(child, id))
}

/// Append every mark in the tree, parent before child, matching sway's
/// `root_for_each_container` walk.
pub(super) fn collect_marks(node: &swayward_ipc::Node, out: &mut Vec<String>) {
    out.extend(node.marks.iter().cloned());
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect_marks(child, out);
    }
}

pub(super) fn refresh_query_state(
    layout: &crate::layout::Layout<Mapped>,
    global_space: &smithay::desktop::Space<smithay::desktop::Window>,
    output_power: &std::collections::HashMap<String, bool>,
    ipc_outputs: &crate::backend::IpcOutputMap,
    marks: &std::collections::HashMap<crate::window::mapped::MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<crate::layout::tiling_tree::NodeId, Vec<String>>,
    state: &mut QueryState,
) {
    let tree = describe_tree_with_power(layout, global_space, marks, container_marks, output_power);
    let focus = tree_focus(&tree).unwrap_or(0);
    if focus != state.seat_focus {
        state.seat_focus = focus;
        refresh_seats(state);
    }
    state.tree = serde_json::to_string(&tree)
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    state.workspaces = serde_json::to_string(&describe_workspaces_with_marks(
        layout,
        global_space,
        marks,
        container_marks,
    ))
    .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    let mut outputs = serde_json::to_value(crate::ipc::tree::describe_outputs_with_power(
        layout,
        global_space,
        output_power,
    ))
    .unwrap_or_default();
    if let Some(outputs) = outputs.as_array_mut() {
        // Backend state owns the complete advertised mode list. The layout
        // output exposes only the current smithay mode.
        for output in outputs.iter_mut() {
            let Some(name) = output["name"].as_str() else {
                continue;
            };
            let Some(backend) = ipc_outputs
                .values()
                .find(|candidate| candidate.name == name)
            else {
                continue;
            };
            output["modes"] = serde_json::Value::Array(
                backend
                    .modes
                    .iter()
                    .map(|mode| {
                        serde_json::json!({
                            "width": mode.width,
                            "height": mode.height,
                            "refresh": mode.refresh_rate,
                            "picture_aspect_ratio": "none",
                        })
                    })
                    .collect(),
            );
        }
        outputs.extend(
            ipc_outputs
                .values()
                .filter(|output| output.logical.is_none())
                .map(|output| {
                    serde_json::json!({
                        "active": false,
                        "current_workspace": null,
                        "dpms": false,
                        "features": { "adaptive_sync": false, "hdr": false },
                        "make": if output.make.is_empty() { "Unknown" } else { &output.make },
                        "model": if output.model.is_empty() { "Unknown" } else { &output.model },
                        "modes": output.modes.iter().map(|mode| serde_json::json!({
                            "width": mode.width,
                            "height": mode.height,
                            "refresh": mode.refresh_rate,
                            "picture_aspect_ratio": "none",
                        })).collect::<Vec<_>>(),
                        "name": output.name,
                        "non_desktop": false,
                        "percent": null,
                        "power": false,
                        "primary": false,
                        "rect": swayward_ipc::Rect::default(),
                        "serial": output.serial.as_deref().unwrap_or("Unknown"),
                        "type": "output",
                    })
                }),
        );
    }
    state.outputs = serde_json::to_string(&outputs)
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    // Sway walks the container tree and appends each container's marks in the
    // order it meets them (`sway/tree/root.c:246-260`,
    // `sway/ipc-server.c:604-610,825-834`). Collecting from the tree we just
    // built gives that order for free, and reaches marks on split containers
    // as well as on views. Sorting the per-window map did neither: it imposed
    // an order sway never uses, and omitted container marks that GET_TREE was
    // already reporting.
    let mut all_marks = Vec::new();
    collect_marks(&tree, &mut all_marks);
    state.marks = serde_json::to_string(&all_marks)
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
}

#[cfg(test)]
mod tests {
    use super::seat_capabilities;

    #[test]
    fn tablet_tools_give_the_seat_pointer_capability_like_sway() {
        let devices = |types: &[&str]| {
            types
                .iter()
                .map(|kind| serde_json::json!({ "type": kind }))
                .collect::<Vec<_>>()
        };
        assert_eq!(seat_capabilities(&devices(&["tablet_tool"])), 1);
        assert_eq!(seat_capabilities(&devices(&["tablet_pad", "switch"])), 0);
        assert_eq!(
            seat_capabilities(&devices(&["keyboard", "pointer", "touch", "tablet_tool"])),
            7
        );
    }
}

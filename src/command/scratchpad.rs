use swayward_ipc::CommandOutcome;

use super::{failure, CommandTarget};
use crate::swayward::State;

pub(super) fn show(state: &mut State) {
    state.ipc_order_scratchpad_events(crate::ipc::server::ScratchpadEventOrder::Show);
    let shown = state.swayward.layout.show_scratchpad(None);
    let group = shown.as_ref().and_then(|window| {
        let workspace = state.swayward.layout.active_workspace()?;
        Some((
            workspace.floating_tree_root_for_window(window)?,
            workspace.active_window()?.id(),
        ))
    });
    if let Some((root, focused)) = group {
        state.ipc_refresh_layout();
        if let Some(server) = &state.swayward.ipc_server {
            let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
                &state.swayward.layout,
                &state.swayward.global_space,
                &state.swayward.marks_by_window,
                &state.swayward.marks_by_container,
            ))
            .unwrap_or_default();
            for (change, id) in [
                ("focus", crate::ipc::tree::window_id(focused)),
                ("move", crate::ipc::tree::container_id(root)),
            ] {
                if let Some(mut container) = crate::ipc::server::find_node_by_id(&tree, id).cloned()
                {
                    if change == "focus" {
                        container["focused"] = true.into();
                        if let Some(percent) = container["percent"].as_f64() {
                            container["percent"] = (1. - percent).into();
                        }
                    } else {
                        container["focused"] = false.into();
                        if let Some(focused) =
                            container["focus"].as_array().and_then(|ids| ids.first())
                        {
                            let focused = focused.clone();
                            if let Some(nodes) = container["nodes"].as_array_mut() {
                                for node in nodes {
                                    node["focused"] = (node["id"] == focused).into();
                                }
                            }
                        }
                        container["scratchpad_state"] = "fresh".into();
                    }
                    server.send_event(swayward_ipc::legacy::Event::SwayWindowChanged {
                        change: change.into(),
                        container,
                    });
                }
            }
        }
    }
    state.swayward.queue_redraw_all();
}

pub(super) fn move_targeted(
    state: &mut State,
    target: CommandTarget,
) -> Result<(), CommandOutcome> {
    let window = target_window(state, target)?;
    state.ipc_order_scratchpad_events(crate::ipc::server::ScratchpadEventOrder::Hide);
    state.swayward.layout.move_to_scratchpad(Some(&window));
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn show_targeted(
    state: &mut State,
    target: CommandTarget,
) -> Result<(), CommandOutcome> {
    let window = target_window(state, target)?;
    if !state.swayward.layout.is_scratchpad_window(&window) {
        return Err(failure("Container is not in scratchpad."));
    }
    let order = if state.swayward.layout.is_scratchpad_hidden(&window) {
        crate::ipc::server::ScratchpadEventOrder::Show
    } else {
        crate::ipc::server::ScratchpadEventOrder::Hide
    };
    state.ipc_order_scratchpad_events(order);
    state.swayward.layout.show_scratchpad(Some(&window));
    state.swayward.queue_redraw_all();
    Ok(())
}

fn target_window(
    state: &State,
    target: CommandTarget,
) -> Result<smithay::desktop::Window, CommandOutcome> {
    match target {
        CommandTarget::Container(workspace, node) => state
            .swayward
            .layout
            .window_in_node(workspace, node)
            .ok_or_else(|| failure("No matching node.")),
        CommandTarget::Window(target) => state
            .swayward
            .layout
            .windows()
            .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()))
            .ok_or_else(|| failure("No matching node.")),
    }
}

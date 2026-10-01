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
        state.ipc_emit_window_change("focus", crate::ipc::tree::window_id(focused), |container| {
            container["focused"] = true.into();
            if let Some(percent) = container["percent"].as_f64() {
                container["percent"] = (1. - percent).into();
            }
        });
        state.ipc_emit_window_change("move", crate::ipc::tree::container_id(root), |container| {
            container["focused"] = false.into();
            if let Some(focused) = container["focus"].as_array().and_then(|ids| ids.first()) {
                let focused = focused.clone();
                if let Some(nodes) = container["nodes"].as_array_mut() {
                    for node in nodes {
                        node["focused"] = (node["id"] == focused).into();
                    }
                }
            }
            container["scratchpad_state"] = "fresh".into();
        });
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
        // CMD_INVALID in sway (`sway/sway/commands/scratchpad.c:118-125`), so it
        // stops the remaining matches and the command list.
        return Err(swayward_ipc::command::parse_error(
            "Container is not in scratchpad.",
        ));
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
        CommandTarget::Window(target) => {
            super::mapped_window(state, target).ok_or_else(|| failure("No matching node."))
        }
    }
}

pub(super) fn move_focused(state: &mut State) -> super::HandlerResult {
    let floating_root = state
        .swayward
        .layout
        .active_workspace()
        .and_then(crate::layout::workspace::Workspace::focused_floating_tree_root);
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(swayward_ipc::command::parse_error(
            "Can't move an empty workspace to the scratchpad",
        ));
    };
    let window = match target {
        CommandTarget::Container(workspace, node) => {
            let window = state.swayward.layout.window_in_node(workspace, node);
            if state
                .swayward
                .layout
                .set_container_floating(workspace, node, true)
                .is_none()
            {
                return Err(failure("No matching node."));
            }
            window
        }
        CommandTarget::Window(_) => None,
    };
    state.ipc_order_scratchpad_events(crate::ipc::server::ScratchpadEventOrder::Hide);
    state.swayward.layout.move_to_scratchpad(window.as_ref());
    if let Some(root) = floating_root {
        state.ipc_refresh_layout();
        state.ipc_emit_window_change("move", crate::ipc::tree::container_id(root), |container| {
            if let Some(container) = container.as_object_mut() {
                container.remove("visible");
            }
        });
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn show_focused(state: &mut State) -> super::HandlerResult {
    if state.swayward.layout.scratchpad_is_empty() {
        return Err(swayward_ipc::command::parse_error("Scratchpad is empty"));
    }
    show(state);
    Ok(None)
}

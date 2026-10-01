use super::{create_output, failure, HandlerResult, Toggle, XkbLayoutTarget};
use crate::swayward::State;

pub(super) fn create(state: &mut State) -> HandlerResult {
    let State { backend, swayward } = state;
    let headless = match backend {
        crate::backend::Backend::Headless(headless) => Some(headless),
        crate::backend::Backend::Tty(_) | crate::backend::Backend::Winit(_) => None,
    };
    super::handled_outcome(create_output(headless, swayward))
}

pub(super) fn switch_layout(
    state: &mut State,
    identifier: &str,
    target: XkbLayoutTarget,
) -> HandlerResult {
    let matches_keyboard = state.swayward.ipc_input_devices.values().any(|device| {
        device.device_type == "keyboard"
            && (matches!(identifier, "*" | "type:keyboard") || device.identifier == identifier)
    });
    if let (true, Some(keyboard)) = (matches_keyboard, state.swayward.seat.get_keyboard()) {
        keyboard.with_xkb_state(state, |mut context| match target {
            XkbLayoutTarget::Next => context.cycle_next_layout(),
            XkbLayoutTarget::Prev => context.cycle_prev_layout(),
            XkbLayoutTarget::Index(index) => {
                let count = context.xkb().lock().map_or(0, |xkb| xkb.layouts().count());
                if (index as usize) < count {
                    context.set_layout(smithay::input::keyboard::Layout(index));
                }
            }
        });
        state.ipc_refresh_keyboard_layout_index();
    }
    Ok(None)
}

pub(super) fn configure(
    state: &mut State,
    target: String,
    actions: Vec<swayward_ipc::OutputAction>,
) -> HandlerResult {
    if actions
        .iter()
        .any(|action| matches!(action, swayward_ipc::OutputAction::Off))
    {
        state.ipc_suppress_workspace_moves();
    }
    let targets = if target == "*" {
        state
            .swayward
            .global_space
            .outputs()
            .map(|output| output.name())
            .collect::<Vec<_>>()
    } else {
        vec![target]
    };
    let has_output_event = actions.iter().any(|action| {
        matches!(
            action,
            swayward_ipc::OutputAction::On
                | swayward_ipc::OutputAction::Off
                | swayward_ipc::OutputAction::Power { .. }
        )
    });
    for target in targets {
        for action in &actions {
            if let swayward_ipc::OutputAction::Power { power } = action {
                let output = state.swayward.output_by_name_match(&target).cloned();
                if *power == Toggle::Toggle && output.is_none() {
                    return Err(failure(format!(
                        "Cannot apply toggle to unknown output {target}"
                    )));
                }
                let target = output
                    .as_ref()
                    .map_or_else(|| target.clone(), |output| output.name());
                let current = state
                    .swayward
                    .output_power
                    .get(&target)
                    .copied()
                    .unwrap_or(true);
                let enabled = match power {
                    Toggle::Enable => true,
                    Toggle::Disable => false,
                    Toggle::Toggle => !current,
                };
                state.swayward.output_power.insert(target, enabled);
                if let Some(output) = output {
                    state.backend.set_output_power(&output, enabled);
                    if enabled {
                        state.swayward.queue_redraw(&output);
                    }
                }
            }
        }
        let config_actions = actions
            .iter()
            .filter(|action| !matches!(action, swayward_ipc::OutputAction::Power { .. }))
            .cloned()
            .collect::<Vec<_>>();
        if !config_actions.is_empty() {
            state.apply_transient_output_config(&target, &config_actions);
        }
    }
    if has_output_event {
        // One event per configuration change, like sway's
        // update_output_manager_config (sway/desktop/output.c:377-399), even
        // when the change also repositioned surviving outputs.
        state.swayward.ipc_outputs_changed = true;
        state.refresh_ipc_outputs();
    }
    Ok(None)
}

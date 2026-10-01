use swayward_ipc::criteria;

use super::targeted::focused_con_id;
use super::{failure, HandlerResult};
use crate::swayward::State;

fn parse(state: &State, raw: &str) -> Result<criteria::Criteria, swayward_ipc::CommandOutcome> {
    criteria::Criteria::parse(raw, focused_con_id(state)).map_err(failure)
}

pub(super) fn assign(
    state: &mut State,
    raw: String,
    target: swayward_ipc::command::AssignmentTarget,
) -> HandlerResult {
    let criteria = parse(state, &raw)?;
    state
        .swayward
        .runtime_window_rules
        .push(crate::swayward::RuntimeWindowRule::Assign(criteria, target));
    Ok(None)
}

pub(super) fn no_focus(state: &mut State, raw: String) -> HandlerResult {
    let criteria = parse(state, &raw)?;
    if !state.swayward.runtime_window_rules.iter().any(|rule| {
        matches!(rule, crate::swayward::RuntimeWindowRule::NoFocus(existing, _) if existing == &raw)
    }) {
        state
            .swayward
            .runtime_window_rules
            .push(crate::swayward::RuntimeWindowRule::NoFocus(raw, criteria));
    }
    Ok(None)
}

pub(super) fn for_window(state: &mut State, raw: String, command: String) -> HandlerResult {
    let criteria = parse(state, &raw)?;
    if !state
        .swayward
        .for_window
        .iter()
        .any(|(existing_raw, existing_command, _)| {
            existing_raw == &raw && existing_command == &command
        })
    {
        state
            .swayward
            .runtime_for_window
            .insert((raw.clone(), command.clone()));
        state.swayward.for_window.push((raw, command, criteria));
    }
    Ok(None)
}

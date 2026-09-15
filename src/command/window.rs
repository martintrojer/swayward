use swayward_config::Action;
use swayward_ipc::command::Border;
use swayward_ipc::legacy::SizeChange;
use swayward_ipc::CommandOutcome;

use super::{failure, CommandTarget, ResizeAmount, ResizeAxis, ResizeUnit, Toggle};
use crate::swayward::State;

fn target_window(
    state: &State,
    target: CommandTarget,
    container_error: &str,
) -> Result<smithay::desktop::Window, CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure(container_error));
    };
    state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()))
        .ok_or_else(|| failure("No matching node."))
}

pub(super) fn sticky(
    state: &mut State,
    target: CommandTarget,
    value: &str,
) -> Result<(), CommandOutcome> {
    let window = target_window(state, target, "No current container")?;
    if state.swayward.layout.is_scratchpad_hidden(&window) {
        return Ok(());
    }
    if !state.swayward.layout.set_window_sticky(&window, value) {
        return Err(failure("Expected output to have a workspace"));
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn border(
    state: &mut State,
    target: CommandTarget,
    border: &Border,
) -> Result<(), CommandOutcome> {
    let window = target_window(state, target, "Only views can have borders")?;
    state
        .swayward
        .layout
        .set_window_border(&window, border.style, border.width)
        .map_err(failure)?;
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn floating(
    state: &mut State,
    target: CommandTarget,
    mode: &Toggle,
) -> Result<(), CommandOutcome> {
    let window = target_window(state, target, "command requires a window target")?;
    if state.swayward.layout.is_scratchpad_hidden(&window) {
        return Err(failure(
            "Can't change floating on hidden scratchpad container",
        ));
    }
    match mode {
        Toggle::Enable => state
            .swayward
            .layout
            .set_window_floating(Some(&window), true),
        Toggle::Disable => state
            .swayward
            .layout
            .set_window_floating(Some(&window), false),
        Toggle::Toggle => state.swayward.layout.toggle_window_floating(Some(&window)),
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn kill(state: &mut State, target: CommandTarget) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("command requires a window target"));
    };
    state.do_action(Action::CloseWindowById(target.get()), false);
    Ok(())
}

fn set_size_change(amount: ResizeAmount, floating: bool) -> Option<SizeChange> {
    (amount.amount > 0).then(|| match amount.unit {
        ResizeUnit::Pixels | ResizeUnit::Default if floating => SizeChange::SetFixed(amount.amount),
        ResizeUnit::Pixels => SizeChange::SetFixed(amount.amount),
        ResizeUnit::Default | ResizeUnit::PercentagePoints => {
            SizeChange::SetProportion(f64::from(amount.amount))
        }
    })
}

pub(super) fn resize_set(
    state: &mut State,
    target: CommandTarget,
    width: Option<ResizeAmount>,
    height: Option<ResizeAmount>,
) -> Result<(), CommandOutcome> {
    let floating = match target {
        CommandTarget::Window(target) => state
            .swayward
            .layout
            .windows()
            .any(|(_, mapped)| mapped.id() == target && mapped.is_floating()),
        CommandTarget::Container(_, _) => false,
    };
    let width = width.and_then(|amount| set_size_change(amount, floating));
    let height = height.and_then(|amount| set_size_change(amount, floating));
    match target {
        CommandTarget::Window(_) => {
            let window = target_window(state, target, "command requires a window target")?;
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return Err(failure("Cannot resize a hidden scratchpad container"));
            }
            state
                .swayward
                .layout
                .set_window_size_sway(&window, width, height);
        }
        CommandTarget::Container(workspace, node) => {
            state
                .swayward
                .layout
                .set_tiling_node_size_sway(workspace, node, width, height);
        }
    }
    Ok(())
}

pub(super) fn resize(
    state: &mut State,
    target: CommandTarget,
    grow: bool,
    axis: ResizeAxis,
    first: ResizeAmount,
    second: Option<ResizeAmount>,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("command requires a window target"));
    };
    let window = state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
    let Some(window) = window else {
        return Err(failure("No matching node."));
    };
    if state.swayward.layout.is_scratchpad_hidden(&window) {
        return Err(failure("Cannot resize a hidden scratchpad container"));
    }
    let floating = state
        .swayward
        .layout
        .windows()
        .any(|(_, mapped)| mapped.id() == target && mapped.is_floating());
    let selected = super::movement::select_resize_amount(first, second, floating);
    let sign = if grow { 1 } else { -1 };
    let amount = selected.amount.saturating_mul(sign);
    let change = match selected.unit {
        ResizeUnit::Default if floating => SizeChange::AdjustFixed(amount),
        ResizeUnit::Pixels => SizeChange::AdjustFixed(amount),
        ResizeUnit::Default | ResizeUnit::PercentagePoints => {
            SizeChange::AdjustProportion(f64::from(amount))
        }
    };
    match axis {
        ResizeAxis::Width => state
            .swayward
            .layout
            .set_window_width(Some(&window), change),
        ResizeAxis::Height => state
            .swayward
            .layout
            .set_window_height(Some(&window), change),
        direction => {
            let edge = match direction {
                ResizeAxis::Up => crate::utils::ResizeEdge::TOP,
                ResizeAxis::Down => crate::utils::ResizeEdge::BOTTOM,
                ResizeAxis::Left => crate::utils::ResizeEdge::LEFT,
                ResizeAxis::Right => crate::utils::ResizeEdge::RIGHT,
                ResizeAxis::Width | ResizeAxis::Height => unreachable!(),
            };
            state
                .swayward
                .layout
                .resize_window_edge(Some(&window), edge, change);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_set_preserves_units_and_nonpositive_sentinels() {
        let amount = |amount, unit| ResizeAmount { amount, unit };
        assert_eq!(
            set_size_change(amount(50, ResizeUnit::Default), false),
            Some(SizeChange::SetProportion(50.))
        );
        assert_eq!(
            set_size_change(amount(50, ResizeUnit::Default), true),
            Some(SizeChange::SetFixed(50))
        );
        assert_eq!(
            set_size_change(amount(50, ResizeUnit::PercentagePoints), true),
            Some(SizeChange::SetProportion(50.))
        );
        assert_eq!(set_size_change(amount(0, ResizeUnit::Pixels), false), None);
        assert_eq!(
            set_size_change(amount(-1, ResizeUnit::PercentagePoints), true),
            None
        );
    }
}

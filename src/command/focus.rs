use swayward_config::Action;
use swayward_ipc::CommandOutcome;

use super::{failure, output_target_by_name_or_direction, CommandTarget, Direction};
use crate::swayward::State;

pub(super) fn direction(state: &mut State, direction: Direction) -> Option<Action> {
    let action = match direction {
        Direction::Left => Action::FocusColumnOrMonitorLeft,
        Direction::Right => Action::FocusColumnOrMonitorRight,
        Direction::Up => Action::FocusWindowOrMonitorUp,
        Direction::Down => Action::FocusWindowOrMonitorDown,
    };
    let wrapping = state.swayward.config.borrow().layout.focus_wrapping;
    let local_wrap = matches!(
        wrapping,
        swayward_config::FocusWrapping::Force | swayward_config::FocusWrapping::Workspace
    );
    let workspace_focused = state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| workspace.is_workspace_focused());
    let changed = match (direction, local_wrap) {
        (Direction::Left, true) => state.swayward.layout.focus_left(),
        (Direction::Right, true) => state.swayward.layout.focus_right(),
        (Direction::Up, true) => state.swayward.layout.focus_up(),
        (Direction::Down, true) => state.swayward.layout.focus_down(),
        (Direction::Left, false) => state.swayward.layout.focus_left_without_wrap(),
        (Direction::Right, false) => state.swayward.layout.focus_right_without_wrap(),
        (Direction::Up, false) => state.swayward.layout.focus_up_without_wrap(),
        (Direction::Down, false) => state.swayward.layout.focus_down_without_wrap(),
    };
    if changed {
        state.swayward.queue_redraw_all();
        None
    } else if state.swayward.layout.global_fullscreen_active()
        || direction == Direction::Left && state.swayward.layout.focused_fullscreen_mode().is_some()
        || wrapping == swayward_config::FocusWrapping::Workspace && !workspace_focused
    {
        None
    } else {
        Some(action)
    }
}

pub(super) fn output(state: &mut State, identifier: &str) -> Result<(), CommandOutcome> {
    let output = output_target_by_name_or_direction(state, identifier).map_err(failure)?;
    if let Some(output) = output {
        state.swayward.layout.focus_output(&output);
        state.swayward.queue_redraw_all();
    }
    Ok(())
}

pub(super) fn parent(state: &mut State) {
    state.swayward.layout.focus_parent();
    state.swayward.queue_redraw_all();
}

pub(super) fn child(state: &mut State) {
    state.swayward.layout.focus_child();
    state.swayward.queue_redraw_all();
}

pub(super) fn next_prev_sibling(state: &mut State, next: bool) {
    if state.swayward.layout.focus_next_prev_sibling(next) {
        state.swayward.queue_redraw_all();
    }
}

pub(super) fn next_or_prev(state: &mut State, next: bool) -> Result<(), CommandOutcome> {
    state
        .swayward
        .layout
        .focus_next_or_prev(next)
        .ok_or_else(|| failure("Expected a tiling container"))?;
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn mode(state: &mut State, floating: bool) -> Result<Action, CommandOutcome> {
    let Some(workspace) = state.swayward.layout.active_workspace() else {
        return Err(failure("Target container is not in a workspace"));
    };
    let missing = if floating {
        workspace.floating().is_empty()
    } else {
        workspace.tiling().is_empty()
    };
    if missing {
        let layer = if floating { "floating" } else { "tiling" };
        return Err(failure(format!(
            "Failed to find a {layer} container in workspace."
        )));
    }

    state.swayward.layout.disable_active_workspace_fullscreen();
    Ok(if floating {
        Action::FocusFloating
    } else {
        Action::FocusTiling
    })
}

pub(super) fn targeted(state: &mut State, target: CommandTarget) -> Result<(), CommandOutcome> {
    match target {
        CommandTarget::Window(target) => {
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return Err(failure("No matching node."));
            };
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                state.swayward.layout.show_scratchpad(Some(&window));
            } else {
                state.swayward.layout.activate_window(&window);
            }
        }
        CommandTarget::Container(workspace, node) => {
            if !state.swayward.layout.focus_tiling_node(workspace, node) {
                return Err(failure("No matching node."));
            }
        }
    }
    Ok(())
}

pub(super) fn targeted_workspace(
    state: &mut State,
    target: CommandTarget,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target_id) = target else {
        return Err(failure("No container to focus was specified."));
    };
    let window = state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target_id).then(|| mapped.window.clone()));
    let Some(window) = window else {
        return Err(failure("No matching node."));
    };
    let target_workspace = state
        .swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&window))
        .map(|(_, _, workspace)| workspace.id());
    let active_workspace = state
        .swayward
        .layout
        .active_workspace()
        .map(|workspace| workspace.id());
    let auto_back_and_forth = state
        .swayward
        .config
        .borrow()
        .input
        .workspace_auto_back_and_forth;
    if auto_back_and_forth && target_workspace == active_workspace {
        let previous = crate::command::WorkspaceTarget::BackAndForth;
        state
            .swayward
            .layout
            .activate_sway_workspace(previous)
            .map_err(failure)
    } else {
        targeted(state, target)
    }
}

pub(super) fn targeted_direction(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("directional focus requires a window target"));
    };
    let window = state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
    let Some(window) = window else {
        return Err(failure("No matching node."));
    };
    state.swayward.layout.activate_window(&window);
    state.do_action(
        match direction {
            Direction::Left => Action::FocusColumnLeft,
            Direction::Right => Action::FocusColumnRight,
            Direction::Up => Action::FocusWindowUp,
            Direction::Down => Action::FocusWindowDown,
        },
        false,
    );
    Ok(())
}

pub use swayward_ipc::command::{
    parse, parse_boolean, AssignmentTarget, BorderStyle, ClientColorClass, Command, Direction,
    Layout, LayoutToggle, LayoutToggleEntry, MovePosition, OutputTarget, ParsedCommand,
    ResizeAmount, ResizeAxis, ResizeUnit, SwapTarget, Toggle, WorkspaceTarget, XkbLayoutTarget,
};
use swayward_ipc::CommandOutcome;

mod bindings;
mod dispatch;
mod focus;
mod gaps;
mod layout;
mod movement;
mod output;
mod rules;
mod scratchpad;
mod session;
mod settings;
mod targeted;
mod window;
mod workspace;

pub use dispatch::execute;
use movement::output_target_by_name_or_direction;
#[cfg(test)]
pub(crate) use settings::{global_setting_executions, reset_global_setting_executions};
pub use targeted::run_for_window;
use targeted::tiling_target;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandTarget {
    Window(crate::window::mapped::MappedId),
    Container(
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
}

/// Result from a focused command handler.
///
/// `Ok` continues through the shared action/layout-refresh epilogue. `Err`
/// returns the outcome immediately, including the few successful outcomes
/// that intentionally skip that epilogue.
type HandlerResult = Result<Option<swayward_config::Action>, CommandOutcome>;

fn handled(result: Result<(), CommandOutcome>) -> HandlerResult {
    result.map(|()| None)
}

fn handled_outcome(outcome: CommandOutcome) -> HandlerResult {
    if outcome.success {
        Ok(None)
    } else {
        Err(outcome)
    }
}

pub(super) fn mapped_window(
    state: &crate::swayward::State,
    id: crate::window::mapped::MappedId,
) -> Option<smithay::desktop::Window> {
    state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == id).then(|| mapped.window.clone()))
}

pub(crate) fn create_output(
    headless: Option<&mut crate::backend::Headless>,
    swayward: &mut crate::swayward::Swayward,
) -> CommandOutcome {
    let Some(headless) = headless else {
        // Sway uses this when its multi-backend contains no Wayland, X11, or
        // headless backend (`sway/commands/create_output.c:12-52`). Swayward's
        // tty and single-window winit backends likewise cannot add an output.
        return failure("Can only create outputs for Wayland, X11 or headless backends");
    };
    match headless.create_output(swayward) {
        Ok(()) => success(),
        Err(error) => failure(error),
    }
}

pub(super) fn success() -> CommandOutcome {
    CommandOutcome {
        success: true,
        error: None,
        parse_error: None,
    }
}

pub(super) fn failure(error: impl Into<String>) -> CommandOutcome {
    CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: Some(false),
    }
}

#[cfg(test)]
mod tests;

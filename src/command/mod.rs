pub use swayward_ipc::command::{
    parse, parse_boolean, AssignmentTarget, BorderStyle, ClientColorClass, Command, Direction,
    Layout, LayoutToggle, LayoutToggleEntry, MovePosition, OutputTarget, ParsedCommand,
    ResizeAmount, ResizeAxis, ResizeUnit, SwapTarget, Toggle, WorkspaceTarget, XkbLayoutTarget,
};
use swayward_ipc::CommandOutcome;

mod bindings;
mod dispatch;
mod focus;
mod layout;
mod movement;
mod scratchpad;
mod settings;
mod targeted;
mod window;

pub use dispatch::execute;
use movement::output_target_by_name_or_direction;
#[cfg(test)]
pub(crate) use settings::{global_setting_executions, reset_global_setting_executions};
use swayward_ipc::legacy::PositionChange;
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

pub(super) fn command_failure(error: impl Into<String>) -> CommandOutcome {
    failure(error)
}

#[cfg(test)]
mod tests;

use swayward_ipc::CommandOutcome;

use super::{failure, CommandTarget};
use crate::swayward::State;

pub(super) fn move_focused(state: &mut State) {
    state.swayward.layout.move_to_scratchpad(None);
    state.swayward.queue_redraw_all();
}

pub(super) fn show(state: &mut State) {
    state.swayward.layout.show_scratchpad(None);
    state.swayward.queue_redraw_all();
}

pub(super) fn move_targeted(
    state: &mut State,
    target: CommandTarget,
) -> Result<(), CommandOutcome> {
    let window = target_window(state, target)?;
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
    state.swayward.layout.show_scratchpad(Some(&window));
    state.swayward.queue_redraw_all();
    Ok(())
}

fn target_window(
    state: &State,
    target: CommandTarget,
) -> Result<smithay::desktop::Window, CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("command requires a window target"));
    };
    state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()))
        .ok_or_else(|| failure("No matching node."))
}

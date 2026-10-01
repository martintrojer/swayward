use super::{failure, HandlerResult, WorkspaceTarget};
use crate::swayward::State;

pub(super) fn activate(
    state: &mut State,
    target: WorkspaceTarget,
    auto_back_and_forth: bool,
) -> HandlerResult {
    if target != WorkspaceTarget::BackAndForth {
        // Sway completes focus changes synchronously. Finish a prior
        // render-only transition before resolving the next named or numbered
        // command so its inactive empty workspace is gone.
        state.swayward.layout.finish_sway_workspace_switch(&target);
    }
    let auto_back_and_forth = auto_back_and_forth
        && state
            .swayward
            .config
            .borrow()
            .input
            .workspace_auto_back_and_forth;
    let result = if auto_back_and_forth {
        state
            .swayward
            .layout
            .activate_sway_workspace_auto_back_and_forth(target)
    } else {
        state.swayward.layout.activate_sway_workspace(target)
    };
    if let Err(error) = result {
        return Err(failure(error));
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn assign(
    state: &mut State,
    target: WorkspaceTarget,
    outputs: &[String],
) -> HandlerResult {
    if let Err(error) = state.swayward.layout.assign_sway_workspace(target, outputs) {
        return Err(failure(error));
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn rename(
    state: &mut State,
    old: Option<WorkspaceTarget>,
    new_name: String,
) -> HandlerResult {
    if let Err(error) = state.swayward.layout.rename_sway_workspace(old, new_name) {
        return Err(swayward_ipc::command::parse_error(error));
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

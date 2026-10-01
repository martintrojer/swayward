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

fn target_workspace(
    state: &State,
    target: super::CommandTarget,
) -> Option<crate::layout::workspace::WorkspaceId> {
    match target {
        super::CommandTarget::Container(workspace, _) => Some(workspace),
        super::CommandTarget::Window(id) => {
            state.swayward.layout.windows().find_map(|(_, mapped)| {
                (mapped.id() == id)
                    .then(|| state.swayward.layout.window_workspace_id(&mapped.window))
                    .flatten()
            })
        }
    }
}

pub(super) fn rename_targeted(
    state: &mut State,
    target: super::CommandTarget,
    old: Option<&WorkspaceTarget>,
    new_name: &str,
) -> super::HandlerResult {
    // Only the `rename workspace to <new>` form reads the matched container's
    // workspace. The `<old>` and `number <n>` forms resolve by name regardless
    // of criteria, and sway still runs the handler once per match, so the
    // second pass finds the old name gone and fails
    // (`sway/sway/commands/rename.c:35-58`).
    let resolved = match old {
        Some(target) => state
            .swayward
            .layout
            .rename_sway_workspace(Some(target.clone()), new_name.to_owned()),
        None => match target_workspace(state, target) {
            Some(workspace) => state
                .swayward
                .layout
                .rename_sway_workspace_by_id(workspace, new_name.to_owned()),
            // Sway's NULL workspace lands on the same message, because
            // `!workspace` is the branch that reports it
            // (`sway/sway/commands/rename.c:60-63`).
            None => Err("There is no workspace with that name".to_owned()),
        },
    };
    if let Err(error) = resolved {
        return Err(swayward_ipc::command::parse_error(error));
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

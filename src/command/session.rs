use super::{failure, HandlerResult};
use crate::swayward::State;
use crate::utils::spawning::{spawn_sh, spawn_sh_without_startup_id};

pub(super) fn reload(state: &mut State) -> HandlerResult {
    let Some(watcher) = &state.swayward.config_file_watcher else {
        return Err(failure(
            "config reload is not available without a config file watcher",
        ));
    };
    if !watcher.validate_config() {
        return Err(failure("Error(s) reloading config."));
    }
    watcher.load_config(None);
    Ok(None)
}

pub(super) fn exit(state: &mut State) -> HandlerResult {
    state.request_stop("exit");
    Ok(None)
}

pub(super) fn exec(state: &mut State, command: String, no_startup_id: bool) -> HandlerResult {
    let (token, _) = state.swayward.activation_state.create_external_token(None);
    if no_startup_id {
        spawn_sh_without_startup_id(command, Some(token.clone()));
    } else {
        spawn_sh(command, Some(token.clone()));
    }
    Ok(None)
}

use super::*;

impl State {
    pub(super) fn do_session_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::SwayCommand(command) => {
                let _ = crate::command::execute(self, &command);
            }
            Action::Quit(skip_confirmation) => {
                if !skip_confirmation && self.swayward.exit_confirm_dialog.show() {
                    self.swayward.queue_redraw_all();
                    return None;
                }

                info!("quitting as requested");
                self.request_stop("exit")
            }
            Action::ChangeVt(vt) => {
                self.backend.change_vt(vt);
                // Changing VT may not deliver the key releases, so clear the state.
                self.swayward.suppressed_keys.clear();
            }
            Action::Suspend => {
                self.backend.suspend();
                // Suspend may not deliver the key releases, so clear the state.
                self.swayward.suppressed_keys.clear();
            }
            Action::PowerOffMonitors => {
                self.swayward.deactivate_monitors(&mut self.backend);
            }
            Action::PowerOnMonitors => {
                self.swayward.activate_monitors(&mut self.backend);
            }
            Action::ToggleDebugTint => {
                self.backend.toggle_debug_tint();
                self.swayward.queue_redraw_all();
            }
            Action::DebugToggleOpaqueRegions => {
                self.swayward.debug_draw_opaque_regions = !self.swayward.debug_draw_opaque_regions;
                self.swayward.queue_redraw_all();
            }
            Action::DebugToggleDamage => {
                self.swayward.debug_toggle_damage();
            }
            Action::Spawn(command) => {
                let (token, _) = self.swayward.activation_state.create_external_token(None);
                spawn(command, Some(token.clone()));
            }
            Action::SpawnSh(command) => {
                let (token, _) = self.swayward.activation_state.create_external_token(None);
                spawn_sh(command, Some(token.clone()));
            }
            Action::DoScreenTransition(delay_ms) => {
                self.backend.with_primary_renderer(|renderer| {
                    self.swayward.do_screen_transition(renderer, delay_ms);
                });
            }
            action => return Some(action),
        }
        None
    }
}

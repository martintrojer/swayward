use super::*;

impl State {
    pub(super) fn do_mru_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::MruConfirm => {
                self.confirm_mru();
            }
            Action::MruCancel => {
                self.swayward.cancel_mru();
            }
            Action::MruAdvance {
                direction,
                scope,
                filter,
            } => {
                if self.swayward.window_mru_ui.is_open() {
                    self.swayward.window_mru_ui.advance(direction, filter);
                    self.swayward.queue_redraw_mru_output();
                } else if self.swayward.config.borrow().recent_windows.on {
                    self.swayward.mru_apply_keyboard_commit();

                    let config = self.swayward.config.borrow();
                    let scope = scope.unwrap_or(self.swayward.window_mru_ui.scope());

                    let mut wmru = WindowMru::new(&self.swayward);
                    if !wmru.is_empty() {
                        wmru.set_scope(scope);
                        if let Some(filter) = filter {
                            wmru.set_filter(filter);
                        }

                        if let Some(output) = self.swayward.layout.active_output() {
                            self.swayward.window_mru_ui.open(
                                self.swayward.clock.clone(),
                                wmru,
                                output.clone(),
                            );

                            // Only select the *next* window if some window (which should be the
                            // first one) is already focused. If nothing is focused, keep the first
                            // window (which is logically the "previously selected" one).
                            let keep_first = direction == MruDirection::Forward
                                && self.swayward.layout.focus().is_none();
                            if !keep_first {
                                self.swayward.window_mru_ui.advance(direction, None);
                            }

                            drop(config);
                            self.swayward.queue_redraw_all();
                        }
                    }
                }
            }
            Action::MruCloseCurrentWindow => {
                if self.swayward.window_mru_ui.is_open() {
                    if let Some(id) = self.swayward.window_mru_ui.current_window_id() {
                        if let Some(w) = self.swayward.find_window_by_id(id) {
                            if let Some(tl) = w.toplevel() {
                                tl.send_close();
                            }
                        }
                    }
                }
            }
            Action::MruFirst => {
                if self.swayward.window_mru_ui.is_open() {
                    self.swayward.window_mru_ui.first();
                    self.swayward.queue_redraw_mru_output();
                }
            }
            Action::MruLast => {
                if self.swayward.window_mru_ui.is_open() {
                    self.swayward.window_mru_ui.last();
                    self.swayward.queue_redraw_mru_output();
                }
            }
            Action::MruSetScope(scope) => {
                if self.swayward.window_mru_ui.is_open() {
                    self.swayward.window_mru_ui.set_scope(scope);
                    self.swayward.queue_redraw_mru_output();
                }
            }
            Action::MruCycleScope => {
                if self.swayward.window_mru_ui.is_open() {
                    self.swayward.window_mru_ui.cycle_scope();
                    self.swayward.queue_redraw_mru_output();
                }
            }
            action => return Some(action),
        }
        None
    }
}

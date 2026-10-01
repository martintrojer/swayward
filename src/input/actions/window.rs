use super::*;

impl State {
    pub(super) fn do_window_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::ToggleKeyboardShortcutsInhibit => {
                if let Some(inhibitor) =
                    self.swayward.keyboard_focus.surface().and_then(|surface| {
                        self.swayward
                            .keyboard_shortcuts_inhibiting_surfaces
                            .get(surface)
                    })
                {
                    if inhibitor.is_active() {
                        inhibitor.inactivate();
                    } else {
                        inhibitor.activate();
                    }
                }
            }
            Action::CloseWindow => {
                if let Some(mapped) = self.swayward.layout.focus() {
                    mapped.toplevel().send_close();
                }
            }
            Action::CloseWindowById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                if let Some((_, mapped)) = window {
                    mapped.toplevel().send_close();
                }
            }
            Action::FullscreenWindow => {
                let focus = self.swayward.layout.focus().map(|m| m.window.clone());
                if let Some(window) = focus {
                    self.swayward.layout.toggle_fullscreen(&window);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::ToggleWindowedFullscreen => {
                let focus = self.swayward.layout.focus().map(|m| m.window.clone());
                if let Some(window) = focus {
                    self.swayward.layout.toggle_windowed_fullscreen(&window);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::SetColumnWidth(change) => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.set_width(change);

                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                } else {
                    self.swayward.layout.set_focused_width(change);
                }
            }
            Action::SetWindowWidth(change) => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.set_width(change);

                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                } else {
                    self.swayward.layout.set_window_width(None, change);
                }
            }
            Action::SetWindowHeight(change) => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.set_height(change);

                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                } else {
                    self.swayward.layout.set_window_height(None, change);
                }
            }
            Action::ResetWindowHeight => {
                self.swayward.layout.reset_window_height(None);
            }
            Action::ExpandColumnToAvailableWidth => {
                self.swayward.layout.expand_focused_to_available_width();
            }
            Action::ShowHotkeyOverlay => {
                if self.swayward.hotkey_overlay.show() {
                    self.swayward.queue_redraw_all();

                    #[cfg(feature = "dbus")]
                    self.swayward.a11y_announce_hotkey_overlay();
                }
            }
            Action::ToggleWindowFloating => {
                self.swayward.layout.toggle_window_floating(None);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowToFloating => {
                self.swayward.layout.set_window_floating(None, true);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowToTiling => {
                self.swayward.layout.set_window_floating(None, false);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusFloating => {
                self.swayward.layout.focus_floating();
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusTiling => {
                self.swayward.layout.focus_tiling();
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::SwitchFocusBetweenFloatingAndTiling => {
                self.swayward.layout.switch_focus_floating_tiling();
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ToggleWindowRuleOpacity => {
                let active_window = self
                    .swayward
                    .layout
                    .active_workspace_mut()
                    .and_then(|ws| ws.active_window_mut());
                if let Some(window) = active_window {
                    if window.rules().opacity.is_some_and(|o| o != 1.) {
                        window.toggle_ignore_opacity_window_rule();
                        // FIXME: granular
                        self.swayward.queue_redraw_all();
                    }
                }
            }
            action => return Some(action),
        }
        None
    }
}

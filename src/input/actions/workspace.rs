use super::*;

impl State {
    pub(super) fn do_workspace_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::MoveWindowDownOrToWorkspaceDown => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_down();
                } else {
                    self.swayward.layout.move_down_or_to_workspace_down();
                    self.maybe_warp_cursor_to_focus();
                }
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowUpOrToWorkspaceUp => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_up();
                } else {
                    self.swayward.layout.move_up_or_to_workspace_up();
                    self.maybe_warp_cursor_to_focus();
                }
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowOrWorkspaceDown => {
                self.swayward.layout.focus_window_or_workspace_down();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowOrWorkspaceUp => {
                self.swayward.layout.focus_window_or_workspace_up();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowToWorkspaceDown(focus) => {
                self.swayward.layout.move_to_workspace_down(focus);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowToWorkspaceUp(focus) => {
                self.swayward.layout.move_to_workspace_up(focus);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowToWorkspace(reference, focus) => {
                if let Some((mut output, index)) =
                    self.swayward.find_output_and_workspace_index(reference)
                {
                    // The source output is always the active output, so if the target output is
                    // also the active output, we don't need to use move_to_output().
                    if let Some(active) = self.swayward.layout.active_output() {
                        if output.as_ref() == Some(active) {
                            output = None;
                        }
                    }

                    let activate = if focus {
                        ActivateWindow::Smart
                    } else {
                        ActivateWindow::No
                    };

                    if let Some(output) = output {
                        self.swayward
                            .layout
                            .move_to_output(None, &output, Some(index), activate);

                        if focus {
                            if !self.maybe_warp_cursor_to_focus_centered() {
                                self.move_cursor_to_output(&output);
                            }
                        } else {
                            self.maybe_warp_cursor_to_focus();
                        }
                    } else {
                        self.swayward
                            .layout
                            .move_to_workspace(None, index, activate);
                        self.maybe_warp_cursor_to_focus();
                    }

                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::MoveColumnToWorkspaceDown(focus) => {
                self.swayward.layout.move_focused_to_workspace_down(focus);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveColumnToWorkspaceUp(focus) => {
                self.swayward.layout.move_focused_to_workspace_up(focus);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveColumnToWorkspace(reference, focus) => {
                if let Some((mut output, index)) =
                    self.swayward.find_output_and_workspace_index(reference)
                {
                    if let Some(active) = self.swayward.layout.active_output() {
                        if output.as_ref() == Some(active) {
                            output = None;
                        }
                    }

                    if let Some(output) = output {
                        self.swayward
                            .layout
                            .move_focused_to_output(&output, Some(index), focus);
                        if focus && !self.maybe_warp_cursor_to_focus_centered() {
                            self.move_cursor_to_output(&output);
                        }
                    } else {
                        self.swayward.layout.move_focused_to_workspace(index, focus);
                        if focus {
                            self.maybe_warp_cursor_to_focus();
                        }
                    }

                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::FocusWorkspaceDown => {
                // The overview shows the whole stack at once, so the ends are
                // visible and stopping at them reads as a dead key. Wrap there,
                // matching sway's own `workspace next`.
                if self.swayward.layout.is_overview_open() {
                    self.swayward.layout.switch_workspace_down_wrapping();
                } else {
                    self.swayward.layout.switch_workspace_down();
                }
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWorkspaceDownUnderMouse => {
                if let Some(output) = self.swayward.output_under_cursor() {
                    if let Some(mon) = self.swayward.layout.monitor_for_output_mut(&output) {
                        mon.switch_workspace_down();
                        self.maybe_warp_cursor_to_focus();
                        self.swayward.layer_shell_on_demand_focus = None;
                        self.swayward.queue_redraw(&output);
                    }
                }
            }
            Action::FocusWorkspaceUp => {
                // See FocusWorkspaceDown.
                if self.swayward.layout.is_overview_open() {
                    self.swayward.layout.switch_workspace_up_wrapping();
                } else {
                    self.swayward.layout.switch_workspace_up();
                }
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWorkspaceUpUnderMouse => {
                if let Some(output) = self.swayward.output_under_cursor() {
                    if let Some(mon) = self.swayward.layout.monitor_for_output_mut(&output) {
                        mon.switch_workspace_up();
                        self.maybe_warp_cursor_to_focus();
                        self.swayward.layer_shell_on_demand_focus = None;
                        self.swayward.queue_redraw(&output);
                    }
                }
            }
            Action::FocusWorkspace(reference) => {
                if let Some((mut output, index)) =
                    self.swayward.find_output_and_workspace_index(reference)
                {
                    if let Some(active) = self.swayward.layout.active_output() {
                        if output.as_ref() == Some(active) {
                            output = None;
                        }
                    }

                    if let Some(output) = output {
                        self.swayward.layout.focus_output(&output);
                        self.swayward.layout.switch_workspace(index);
                        if !self.maybe_warp_cursor_to_focus_centered() {
                            self.move_cursor_to_output(&output);
                        }
                    } else {
                        let config = &self.swayward.config;
                        if config.borrow().input.workspace_auto_back_and_forth {
                            self.swayward
                                .layout
                                .switch_workspace_auto_back_and_forth(index);
                        } else {
                            self.swayward.layout.switch_workspace(index);
                        }
                        self.maybe_warp_cursor_to_focus();
                    }
                    self.swayward.layer_shell_on_demand_focus = None;

                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::FocusWorkspacePrevious => {
                self.swayward.layout.switch_workspace_previous();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWorkspaceDown => {
                self.swayward.layout.move_workspace_down();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWorkspaceUp => {
                self.swayward.layout.move_workspace_up();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWorkspaceToIndex(new_idx) => {
                let new_idx = new_idx.saturating_sub(1);
                self.swayward.layout.move_workspace_to_idx(None, new_idx);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::SetWorkspaceName(name) => {
                self.swayward.layout.set_workspace_name(name, None);
            }
            Action::UnsetWorkspaceName => {
                self.swayward.layout.unset_workspace_name(None);
            }
            action => return Some(action),
        }
        None
    }
}

use super::*;

impl State {
    pub(super) fn do_monitor_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::MoveColumnLeftOrToMonitorLeft => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_left();
                } else if let Some(output) = self.swayward.output_left() {
                    if self.swayward.layout.move_left_or_to_output(&output)
                        && !self.maybe_warp_cursor_to_focus_centered()
                    {
                        self.move_cursor_to_output(&output);
                    } else {
                        self.maybe_warp_cursor_to_focus();
                    }
                } else {
                    self.swayward.layout.move_left();
                    self.maybe_warp_cursor_to_focus();
                }

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveColumnRightOrToMonitorRight => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_right();
                } else if let Some(output) = self.swayward.output_right() {
                    if self.swayward.layout.move_right_or_to_output(&output)
                        && !self.maybe_warp_cursor_to_focus_centered()
                    {
                        self.move_cursor_to_output(&output);
                    } else {
                        self.maybe_warp_cursor_to_focus();
                    }
                } else {
                    self.swayward.layout.move_right();
                    self.maybe_warp_cursor_to_focus();
                }

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowOrMonitorUp => {
                if let Some(output) = self.swayward.adjacent_output_up() {
                    if self.swayward.layout.focus_window_up_or_output(&output)
                        && !self.maybe_warp_cursor_to_focus_centered()
                    {
                        self.move_cursor_to_output(&output);
                    } else {
                        self.maybe_warp_cursor_to_focus();
                    }
                } else {
                    self.swayward.layout.focus_up();
                    self.maybe_warp_cursor_to_focus();
                }
                self.swayward.layer_shell_on_demand_focus = None;

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowOrMonitorDown => {
                if let Some(output) = self.swayward.adjacent_output_down() {
                    if self.swayward.layout.focus_window_down_or_output(&output)
                        && !self.maybe_warp_cursor_to_focus_centered()
                    {
                        self.move_cursor_to_output(&output);
                    } else {
                        self.maybe_warp_cursor_to_focus();
                    }
                } else {
                    self.swayward.layout.focus_down();
                    self.maybe_warp_cursor_to_focus();
                }
                self.swayward.layer_shell_on_demand_focus = None;

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnOrMonitorLeft => {
                if let Some(output) = self.swayward.adjacent_output_left() {
                    if self.swayward.layout.focus_left_or_output(&output)
                        && !self.maybe_warp_cursor_to_focus_centered()
                    {
                        self.move_cursor_to_output(&output);
                    } else {
                        self.maybe_warp_cursor_to_focus();
                    }
                } else {
                    self.swayward.layout.focus_left();
                    self.maybe_warp_cursor_to_focus();
                }
                self.swayward.layer_shell_on_demand_focus = None;

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnOrMonitorRight => {
                if let Some(output) = self.swayward.adjacent_output_right() {
                    if self.swayward.layout.focus_right_or_output(&output)
                        && !self.maybe_warp_cursor_to_focus_centered()
                    {
                        self.move_cursor_to_output(&output);
                    } else {
                        self.maybe_warp_cursor_to_focus();
                    }
                } else {
                    self.swayward.layout.focus_right();
                    self.maybe_warp_cursor_to_focus();
                }
                self.swayward.layer_shell_on_demand_focus = None;

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusMonitorLeft => {
                if let Some(output) = self.swayward.output_left() {
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                    self.swayward.layer_shell_on_demand_focus = None;
                }
            }
            Action::FocusMonitorRight => {
                if let Some(output) = self.swayward.output_right() {
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                    self.swayward.layer_shell_on_demand_focus = None;
                }
            }
            Action::FocusMonitorDown => {
                if let Some(output) = self.swayward.output_down() {
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                    self.swayward.layer_shell_on_demand_focus = None;
                }
            }
            Action::FocusMonitorUp => {
                if let Some(output) = self.swayward.output_up() {
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                    self.swayward.layer_shell_on_demand_focus = None;
                }
            }
            Action::FocusMonitorPrevious => {
                if let Some(output) = self.swayward.output_previous() {
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                    self.swayward.layer_shell_on_demand_focus = None;
                }
            }
            Action::FocusMonitorNext => {
                if let Some(output) = self.swayward.output_next() {
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                    self.swayward.layer_shell_on_demand_focus = None;
                }
            }
            Action::FocusMonitor(output) => {
                if let Some(output) = self.swayward.output_by_name_match(&output).cloned() {
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                    self.swayward.layer_shell_on_demand_focus = None;
                }
            }
            Action::MoveWindowToMonitorLeft => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_left_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_left() {
                    self.swayward
                        .layout
                        .move_to_output(None, &output, None, ActivateWindow::Smart);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWindowToMonitorRight => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_right_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_right() {
                    self.swayward
                        .layout
                        .move_to_output(None, &output, None, ActivateWindow::Smart);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWindowToMonitorDown => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_down_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_down() {
                    self.swayward
                        .layout
                        .move_to_output(None, &output, None, ActivateWindow::Smart);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWindowToMonitorUp => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_up_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_up() {
                    self.swayward
                        .layout
                        .move_to_output(None, &output, None, ActivateWindow::Smart);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWindowToMonitorPrevious => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_previous_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_previous() {
                    self.swayward
                        .layout
                        .move_to_output(None, &output, None, ActivateWindow::Smart);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWindowToMonitorNext => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_next_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_next() {
                    self.swayward
                        .layout
                        .move_to_output(None, &output, None, ActivateWindow::Smart);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWindowToMonitor(output) => {
                if let Some(output) = self.swayward.output_by_name_match(&output).cloned() {
                    if self.swayward.screenshot_ui.is_open() {
                        self.move_cursor_to_output(&output);
                        self.swayward.screenshot_ui.move_to_output(output);
                    } else {
                        self.swayward.layout.move_to_output(
                            None,
                            &output,
                            None,
                            ActivateWindow::Smart,
                        );
                        self.swayward.layout.focus_output(&output);
                        if !self.maybe_warp_cursor_to_focus_centered() {
                            self.move_cursor_to_output(&output);
                        }
                    }
                }
            }
            Action::MoveColumnToMonitorLeft => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_left_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_left() {
                    self.swayward
                        .layout
                        .move_focused_to_output(&output, None, true);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveColumnToMonitorRight => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_right_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_right() {
                    self.swayward
                        .layout
                        .move_focused_to_output(&output, None, true);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveColumnToMonitorDown => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_down_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_down() {
                    self.swayward
                        .layout
                        .move_focused_to_output(&output, None, true);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveColumnToMonitorUp => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_up_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_up() {
                    self.swayward
                        .layout
                        .move_focused_to_output(&output, None, true);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveColumnToMonitorPrevious => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_previous_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_previous() {
                    self.swayward
                        .layout
                        .move_focused_to_output(&output, None, true);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveColumnToMonitorNext => {
                if let Some(current_output) = self.swayward.screenshot_ui.selection_output() {
                    if let Some(target_output) = self.swayward.output_next_of(current_output) {
                        self.move_cursor_to_output(&target_output);
                        self.swayward.screenshot_ui.move_to_output(target_output);
                    }
                } else if let Some(output) = self.swayward.output_next() {
                    self.swayward
                        .layout
                        .move_focused_to_output(&output, None, true);
                    self.swayward.layout.focus_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveColumnToMonitor(output) => {
                if let Some(output) = self.swayward.output_by_name_match(&output).cloned() {
                    if self.swayward.screenshot_ui.is_open() {
                        self.move_cursor_to_output(&output);
                        self.swayward.screenshot_ui.move_to_output(output);
                    } else {
                        self.swayward
                            .layout
                            .move_focused_to_output(&output, None, true);
                        self.swayward.layout.focus_output(&output);
                        if !self.maybe_warp_cursor_to_focus_centered() {
                            self.move_cursor_to_output(&output);
                        }
                    }
                }
            }
            Action::MoveWorkspaceToMonitorLeft => {
                if let Some(output) = self.swayward.output_left() {
                    self.swayward.layout.move_workspace_to_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWorkspaceToMonitorRight => {
                if let Some(output) = self.swayward.output_right() {
                    self.swayward.layout.move_workspace_to_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWorkspaceToMonitorDown => {
                if let Some(output) = self.swayward.output_down() {
                    self.swayward.layout.move_workspace_to_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWorkspaceToMonitorUp => {
                if let Some(output) = self.swayward.output_up() {
                    self.swayward.layout.move_workspace_to_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWorkspaceToMonitorPrevious => {
                if let Some(output) = self.swayward.output_previous() {
                    self.swayward.layout.move_workspace_to_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWorkspaceToMonitorNext => {
                if let Some(output) = self.swayward.output_next() {
                    self.swayward.layout.move_workspace_to_output(&output);
                    if !self.maybe_warp_cursor_to_focus_centered() {
                        self.move_cursor_to_output(&output);
                    }
                }
            }
            Action::MoveWorkspaceToMonitor(new_output) => {
                if let Some(new_output) = self.swayward.output_by_name_match(&new_output).cloned() {
                    if self.swayward.layout.move_workspace_to_output(&new_output)
                        && !self.maybe_warp_cursor_to_focus_centered()
                    {
                        self.move_cursor_to_output(&new_output);
                    }
                }
            }
            action => return Some(action),
        }
        None
    }
}

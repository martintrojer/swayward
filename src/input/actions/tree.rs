use super::*;

impl State {
    pub(super) fn do_tree_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::FocusWindowInColumn(index) => {
                self.swayward.layout.focus_window_in_parent(index);
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowPrevious => {
                let current = self.swayward.layout.focus().map(|win| win.id());
                if let Some(window) = self
                    .swayward
                    .layout
                    .windows()
                    .map(|(_, win)| win)
                    .filter(|win| Some(win.id()) != current)
                    .max_by_key(|win| win.get_focus_timestamp())
                    .map(|win| win.window.clone())
                {
                    // Commit current focus so repeated focus-window-previous works as expected.
                    self.swayward.mru_apply_keyboard_commit();

                    self.focus_window(&window);
                }
            }
            Action::SwitchLayout(action) => {
                let Some(keyboard) = &self.swayward.seat.get_keyboard() else {
                    warn!("cannot switch layout: the seat has no keyboard");
                    return None;
                };
                keyboard.with_xkb_state(self, |mut state| match action {
                    LayoutSwitchTarget::Next => state.cycle_next_layout(),
                    LayoutSwitchTarget::Prev => state.cycle_prev_layout(),
                    LayoutSwitchTarget::Index(layout) => {
                        let num_layouts = state.xkb().lock().unwrap().layouts().count();
                        if usize::from(layout) >= num_layouts {
                            warn!("requested layout doesn't exist")
                        } else {
                            state.set_layout(Layout(layout.into()))
                        }
                    }
                });
            }
            Action::MoveColumnLeft => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_left();
                } else {
                    let id = self.focused_view_id();
                    let moved = self.swayward.layout.move_left();
                    self.maybe_warp_cursor_to_focus();
                    self.emit_window_move(moved, id);
                }

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveColumnRight => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_right();
                } else {
                    let id = self.focused_view_id();
                    let moved = self.swayward.layout.move_right();
                    self.maybe_warp_cursor_to_focus();
                    self.emit_window_move(moved, id);
                }

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveColumnToFirst => {
                self.swayward.layout.move_focused_root_child_to_first();
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveColumnToLast => {
                self.swayward.layout.move_focused_root_child_to_last();
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowDown => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_down();
                } else {
                    let id = self.focused_view_id();
                    let moved = self.swayward.layout.move_down();
                    self.maybe_warp_cursor_to_focus();
                    self.emit_window_move(moved, id);
                }

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowUp => {
                if self.swayward.screenshot_ui.is_open() {
                    self.swayward.screenshot_ui.move_up();
                } else {
                    let id = self.focused_view_id();
                    let moved = self.swayward.layout.move_up();
                    self.maybe_warp_cursor_to_focus();
                    self.emit_window_move(moved, id);
                }

                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ConsumeOrExpelWindowLeft => {
                self.swayward.layout.nest_or_unnest_window_left(None);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ConsumeOrExpelWindowRight => {
                self.swayward.layout.nest_or_unnest_window_right(None);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnLeft => {
                self.swayward.layout.focus_left();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnLeftUnderMouse => {
                if let Some((output, ws)) = self.swayward.workspace_under_cursor(true) {
                    let ws_id = ws.id();
                    let ws = {
                        let mut workspaces = self.swayward.layout.workspaces_mut();
                        workspaces.find(|ws| ws.id() == ws_id).unwrap()
                    };
                    ws.focus_left();
                    self.maybe_warp_cursor_to_focus();
                    self.swayward.layer_shell_on_demand_focus = None;
                    self.swayward.queue_redraw(&output);
                }
            }
            Action::FocusColumnRight => {
                self.swayward.layout.focus_right();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnRightUnderMouse => {
                if let Some((output, ws)) = self.swayward.workspace_under_cursor(true) {
                    let ws_id = ws.id();
                    let ws = {
                        let mut workspaces = self.swayward.layout.workspaces_mut();
                        workspaces.find(|ws| ws.id() == ws_id).unwrap()
                    };
                    ws.focus_right();
                    self.maybe_warp_cursor_to_focus();
                    self.swayward.layer_shell_on_demand_focus = None;
                    self.swayward.queue_redraw(&output);
                }
            }
            Action::FocusColumnFirst => {
                self.swayward.layout.focus_first_root_child();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnLast => {
                self.swayward.layout.focus_last_root_child();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnRightOrFirst => {
                self.swayward.layout.focus_right_or_first_root_child();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumnLeftOrLast => {
                self.swayward.layout.focus_left_or_last_root_child();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusColumn(index) => {
                self.swayward.layout.focus_root_child(index);
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowDown => {
                self.swayward.layout.focus_down();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowUp => {
                self.swayward.layout.focus_up();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowDownOrColumnLeft => {
                self.swayward.layout.focus_down_or_left();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowDownOrColumnRight => {
                self.swayward.layout.focus_down_or_right();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowUpOrColumnLeft => {
                self.swayward.layout.focus_up_or_left();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowUpOrColumnRight => {
                self.swayward.layout.focus_up_or_right();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowTop => {
                self.swayward.layout.focus_window_top();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowBottom => {
                self.swayward.layout.focus_window_bottom();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowDownOrTop => {
                self.swayward.layout.focus_window_down_or_top();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::FocusWindowUpOrBottom => {
                self.swayward.layout.focus_window_up_or_bottom();
                self.maybe_warp_cursor_to_focus();
                self.swayward.layer_shell_on_demand_focus = None;
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveColumnToIndex(idx) => {
                self.swayward.layout.move_focused_root_child_to_index(idx);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ConsumeWindowIntoColumn => {
                self.swayward.layout.nest_focused_window();
                // This does not cause immediate focus or window size change, so warping mouse to
                // focus won't do anything here.
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ExpelWindowFromColumn => {
                self.swayward.layout.unnest_focused_window();
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::SwapWindowRight => {
                self.swayward.layout.swap_window_horizontal(true);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::SwapWindowLeft => {
                self.swayward.layout.swap_window_horizontal(false);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ToggleColumnTabbedDisplay => {
                self.swayward.layout.toggle_focused_tabbed_display();
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::SetColumnDisplay(display) => {
                self.swayward.layout.set_focused_display(display);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::SwitchPresetColumnWidth => {
                self.swayward.layout.toggle_width(true);
            }
            Action::SwitchPresetColumnWidthBack => {
                self.swayward.layout.toggle_width(false);
            }
            Action::SwitchPresetWindowWidth => {
                self.swayward.layout.toggle_window_width(None, true);
            }
            Action::SwitchPresetWindowWidthBack => {
                self.swayward.layout.toggle_window_width(None, false);
            }
            Action::SwitchPresetWindowHeight => {
                self.swayward.layout.toggle_window_height(None, true);
            }
            Action::SwitchPresetWindowHeightBack => {
                self.swayward.layout.toggle_window_height(None, false);
            }
            Action::CenterColumn => {
                warn!("center-column has no sway equivalent and is not supported");
            }
            Action::CenterWindow => {
                self.swayward.layout.center_window(None);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::CenterVisibleColumns => {
                warn!("center-visible-columns has no sway equivalent and is not supported");
            }
            Action::MaximizeColumn => {
                self.swayward.layout.toggle_full_width();
            }
            Action::MaximizeWindowToEdges => {
                let focus = self.swayward.layout.focus().map(|m| m.window.clone());
                if let Some(window) = focus {
                    self.swayward.layout.toggle_maximized(&window);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            action => return Some(action),
        }
        None
    }
}

use super::*;

impl State {
    pub fn handle_bind(&mut self, bind: Bind) {
        if self.swayward.is_locked()
            && !(bind.allow_when_locked || allowed_when_locked(&bind.action))
        {
            return;
        }

        if let Some(cooldown) = bind.cooldown {
            match self.swayward.bind_cooldown_timers.entry((
                bind.key,
                bind.input_device.clone(),
                bind.group,
                bind.release,
                bind.allow_when_locked,
                bind.allow_inhibiting,
            )) {
                Entry::Occupied(_) => return,
                Entry::Vacant(entry) => {
                    let timer = Timer::from_duration(cooldown);
                    let cooldown_key = (
                        bind.key,
                        bind.input_device.clone(),
                        bind.group,
                        bind.release,
                        bind.allow_when_locked,
                        bind.allow_inhibiting,
                    );
                    let token = self
                        .swayward
                        .event_loop
                        .insert_source(timer, move |_, _, state| {
                            if state
                                .swayward
                                .bind_cooldown_timers
                                .remove(&cooldown_key)
                                .is_none()
                            {
                                error!("bind cooldown timer entry disappeared");
                            }
                            TimeoutAction::Drop
                        })
                        .unwrap();
                    entry.insert(token);
                }
            }
        }

        let event = sway_binding_event(&bind, self.backend.mod_key(&self.swayward.config.borrow()));
        let succeeded = match bind.action {
            Action::SwayCommand(command) => crate::command::execute(self, &command)
                .into_iter()
                .all(|outcome| outcome.success),
            action => {
                self.do_action(action, bind.allow_when_locked);
                false
            }
        };
        if succeeded {
            if let (Some(server), Some(event)) = (&self.swayward.ipc_server, event) {
                server.send_event(event);
            }
        }
    }

    pub(super) fn focused_view_id(&self) -> Option<i64> {
        let workspace = self.swayward.layout.active_workspace()?;
        if workspace
            .focused_container_node()
            .is_some_and(|node| workspace.is_tiling_split(node))
        {
            return None;
        }
        self.swayward
            .layout
            .focus()
            .map(|window| crate::ipc::tree::window_id(window.id()))
    }

    pub(super) fn emit_window_move(&mut self, moved: bool, id: Option<i64>) {
        if !moved {
            return;
        }
        self.ipc_refresh_layout();
        if let (Some(server), Some(id)) = (&self.swayward.ipc_server, id) {
            server.send_event(swayward_ipc::legacy::Event::WindowMoved { id });
        }
    }

    pub fn do_action(&mut self, action: Action, allow_when_locked: bool) {
        if self.swayward.is_locked() && !(allow_when_locked || allowed_when_locked(&action)) {
            return;
        }

        if let Some(touch) = self.swayward.seat.get_touch() {
            touch.cancel(self);
        }

        match action {
            Action::SwayCommand(command) => {
                let _ = crate::command::execute(self, &command);
            }
            Action::Quit(skip_confirmation) => {
                if !skip_confirmation && self.swayward.exit_confirm_dialog.show() {
                    self.swayward.queue_redraw_all();
                    return;
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
            Action::ScreenshotScreen(write_to_disk, show_pointer, path) => {
                let active = self.swayward.layout.active_output().cloned();
                if let Some(active) = active {
                    self.backend.with_primary_renderer(|renderer| {
                        if let Err(err) = self.swayward.screenshot(
                            renderer,
                            &active,
                            write_to_disk,
                            show_pointer,
                            path,
                        ) {
                            warn!("error taking screenshot: {err:?}");
                        }
                    });
                }
            }
            Action::ConfirmScreenshot { write_to_disk } => {
                self.confirm_screenshot(write_to_disk);
            }
            Action::CancelScreenshot => {
                if !self.swayward.screenshot_ui.is_open() {
                    return;
                }

                self.swayward.screenshot_ui.close();
                self.swayward
                    .cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
                self.swayward.queue_redraw_all();
            }
            Action::ScreenshotTogglePointer => {
                self.swayward.screenshot_ui.toggle_pointer();
                self.swayward.queue_redraw_all();
            }
            Action::Screenshot(show_cursor, path) => {
                self.open_screenshot_ui(show_cursor, path);
                self.swayward.cancel_mru();
            }
            Action::ScreenshotWindow(write_to_disk, show_pointer, path) => {
                let focus = self.swayward.layout.focus_with_output();
                if let Some((mapped, output)) = focus {
                    self.backend.with_primary_renderer(|renderer| {
                        if let Err(err) = self.swayward.screenshot_window(
                            renderer,
                            output,
                            mapped,
                            write_to_disk,
                            show_pointer,
                            path,
                        ) {
                            warn!("error taking screenshot: {err:?}");
                        }
                    });
                }
            }
            Action::ScreenshotWindowById {
                id,
                write_to_disk,
                show_pointer,
                path,
            } => {
                let mut windows = self.swayward.layout.windows();
                let window = windows.find(|(_, m)| m.id().get() == id);
                if let Some((Some(monitor), mapped)) = window {
                    let output = monitor.output();
                    self.backend.with_primary_renderer(|renderer| {
                        if let Err(err) = self.swayward.screenshot_window(
                            renderer,
                            output,
                            mapped,
                            write_to_disk,
                            show_pointer,
                            path,
                        ) {
                            warn!("error taking screenshot: {err:?}");
                        }
                    });
                }
            }
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
            Action::FullscreenWindowById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
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
            Action::ToggleWindowedFullscreenById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward.layout.toggle_windowed_fullscreen(&window);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::FocusWindow(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.focus_window(&window);
                }
            }
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
                let keyboard = &self.swayward.seat.get_keyboard().unwrap();
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
            Action::ConsumeOrExpelWindowLeft => {
                self.swayward.layout.nest_or_unnest_window_left(None);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ConsumeOrExpelWindowLeftById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .nest_or_unnest_window_left(Some(&window));
                    self.maybe_warp_cursor_to_focus();
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::ConsumeOrExpelWindowRight => {
                self.swayward.layout.nest_or_unnest_window_right(None);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ConsumeOrExpelWindowRightById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .nest_or_unnest_window_right(Some(&window));
                    self.maybe_warp_cursor_to_focus();
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
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
            Action::MoveWindowToWorkspaceById {
                window_id: id,
                reference,
                focus,
            } => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    if let Some((output, index)) =
                        self.swayward.find_output_and_workspace_index(reference)
                    {
                        let target_was_active = self
                            .swayward
                            .layout
                            .active_output()
                            .is_some_and(|active| output.as_ref() == Some(active));

                        let activate = if focus {
                            ActivateWindow::Smart
                        } else {
                            ActivateWindow::No
                        };

                        if let Some(output) = output {
                            self.swayward.layout.move_to_output(
                                Some(&window),
                                &output,
                                Some(index),
                                activate,
                            );

                            // If the active output changed (window was moved and focused).
                            if !target_was_active
                                && self.swayward.layout.active_output() == Some(&output)
                                && !self.maybe_warp_cursor_to_focus_centered()
                            {
                                self.move_cursor_to_output(&output);
                            }
                        } else {
                            self.swayward
                                .layout
                                .move_to_workspace(Some(&window), index, activate);

                            // If we focused the target window.
                            let new_focus = self.swayward.layout.focus();
                            if new_focus.is_some_and(|win| win.window == window) {
                                self.maybe_warp_cursor_to_focus();
                            }
                        }

                        // FIXME: granular
                        self.swayward.queue_redraw_all();
                    }
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
            Action::MoveColumnToIndex(idx) => {
                self.swayward.layout.move_focused_root_child_to_index(idx);
                self.maybe_warp_cursor_to_focus();
                // FIXME: granular
                self.swayward.queue_redraw_all();
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
            Action::MoveWorkspaceToIndexByRef { new_idx, reference } => {
                if let Some(res) = self.swayward.find_output_and_workspace_index(reference) {
                    let new_idx = new_idx.saturating_sub(1);
                    self.swayward
                        .layout
                        .move_workspace_to_idx(Some(res), new_idx);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::SetWorkspaceName(name) => {
                self.swayward.layout.set_workspace_name(name, None);
            }
            Action::SetWorkspaceNameByRef { name, reference } => {
                self.swayward
                    .layout
                    .set_workspace_name(name, Some(reference));
            }
            Action::UnsetWorkspaceName => {
                self.swayward.layout.unset_workspace_name(None);
            }
            Action::UnsetWorkSpaceNameByRef(reference) => {
                self.swayward.layout.unset_workspace_name(Some(reference));
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
            Action::SwitchPresetWindowWidthById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .toggle_window_width(Some(&window), true);
                }
            }
            Action::SwitchPresetWindowWidthBackById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .toggle_window_width(Some(&window), false);
                }
            }
            Action::SwitchPresetWindowHeight => {
                self.swayward.layout.toggle_window_height(None, true);
            }
            Action::SwitchPresetWindowHeightBack => {
                self.swayward.layout.toggle_window_height(None, false);
            }
            Action::SwitchPresetWindowHeightById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .toggle_window_height(Some(&window), true);
                }
            }
            Action::SwitchPresetWindowHeightBackById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .toggle_window_height(Some(&window), false);
                }
            }
            Action::CenterColumn => {
                warn!("center-column has no sway equivalent and is not supported");
            }
            Action::CenterWindow => {
                self.swayward.layout.center_window(None);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::CenterWindowById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward.layout.center_window(Some(&window));
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
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
            Action::MaximizeWindowToEdgesById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward.layout.toggle_maximized(&window);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
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
            Action::MoveWindowToMonitorById { id, output } => {
                if let Some(output) = self.swayward.output_by_name_match(&output).cloned() {
                    let window = self
                        .swayward
                        .layout
                        .windows()
                        .find(|(_, m)| m.id().get() == id);
                    let window = window.map(|(_, m)| m.window.clone());

                    if let Some(window) = window {
                        let target_was_active = self
                            .swayward
                            .layout
                            .active_output()
                            .is_some_and(|active| output == *active);

                        self.swayward.layout.move_to_output(
                            Some(&window),
                            &output,
                            None,
                            ActivateWindow::Smart,
                        );

                        // If the active output changed (window was moved and focused).
                        if !target_was_active
                            && self.swayward.layout.active_output() == Some(&output)
                            && !self.maybe_warp_cursor_to_focus_centered()
                        {
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
            Action::SetWindowWidthById { id, change } => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward.layout.set_window_width(Some(&window), change);
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
            Action::SetWindowHeightById { id, change } => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .set_window_height(Some(&window), change);
                }
            }
            Action::ResetWindowHeight => {
                self.swayward.layout.reset_window_height(None);
            }
            Action::ResetWindowHeightById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward.layout.reset_window_height(Some(&window));
                }
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
            Action::MoveWorkspaceToMonitorByRef {
                output_name,
                reference,
            } => {
                if let Some((output, old_idx)) =
                    self.swayward.find_output_and_workspace_index(reference)
                {
                    if let Some(new_output) =
                        self.swayward.output_by_name_match(&output_name).cloned()
                    {
                        let workspace_id = output.as_ref().and_then(|output| {
                            self.swayward.layout.workspace_id_at(output, old_idx)
                        });
                        if workspace_id.is_some_and(|workspace_id| {
                            self.swayward.layout.move_workspace_to_output_by_id(
                                workspace_id,
                                output,
                                &new_output,
                            )
                        }) {
                            // Cursor warp already calls `queue_redraw_all`
                            if !self.maybe_warp_cursor_to_focus_centered() {
                                self.move_cursor_to_output(&new_output);
                            }
                        }
                    }
                }
            }
            Action::ToggleWindowFloating => {
                self.swayward.layout.toggle_window_floating(None);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::ToggleWindowFloatingById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward.layout.toggle_window_floating(Some(&window));
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::MoveWindowToFloating => {
                self.swayward.layout.set_window_floating(None, true);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowToFloatingById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .set_window_floating(Some(&window), true);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
            }
            Action::MoveWindowToTiling => {
                self.swayward.layout.set_window_floating(None, false);
                // FIXME: granular
                self.swayward.queue_redraw_all();
            }
            Action::MoveWindowToTilingById(id) => {
                let window = self
                    .swayward
                    .layout
                    .windows()
                    .find(|(_, m)| m.id().get() == id);
                let window = window.map(|(_, m)| m.window.clone());
                if let Some(window) = window {
                    self.swayward
                        .layout
                        .set_window_floating(Some(&window), false);
                    // FIXME: granular
                    self.swayward.queue_redraw_all();
                }
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
            Action::MoveFloatingWindowById { id, x, y } => {
                let window = if let Some(id) = id {
                    let window = self
                        .swayward
                        .layout
                        .windows()
                        .find(|(_, m)| m.id().get() == id);
                    let window = window.map(|(_, m)| m.window.clone());
                    if window.is_none() {
                        return;
                    }
                    window
                } else {
                    None
                };

                self.swayward
                    .layout
                    .move_floating_window(window.as_ref(), x, y, true);
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
            Action::ToggleWindowRuleOpacityById(id) => {
                let window = self
                    .swayward
                    .layout
                    .workspaces_mut()
                    .find_map(|ws| ws.windows_mut().find(|w| w.id().get() == id));
                if let Some(window) = window {
                    if window.rules().opacity.is_some_and(|o| o != 1.) {
                        window.toggle_ignore_opacity_window_rule();
                        // FIXME: granular
                        self.swayward.queue_redraw_all();
                    }
                }
            }
            Action::SetDynamicCastWindow => {
                let id = self
                    .swayward
                    .layout
                    .active_workspace()
                    .and_then(|ws| ws.active_window())
                    .map(|mapped| mapped.id().get());
                if let Some(id) = id {
                    self.set_dynamic_cast_target(CastTarget::Window { id });
                }
            }
            Action::SetDynamicCastWindowById(id) => {
                let layout = &self.swayward.layout;
                if layout.windows().any(|(_, mapped)| mapped.id().get() == id) {
                    self.set_dynamic_cast_target(CastTarget::Window { id });
                }
            }
            Action::SetDynamicCastMonitor(output) => {
                let output = match output {
                    None => self.swayward.layout.active_output(),
                    Some(name) => self.swayward.output_by_name_match(&name),
                };
                if let Some(output) = output {
                    self.set_dynamic_cast_target(CastTarget::output(output));
                }
            }
            Action::ClearDynamicCastTarget => {
                self.set_dynamic_cast_target(CastTarget::Nothing);
            }
            Action::StopCast(session_id) => {
                self.swayward.stop_cast(CastSessionId::from(session_id));
            }
            Action::ToggleOverview => {
                // A layer surface holding on-demand focus outranks the
                // overview (Swayward::compute_focus checks Layer::Top first),
                // so clicking a bar and then opening the overview left every
                // key going to the bar and none of the overview binds firing.
                self.swayward.layer_shell_on_demand_focus = None;
                self.swayward.layout.toggle_overview();
                self.swayward.queue_redraw_all();
            }
            Action::OpenOverview => {
                if self.swayward.layout.open_overview() {
                    self.swayward.layer_shell_on_demand_focus = None;
                    self.swayward.queue_redraw_all();
                }
            }
            Action::CloseOverview => {
                if self.swayward.layout.close_overview() {
                    self.swayward.queue_redraw_all();
                }
            }
            Action::ToggleWindowUrgent(id) => {
                let window = self
                    .swayward
                    .layout
                    .workspaces_mut()
                    .find_map(|ws| ws.windows_mut().find(|w| w.id().get() == id));
                if let Some(window) = window {
                    let urgent = window.is_urgent();
                    window.set_urgent(!urgent);
                }
                self.swayward.queue_redraw_all();
            }
            Action::SetWindowUrgent(id) => {
                let window = self
                    .swayward
                    .layout
                    .workspaces_mut()
                    .find_map(|ws| ws.windows_mut().find(|w| w.id().get() == id));
                if let Some(window) = window {
                    window.set_urgent(true);
                }
                self.swayward.queue_redraw_all();
            }
            Action::UnsetWindowUrgent(id) => {
                let window = self
                    .swayward
                    .layout
                    .workspaces_mut()
                    .find_map(|ws| ws.windows_mut().find(|w| w.id().get() == id));
                if let Some(window) = window {
                    window.set_urgent(false);
                }
                self.swayward.queue_redraw_all();
            }
            Action::LoadConfigFile(path) => {
                if let Some(watcher) = &self.swayward.config_file_watcher {
                    watcher.load_config(path);
                }
            }
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
        }
    }
}

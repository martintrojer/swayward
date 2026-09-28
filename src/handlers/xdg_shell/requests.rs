use super::*;

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.swayward.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        let unmapped = Unmapped::new(Window::new_wayland_window(surface));
        let existing = self.swayward.unmapped_windows.insert(wl_surface, unmapped);
        assert!(existing.is_none());
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let popup = PopupKind::Xdg(surface);
        self.unconstrain_popup(&popup);

        if let Err(err) = self.swayward.popups.track_popup(popup) {
            warn!("error tracking popup: {err:?}");
        }
    }

    fn move_request(&mut self, surface: ToplevelSurface, _seat: WlSeat, serial: Serial) {
        let wl_surface = surface.wl_surface();

        let mut grab_start_data = None;

        // See if this comes from a pointer grab.
        let pointer = self.swayward.seat.get_pointer().unwrap();
        pointer.with_grab(|grab_serial, grab| {
            if grab_serial == serial {
                let start_data = grab.start_data();
                if let Some((focus, _)) = &start_data.focus {
                    if focus.id().same_client_as(&wl_surface.id()) {
                        // Deny move requests from DnD grabs to work around
                        // https://gitlab.gnome.org/GNOME/gtk/-/issues/7113
                        let is_dnd_grab = Self::is_dnd_grab(grab.as_any());

                        if !is_dnd_grab {
                            grab_start_data = Some(AnyStartData::Pointer(start_data.clone()));
                        }
                    }
                }
            }
        });

        // See if this comes from a touch grab.
        if let Some(touch) = self.swayward.seat.get_touch() {
            touch.with_grab(|grab_serial, grab| {
                if grab_serial == serial {
                    let start_data = grab.start_data();
                    if let Some((focus, _)) = &start_data.focus {
                        if focus.id().same_client_as(&wl_surface.id()) {
                            // Deny move requests from DnD grabs to work around
                            // https://gitlab.gnome.org/GNOME/gtk/-/issues/7113
                            let is_dnd_grab = Self::is_dnd_grab(grab.as_any());

                            if !is_dnd_grab {
                                grab_start_data = Some(AnyStartData::Touch(start_data.clone()));
                            }
                        }
                    }
                }
            });
        }

        // See if this comes from a tablet tool grab.
        let mut tablet_tool = None;
        let tools = self.swayward.seat.tablet_seat().get_tools();
        for tool in tools.values() {
            let found = tool.with_grab(|grab_serial, grab| {
                if grab_serial == serial {
                    let start_data = grab.start_data();
                    if let Some((focus, _)) = &start_data.focus {
                        if focus.id().same_client_as(&wl_surface.id()) {
                            // Deny move requests from DnD grabs to work around
                            // https://gitlab.gnome.org/GNOME/gtk/-/issues/7113
                            let is_dnd_grab = Self::is_dnd_grab(grab.as_any());

                            if !is_dnd_grab {
                                grab_start_data =
                                    Some(AnyStartData::TabletTool(start_data.clone()));
                                tablet_tool = Some(tool.clone());
                                return true;
                            }
                        }
                    }
                }
                false
            });
            if found == Some(true) {
                break;
            }
        }

        let Some(start_data) = grab_start_data else {
            return;
        };

        let Some((mapped, output)) = self.swayward.layout.find_window_and_output(wl_surface) else {
            return;
        };

        let Some(output) = output else {
            return;
        };

        if !mapped.is_floating() || mapped.pending_sizing_mode().is_fullscreen() {
            return;
        }

        let window = mapped.window.clone();
        let output = output.clone();

        match &start_data {
            AnyStartData::Pointer(_) => {
                if let Some(grab) = MoveGrab::new(self, start_data, window.clone(), true, None) {
                    pointer.set_grab(self, grab, serial, Focus::Clear);
                }
            }
            AnyStartData::Touch(_) => {
                let touch = self.swayward.seat.get_touch().unwrap();
                if let Some(grab) = MoveGrab::new(self, start_data, window.clone(), true, None) {
                    touch.set_grab(self, grab, serial);
                }
            }
            AnyStartData::TabletTool(_) => {
                if let Some(grab) = MoveGrab::new(self, start_data, window.clone(), true, None) {
                    let time = InputTime::now();
                    tablet_tool
                        .unwrap()
                        .set_grab(self, grab, time, serial, Focus::Clear);
                }
            }
        }

        self.swayward.queue_redraw(&output);
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        _seat: WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let wl_surface = surface.wl_surface();

        let mut grab_start_data = None;

        // See if this comes from a pointer grab.
        let pointer = self.swayward.seat.get_pointer().unwrap();
        if pointer.has_grab(serial) {
            if let Some(start_data) = pointer.grab_start_data() {
                if let Some((focus, _)) = &start_data.focus {
                    if focus.id().same_client_as(&wl_surface.id()) {
                        grab_start_data = Some(AnyStartData::Pointer(start_data));
                    }
                }
            }
        }

        // See if this comes from a touch grab.
        if let Some(touch) = self.swayward.seat.get_touch() {
            if touch.has_grab(serial) {
                if let Some(start_data) = touch.grab_start_data() {
                    if let Some((focus, _)) = &start_data.focus {
                        if focus.id().same_client_as(&wl_surface.id()) {
                            grab_start_data = Some(AnyStartData::Touch(start_data));
                        }
                    }
                }
            }
        }

        // See if this comes from a tablet tool grab.
        let mut tablet_tool = None;
        let tools = self.swayward.seat.tablet_seat().get_tools();
        'outer: for tool in tools.values() {
            if tool.has_grab(serial) {
                if let Some(start_data) = tool.grab_start_data() {
                    if let Some((focus, _)) = &start_data.focus {
                        if focus.id().same_client_as(&wl_surface.id()) {
                            grab_start_data = Some(AnyStartData::TabletTool(start_data));
                            tablet_tool = Some(tool.clone());
                            break 'outer;
                        }
                    }
                }
            }
        }

        let Some(start_data) = grab_start_data else {
            return;
        };

        let Some((mapped, _)) = self.swayward.layout.find_window_and_output(wl_surface) else {
            return;
        };

        if !mapped.is_floating() {
            return;
        }

        let edges = ResizeEdge::from(edges);
        let window = mapped.window.clone();

        // See if we got a double resize-click gesture.
        let time = get_monotonic_time();
        let last_cell = mapped.last_interactive_resize_start();
        let mut last = last_cell.get();
        last_cell.set(Some((time, edges)));

        // Floating windows don't have either of the double-resize-click gestures, so just allow it
        // to resize.
        if mapped.is_floating() {
            last = None;
            last_cell.set(None);
        }

        if let Some((last_time, last_edges)) = last {
            if time.saturating_sub(last_time) <= DOUBLE_CLICK_TIME {
                // Allow quick resize after a triple click.
                last_cell.set(None);

                let intersection = edges.intersection(last_edges);
                if intersection.intersects(ResizeEdge::LEFT_RIGHT) {
                    // FIXME: don't activate once we can pass specific windows to actions.
                    self.swayward.layout.activate_window(&window);
                    self.swayward.layer_shell_on_demand_focus = None;
                    self.swayward.layout.toggle_full_width();
                }
                if intersection.intersects(ResizeEdge::TOP_BOTTOM) {
                    self.swayward.layer_shell_on_demand_focus = None;
                    self.swayward.layout.reset_window_height(Some(&window));
                }
                // FIXME: granular.
                self.swayward.queue_redraw_all();
                return;
            }
        }

        if !self
            .swayward
            .layout
            .interactive_resize_begin(window.clone(), edges)
        {
            return;
        }

        match start_data {
            AnyStartData::Pointer(_) => {
                let grab = ResizeGrab::new(start_data, window);
                pointer.set_grab(self, grab, serial, Focus::Clear);
            }
            AnyStartData::Touch(_) => {
                let touch = self.swayward.seat.get_touch().unwrap();
                let grab = ResizeGrab::new(start_data, window);
                touch.set_grab(self, grab, serial);
            }
            AnyStartData::TabletTool(_) => {
                let grab = ResizeGrab::new(start_data, window);
                let time = InputTime::now();
                tablet_tool
                    .unwrap()
                    .set_grab(self, grab, time, serial, Focus::Clear);
            }
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            let geometry = positioner.get_geometry();
            state.geometry = geometry;
            state.positioner = positioner;
        });
        self.unconstrain_popup(&PopupKind::Xdg(surface.clone()));
        surface.send_repositioned(token);
    }

    fn grab(&mut self, surface: PopupSurface, _seat: WlSeat, serial: Serial) {
        let popup = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&popup) else {
            trace!("ignoring popup grab because no root surface");
            return;
        };

        // We need to hand out the grab in a way consistent with what update_keyboard_focus()
        // thinks the current focus is, otherwise it will desync and cause weird issues with
        // keyboard focus being at the wrong place.
        if self.swayward.exit_confirm_dialog.is_open() {
            trace!("ignoring popup grab because the exit confirm dialog is open");
            let _ = PopupManager::dismiss_popup(&root, &popup);
            return;
        } else if self.swayward.is_locked() {
            if Some(&root) != self.swayward.lock_surface_focus().as_ref() {
                trace!("ignoring popup grab because the session is locked");
                let _ = PopupManager::dismiss_popup(&root, &popup);
                return;
            }
        } else if self.swayward.screenshot_ui.is_open() {
            trace!("ignoring popup grab because the screenshot UI is open");
            let _ = PopupManager::dismiss_popup(&root, &popup);
            return;
        } else if let Some(output) = self.swayward.layout.active_output() {
            let layers = layer_map_for_output(output);

            // FIXME: somewhere here we probably need to check is_overview_open to match the logic
            // in update_keyboard_focus().

            if let Some(layer) = layers.layer_for_surface(&root, WindowSurfaceType::TOPLEVEL) {
                // This is a grab for a layer surface.

                if let Some(mapped) = self.swayward.mapped_layer_surfaces.get(layer) {
                    if mapped.place_within_backdrop() {
                        trace!("ignoring popup grab for a layer surface within overview backdrop");
                        let _ = PopupManager::dismiss_popup(&root, &popup);
                        return;
                    }
                }
            } else {
                // This is a grab for a regular window; check that there's no layer surface with a
                // higher input priority.

                if layers.layers_on(Layer::Overlay).any(|l| {
                    (l.cached_state().keyboard_interactivity
                        == wlr_layer::KeyboardInteractivity::Exclusive
                        || Some(l) == self.swayward.layer_shell_on_demand_focus.as_ref())
                        && self.swayward.mapped_layer_surfaces.contains_key(l)
                }) {
                    trace!("ignoring toplevel popup grab because the overlay layer has focus");
                    let _ = PopupManager::dismiss_popup(&root, &popup);
                    return;
                }

                let mon = self.swayward.layout.monitor_for_output(output).unwrap();
                if !mon.render_above_top_layer()
                    && layers.layers_on(Layer::Top).any(|l| {
                        (l.cached_state().keyboard_interactivity
                            == wlr_layer::KeyboardInteractivity::Exclusive
                            || Some(l) == self.swayward.layer_shell_on_demand_focus.as_ref())
                            && self.swayward.mapped_layer_surfaces.contains_key(l)
                    })
                {
                    trace!("ignoring toplevel popup grab because the top layer has focus");
                    let _ = PopupManager::dismiss_popup(&root, &popup);
                    return;
                }

                let layout_focus = self.swayward.layout.focus();
                if Some(&root) != layout_focus.map(|win| win.toplevel().wl_surface()) {
                    trace!("ignoring toplevel popup grab because another window has focus");
                    let _ = PopupManager::dismiss_popup(&root, &popup);
                    return;
                }
            }
        } else {
            trace!("ignoring popup grab because no output is active");
            let _ = PopupManager::dismiss_popup(&root, &popup);
            return;
        }

        let seat = &self.swayward.seat;
        let mut grab = match self
            .swayward
            .popups
            .grab_popup(root.clone(), popup, seat, serial)
        {
            Ok(grab) => grab,
            Err(err) => {
                trace!("ignoring popup grab: {err:?}");
                return;
            }
        };

        let keyboard = seat.get_keyboard().unwrap();
        let pointer = seat.get_pointer().unwrap();

        // Smithay cannot do overlapping grabs, so if we have an IME keyboard grab, don't overwrite
        // it with a popup keyboard grab. This makes the popup menu work in Telegram while an IME
        // is active (otherwise it hits the grab mismatch check below).
        //
        // The second check is for layer surfaces that can't receive keyboard focus, without it
        // popups don't work properly in Waybar (GTK 3).
        let can_receive_keyboard_focus = !self.swayward.seat.input_method().keyboard_grabbed()
            && self
                .swayward
                .layout
                .active_output()
                .and_then(|output| {
                    layer_map_for_output(output)
                        .layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)
                        .map(|layer_surface| layer_surface.can_receive_keyboard_focus())
                })
                .unwrap_or(true);

        let keyboard_grab_mismatches = keyboard.is_grabbed()
            && !(keyboard.has_grab(serial)
                || grab.previous_serial().is_none_or(|s| keyboard.has_grab(s)));
        let pointer_grab_mismatches = pointer.is_grabbed()
            && !(pointer.has_grab(serial)
                || grab.previous_serial().is_none_or(|s| pointer.has_grab(s)));
        if (can_receive_keyboard_focus && keyboard_grab_mismatches) || pointer_grab_mismatches {
            trace!("ignoring popup grab because of current grab mismatch");
            grab.ungrab(PopupUngrabStrategy::All);
            return;
        }

        trace!("new grab for root {:?}", root);
        if can_receive_keyboard_focus {
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
        self.swayward.popup_grab = Some(PopupGrabState {
            root,
            grab,
            has_keyboard_grab: can_receive_keyboard_focus,
        });
    }

    fn maximize_request(&mut self, toplevel: ToplevelSurface) {
        if let Some((mapped, _)) = self
            .swayward
            .layout
            .find_window_and_output_mut(toplevel.wl_surface())
        {
            // A configure is required in response to this event regardless if there are pending
            // changes.
            mapped.set_needs_configure();

            let window = mapped.window.clone();
            self.swayward.layout.set_maximized(&window, true);
        } else if let Some(unmapped) = self
            .swayward
            .unmapped_windows
            .get_mut(toplevel.wl_surface())
        {
            match &mut unmapped.state {
                InitialConfigureState::NotConfigured {
                    wants_maximized, ..
                } => {
                    *wants_maximized = true;

                    // The required configure will be the initial configure.
                }
                InitialConfigureState::Configured {
                    rules,
                    output,
                    is_pending_maximized,
                    ..
                } => {
                    // Figure out the monitor following a similar logic to initial configure.
                    // FIXME: deduplicate.
                    let mon = output
                        .as_ref()
                        .and_then(|o| self.swayward.layout.monitor_for_output(o))
                        .map(|mon| (mon, false))
                        // If not, check if we have a parent with a monitor.
                        .or_else(|| {
                            toplevel
                                .parent()
                                .and_then(|parent| {
                                    self.swayward.layout.find_window_and_output(&parent)
                                })
                                .and_then(|(_win, output)| output)
                                .and_then(|o| self.swayward.layout.monitor_for_output(o))
                                .map(|mon| (mon, true))
                        })
                        // If not, fall back to the active monitor.
                        .or_else(|| {
                            self.swayward
                                .layout
                                .active_monitor_ref()
                                .map(|mon| (mon, false))
                        });

                    *output = mon
                        .filter(|(_, parent)| !parent)
                        .map(|(mon, _)| mon.output().clone());
                    let mon = mon.map(|(mon, _)| mon);

                    let ws = mon
                        .map(|mon| mon.active_workspace_ref())
                        .or_else(|| self.swayward.layout.active_workspace());

                    if let Some(ws) = ws {
                        // If the window is pending fullscreen, then this will do nothing. But
                        // that's expected: the window remains fullscreen, and we simply remember
                        // that it is now pending maximized.
                        *is_pending_maximized = true;
                        toplevel.with_pending_state(|state| {
                            if !state.states.contains(xdg_toplevel::State::Fullscreen) {
                                state.states.set(xdg_toplevel::State::Maximized);
                            }
                        });
                        ws.configure_new_window(&unmapped.window, None, None, false, rules);
                    }

                    // We already sent the initial configure, so we need to reconfigure.
                    toplevel.send_configure();
                }
            }
        } else {
            error!("couldn't find the toplevel in maximize_request()");
            toplevel.send_configure();
        }
    }

    fn unmaximize_request(&mut self, toplevel: ToplevelSurface) {
        if let Some((mapped, _)) = self
            .swayward
            .layout
            .find_window_and_output_mut(toplevel.wl_surface())
        {
            // A configure is required in response to this event regardless if there are pending
            // changes.
            mapped.set_needs_configure();

            let window = mapped.window.clone();
            self.swayward.layout.set_maximized(&window, false);
        } else if let Some(unmapped) = self
            .swayward
            .unmapped_windows
            .get_mut(toplevel.wl_surface())
        {
            match &mut unmapped.state {
                InitialConfigureState::NotConfigured {
                    wants_maximized, ..
                } => {
                    *wants_maximized = false;

                    // The required configure will be the initial configure.
                }
                InitialConfigureState::Configured {
                    rules,
                    width,
                    height,
                    floating_width,
                    floating_height,
                    is_full_width,
                    output,
                    workspace_name,
                    is_pending_maximized,
                } => {
                    // Figure out the monitor following a similar logic to initial configure.
                    // FIXME: deduplicate.
                    let mon = workspace_name
                        .as_deref()
                        .and_then(|name| self.swayward.layout.monitor_for_workspace(name))
                        .map(|mon| (mon, false));

                    let mon = mon.or_else(|| {
                        output
                            .as_ref()
                            .and_then(|o| self.swayward.layout.monitor_for_output(o))
                            .map(|mon| (mon, false))
                            // If not, check if we have a parent with a monitor.
                            .or_else(|| {
                                toplevel
                                    .parent()
                                    .and_then(|parent| {
                                        self.swayward.layout.find_window_and_output(&parent)
                                    })
                                    .and_then(|(_win, output)| output)
                                    .and_then(|o| self.swayward.layout.monitor_for_output(o))
                                    .map(|mon| (mon, true))
                            })
                            // If not, fall back to the active monitor.
                            .or_else(|| {
                                self.swayward
                                    .layout
                                    .active_monitor_ref()
                                    .map(|mon| (mon, false))
                            })
                    });

                    *output = mon
                        .filter(|(_, parent)| !parent)
                        .map(|(mon, _)| mon.output().clone());
                    let mon = mon.map(|(mon, _)| mon);

                    let ws = workspace_name
                        .as_deref()
                        .and_then(|name| mon.map(|mon| mon.find_named_workspace(name)))
                        .unwrap_or_else(|| {
                            mon.map(|mon| mon.active_workspace_ref())
                                .or_else(|| self.swayward.layout.active_workspace())
                        });

                    if let Some(ws) = ws {
                        // If the window is pending fullscreen, then this will do nothing since
                        // then the Maximized state is already unset. But that's expected: the
                        // window remains fullscreen, and we simply remember that it is no
                        // longer pending maximized.
                        *is_pending_maximized = false;
                        toplevel.with_pending_state(|state| {
                            state.states.unset(xdg_toplevel::State::Maximized);
                        });

                        let is_floating = rules.compute_open_floating(&toplevel);
                        let configure_width = if is_floating {
                            *floating_width
                        } else if *is_full_width {
                            Some(PresetSize::Proportion(1.))
                        } else {
                            *width
                        };
                        let configure_height = if is_floating {
                            *floating_height
                        } else {
                            *height
                        };
                        ws.configure_new_window(
                            &unmapped.window,
                            configure_width,
                            configure_height,
                            is_floating,
                            rules,
                        );
                    }

                    // We already sent the initial configure, so we need to reconfigure.
                    toplevel.send_configure();
                }
            }
        } else {
            error!("couldn't find the toplevel in unmaximize_request()");
            toplevel.send_configure();
        }
    }

    fn fullscreen_request(
        &mut self,
        toplevel: ToplevelSurface,
        wl_output: Option<wl_output::WlOutput>,
    ) {
        let requested_output = wl_output.and_then(|o| self.swayward.output_from_resource(&o));

        if let Some((mapped, current_output)) = self
            .swayward
            .layout
            .find_window_and_output_mut(toplevel.wl_surface())
        {
            // A configure is required in response to this event regardless if there are pending
            // changes.
            mapped.set_needs_configure();

            let window = mapped.window.clone();

            if let Some(requested_output) = requested_output {
                if Some(&requested_output) != current_output {
                    self.swayward.layout.move_to_output(
                        Some(&window),
                        &requested_output,
                        None,
                        ActivateWindow::Smart,
                    );
                }
            }

            self.swayward.layout.set_fullscreen(&window, true);
        } else if let Some(unmapped) = self
            .swayward
            .unmapped_windows
            .get_mut(toplevel.wl_surface())
        {
            match &mut unmapped.state {
                InitialConfigureState::NotConfigured {
                    wants_fullscreen, ..
                } => {
                    *wants_fullscreen = Some(requested_output);

                    // The required configure will be the initial configure.
                }
                InitialConfigureState::Configured { rules, output, .. } => {
                    // Figure out the monitor following a similar logic to initial configure.
                    // FIXME: deduplicate.
                    let mon = requested_output
                        .as_ref()
                        // If none requested, try currently configured output.
                        .or(output.as_ref())
                        .and_then(|o| self.swayward.layout.monitor_for_output(o))
                        .map(|mon| (mon, false))
                        // If not, check if we have a parent with a monitor.
                        .or_else(|| {
                            toplevel
                                .parent()
                                .and_then(|parent| {
                                    self.swayward.layout.find_window_and_output(&parent)
                                })
                                .and_then(|(_win, output)| output)
                                .and_then(|o| self.swayward.layout.monitor_for_output(o))
                                .map(|mon| (mon, true))
                        })
                        // If not, fall back to the active monitor.
                        .or_else(|| {
                            self.swayward
                                .layout
                                .active_monitor_ref()
                                .map(|mon| (mon, false))
                        });

                    *output = mon
                        .filter(|(_, parent)| !parent)
                        .map(|(mon, _)| mon.output().clone());
                    let mon = mon.map(|(mon, _)| mon);

                    let ws = mon
                        .map(|mon| mon.active_workspace_ref())
                        .or_else(|| self.swayward.layout.active_workspace());

                    if let Some(ws) = ws {
                        toplevel.with_pending_state(|state| {
                            state.states.set(xdg_toplevel::State::Fullscreen);
                            state.states.unset(xdg_toplevel::State::Maximized);
                        });
                        ws.configure_new_window(&unmapped.window, None, None, false, rules);
                    }

                    // We already sent the initial configure, so we need to reconfigure.
                    toplevel.send_configure();
                }
            }
        } else {
            error!("couldn't find the toplevel in fullscreen_request()");
            toplevel.send_configure();
        }
    }

    fn unfullscreen_request(&mut self, toplevel: ToplevelSurface) {
        if let Some((mapped, _)) = self
            .swayward
            .layout
            .find_window_and_output_mut(toplevel.wl_surface())
        {
            // A configure is required in response to this event regardless if there are pending
            // changes.
            mapped.set_needs_configure();

            let window = mapped.window.clone();
            self.swayward.layout.set_fullscreen(&window, false);
        } else if let Some(unmapped) = self
            .swayward
            .unmapped_windows
            .get_mut(toplevel.wl_surface())
        {
            match &mut unmapped.state {
                InitialConfigureState::NotConfigured {
                    wants_fullscreen, ..
                } => {
                    *wants_fullscreen = None;

                    // The required configure will be the initial configure.
                }
                InitialConfigureState::Configured {
                    rules,
                    width,
                    height,
                    floating_width,
                    floating_height,
                    is_full_width,
                    output,
                    workspace_name,
                    is_pending_maximized,
                } => {
                    // Figure out the monitor following a similar logic to initial configure.
                    // FIXME: deduplicate.
                    let mon = workspace_name
                        .as_deref()
                        .and_then(|name| self.swayward.layout.monitor_for_workspace(name))
                        .map(|mon| (mon, false));

                    let mon = mon.or_else(|| {
                        output
                            .as_ref()
                            .and_then(|o| self.swayward.layout.monitor_for_output(o))
                            .map(|mon| (mon, false))
                            // If not, check if we have a parent with a monitor.
                            .or_else(|| {
                                toplevel
                                    .parent()
                                    .and_then(|parent| {
                                        self.swayward.layout.find_window_and_output(&parent)
                                    })
                                    .and_then(|(_win, output)| output)
                                    .and_then(|o| self.swayward.layout.monitor_for_output(o))
                                    .map(|mon| (mon, true))
                            })
                            // If not, fall back to the active monitor.
                            .or_else(|| {
                                self.swayward
                                    .layout
                                    .active_monitor_ref()
                                    .map(|mon| (mon, false))
                            })
                    });

                    *output = mon
                        .filter(|(_, parent)| !parent)
                        .map(|(mon, _)| mon.output().clone());
                    let mon = mon.map(|(mon, _)| mon);

                    let ws = workspace_name
                        .as_deref()
                        .and_then(|name| mon.map(|mon| mon.find_named_workspace(name)))
                        .unwrap_or_else(|| {
                            mon.map(|mon| mon.active_workspace_ref())
                                .or_else(|| self.swayward.layout.active_workspace())
                        });

                    if let Some(ws) = ws {
                        toplevel.with_pending_state(|state| {
                            state.states.unset(xdg_toplevel::State::Fullscreen);

                            if *is_pending_maximized {
                                state.states.set(xdg_toplevel::State::Maximized);
                            }
                        });

                        let is_floating = rules.compute_open_floating(&toplevel);
                        let configure_width = if is_floating {
                            *floating_width
                        } else if *is_full_width {
                            Some(PresetSize::Proportion(1.))
                        } else {
                            *width
                        };
                        let configure_height = if is_floating {
                            *floating_height
                        } else {
                            *height
                        };
                        ws.configure_new_window(
                            &unmapped.window,
                            configure_width,
                            configure_height,
                            is_floating,
                            rules,
                        );
                    }

                    // We already sent the initial configure, so we need to reconfigure.
                    toplevel.send_configure();
                }
            }
        } else {
            error!("couldn't find the toplevel in unfullscreen_request()");
            toplevel.send_configure();
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        if self
            .swayward
            .unmapped_windows
            .remove(surface.wl_surface())
            .is_some()
        {
            // An unmapped toplevel got destroyed.
            return;
        }

        let win_out = self
            .swayward
            .layout
            .find_window_and_output(surface.wl_surface());

        let Some((mapped, output)) = win_out else {
            // I have no idea how this can happen, but I saw it happen once, in a weird interaction
            // involving laptop going to sleep and resuming.
            error!("toplevel missing from both unmapped_windows and layout");
            return;
        };
        let window = mapped.window.clone();
        let output = output.cloned();

        let id = mapped.id();
        self.swayward
            .stop_casts_for_target(CastTarget::Window { id: id.get() });

        self.store_unmap_snapshot(&window, output.as_ref());

        let transaction = Transaction::new();
        let blocker = transaction.blocker();
        self.backend.with_primary_renderer(|renderer| {
            self.swayward
                .layout
                .start_close_animation_for_window(renderer, &window, blocker);
        });

        let active_window = self.swayward.layout.focus().map(|m| &m.window);
        let was_active = active_window == Some(&window);

        self.swayward.window_mru_ui.remove_window(id);
        self.swayward.cancel_urgency_timer(id);
        self.swayward
            .layout
            .remove_window(&window, transaction.clone());

        let surface = surface.wl_surface();
        // This check is necessary because implicit resource destruction is done with
        // undefined order, so the surface might get destroyed before toplevel_destroyed() is
        // called. In this case, adding the default pre-commit hook here would leak it, since the
        // place that removes it is WlSurface::destroyed(), which had already been called by now.
        if surface.is_alive() {
            self.add_default_dmabuf_pre_commit_hook(surface);
        }

        // If this is the only instance, then this transaction will complete immediately, so no
        // need to set the timer.
        if !transaction.is_last() {
            transaction.register_deadline_timer(&self.swayward.event_loop);
        }

        if was_active {
            self.maybe_warp_cursor_to_focus();
        }

        if let Some(output) = output {
            self.swayward.queue_redraw(&output);
            self.swayward.queue_redraw_mru_output();
        }
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        if let Some(output) = self.output_for_popup(&PopupKind::Xdg(surface)) {
            self.swayward.queue_redraw(&output.clone());
        }
    }

    fn app_id_changed(&mut self, toplevel: ToplevelSurface) {
        self.update_window_rules(&toplevel);
        self.refresh_formatted_title(&toplevel);
    }

    fn title_changed(&mut self, toplevel: ToplevelSurface) {
        self.update_window_rules(&toplevel);
        self.refresh_formatted_title(&toplevel);
    }

    fn parent_changed(&mut self, toplevel: ToplevelSurface) {
        let Some(parent) = toplevel.parent() else {
            return;
        };

        if let Some((mapped, output)) = self.swayward.layout.find_window_and_output_mut(&parent) {
            let output = output.cloned();
            let window = mapped.window.clone();
            if self.swayward.layout.descendants_added(&window) {
                if let Some(output) = output {
                    self.swayward.queue_redraw(&output);
                }
            }
        }
    }
}

use super::*;

impl State {
    pub(super) fn on_tablet_tool_axis<I: InputBackend>(&mut self, event: I::TabletToolAxisEvent)
    where
        I::Device: 'static, // Needed for downcasting.
    {
        self.update_tablet_tool::<I>(&event, true);
    }

    pub(super) fn update_tablet_tool<I: InputBackend>(
        &mut self,
        event: &(impl Event<I> + TabletToolEvent<I>),
        send_frame: bool,
    ) where
        I::Device: 'static,
    {
        let Some(pos) = self.compute_tablet_position(event) else {
            return;
        };

        if let Some(output) = self.swayward.screenshot_ui.selection_output() {
            let geom = self.swayward.global_space.output_geometry(output).unwrap();
            let point = (pos - geom.loc.to_f64())
                .to_physical(output.current_scale().fractional_scale())
                .to_i32_round::<i32>();

            self.swayward.screenshot_ui.pointer_motion(point, None);
        }

        if let Some(mru_output) = self.swayward.window_mru_ui.output() {
            if let Some((output, pos_within_output)) = self.swayward.output_under(pos) {
                if mru_output == output {
                    self.swayward
                        .window_mru_ui
                        .pointer_motion(pos_within_output);
                }
            }
        }

        let under = self.swayward.contents_under(pos);

        let tablet_seat = self.swayward.seat.tablet_seat();
        let tool = tablet_seat.get_tool(&event.tool());
        if let Some(tool) = tool {
            let time = event.time();

            let frame = smithay_tablet::tool::AxisFrame {
                pressure: event.pressure_has_changed().then(|| event.pressure()),
                distance: event.distance_has_changed().then(|| event.distance()),
                tilt: event.tilt_has_changed().then(|| event.tilt()),
                rotation: event.rotation_has_changed().then(|| event.rotation()),
                slider: event.slider_has_changed().then(|| event.slider_position()),
                wheel: event
                    .wheel_has_changed()
                    .then(|| (event.wheel_delta(), event.wheel_delta_discrete())),
            };

            tool.motion(
                self,
                under.surface,
                &smithay_tablet::tool::MotionEvent {
                    location: pos,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );

            // Set axis after motion to ensure it reaches the new focus surface.
            tool.axis(self, frame);

            if send_frame {
                tool.frame(self, time);
            }

            self.swayward.pointer_visibility = PointerVisibility::Visible;
            self.swayward.tablet_cursor_location = Some(pos);
        }

        // Redraw to update the cursor position.
        // FIXME: redraw only outputs overlapping the cursor.
        self.swayward.queue_redraw_all();
    }

    pub(super) fn on_tablet_tool_tip<I: InputBackend>(&mut self, event: I::TabletToolTipEvent)
    where
        I::Device: 'static,
    {
        let tool = self.swayward.seat.tablet_seat().get_tool(&event.tool());

        let Some(tool) = tool else {
            return;
        };

        let tip_state = event.tip_state();
        if tip_state == TabletToolTipState::Down {
            // Tip events can come together with axis event data with no separate axis event.
            self.update_tablet_tool::<I>(&event, false);
        }

        let serial = SERIAL_COUNTER.next_serial();
        let time = event.time();

        match tip_state {
            TabletToolTipState::Down => {
                if let Some(pos) = self.swayward.tablet_cursor_location {
                    let under = self.swayward.contents_under(pos);

                    let mod_key = self.backend.mod_key(&self.swayward.config.borrow());
                    let mods = self.swayward.seat.get_keyboard().unwrap().modifier_state();
                    let modifiers = modifiers_from_state(mods);
                    let mod_down = mod_key.is_pressed(modifiers);

                    if self.swayward.screenshot_ui.is_open() {
                        // If we'll be moving the existing selection, use the selection output.
                        let output = if mod_down {
                            self.swayward.screenshot_ui.selection_output()
                        } else {
                            under.output.as_ref()
                        };

                        if let Some(output) = output.cloned() {
                            let geom = self.swayward.global_space.output_geometry(&output).unwrap();
                            let point = (pos - geom.loc.to_f64())
                                .to_physical(output.current_scale().fractional_scale())
                                .to_i32_round();

                            if self
                                .swayward
                                .screenshot_ui
                                .pointer_down(output, point, None, mod_down)
                            {
                                self.swayward.queue_redraw_all();
                            }
                        }
                    } else if let Some(mru_output) = self.swayward.window_mru_ui.output() {
                        if let Some((output, pos_within_output)) = self.swayward.output_under(pos) {
                            if mru_output == output {
                                let id = self
                                    .swayward
                                    .window_mru_ui
                                    .pointer_motion(pos_within_output);
                                if id.is_some() {
                                    self.confirm_mru();
                                } else {
                                    self.swayward.cancel_mru();
                                }
                            } else {
                                self.swayward.cancel_mru();
                            }
                        }
                    } else if !tool.is_grabbed() {
                        if self.swayward.layout.is_overview_open()
                            && !mod_down
                            && under.layer.is_none()
                            && under.output.is_some()
                        {
                            let (output, pos_within_output) =
                                self.swayward.output_under(pos).unwrap();
                            let output = output.clone();

                            let mut matched_narrow = true;
                            let mut ws = self.swayward.workspace_under(false, pos);
                            if ws.is_none() {
                                matched_narrow = false;
                                ws = self.swayward.workspace_under(true, pos);
                            }
                            let ws_id = ws.map(|(_, ws)| ws.id());

                            let mapped = self.swayward.window_under(pos);
                            let window = mapped.map(|mapped| mapped.window.clone());

                            let start_data = TabletToolGrabStartData {
                                focus: None,
                                trigger: smithay_tablet::tool::GrabTrigger::Tip,
                                location: pos,
                            };
                            let start_data = AnyStartData::TabletTool(start_data);
                            let start_timestamp = Duration::from_micros(event.time().micros());
                            let grab = TouchOverviewGrab::new(
                                start_data,
                                start_timestamp,
                                output,
                                pos_within_output,
                                ws_id,
                                matched_narrow,
                                window,
                            );
                            tool.set_grab(self, grab, time, serial, Focus::Clear);
                        } else if let Some((window, _)) = under.window {
                            self.swayward.layout.activate_window(&window);

                            // Check if we need to start a tablet tool move grab.
                            if mod_down {
                                let start_data = TabletToolGrabStartData {
                                    focus: None,
                                    trigger: smithay_tablet::tool::GrabTrigger::Tip,
                                    location: pos,
                                };
                                let start_data = AnyStartData::TabletTool(start_data);
                                let icon = CursorIcon::Grabbing;
                                if let Some(grab) = MoveGrab::new(
                                    self,
                                    start_data,
                                    window.clone(),
                                    true,
                                    Some(icon),
                                ) {
                                    tool.set_grab(self, grab, time, serial, Focus::Clear);
                                }
                            }

                            // FIXME: granular.
                            self.swayward.queue_redraw_all();
                        } else if let Some(output) = under.output {
                            self.swayward.layout.focus_output(&output);

                            // FIXME: granular.
                            self.swayward.queue_redraw_all();
                        }
                        self.swayward.focus_layer_surface_if_on_demand(under.layer);
                    }
                }

                tool.down(self, &smithay_tablet::tool::DownEvent { serial, time });
            }
            TabletToolTipState::Up => {
                if let Some(capture) = self.swayward.screenshot_ui.pointer_up(None) {
                    if capture {
                        self.confirm_screenshot(true);
                    } else {
                        self.swayward.queue_redraw_all();
                    }
                }

                tool.up(self, &smithay_tablet::tool::UpEvent { serial, time });

                self.update_tablet_tool::<I>(&event, false);
            }
        }

        tool.frame(self, time);
    }

    pub(super) fn on_tablet_tool_proximity<I: InputBackend>(
        &mut self,
        event: I::TabletToolProximityEvent,
    ) where
        I::Device: 'static, // Needed for downcasting.
    {
        let Some(pos) = self.compute_tablet_position(&event) else {
            return;
        };

        let under = self.swayward.contents_under(pos);

        let tablet_seat = self.swayward.seat.tablet_seat();
        let display_handle = self.swayward.display_handle.clone();
        let tool = tablet_seat
            .get_tool(&event.tool())
            .unwrap_or_else(|| tablet_seat.add_wp_tool(self, &display_handle, &event.tool()));
        let tablet = tablet_seat.get_tablet(&TabletDescriptor::from(&event.device()));
        if let Some(tablet) = tablet {
            let serial = SERIAL_COUNTER.next_serial();
            let time = event.time();

            match event.state() {
                ProximityState::In => {
                    let frame = smithay_tablet::tool::AxisFrame {
                        pressure: event.pressure_has_changed().then(|| event.pressure()),
                        distance: event.distance_has_changed().then(|| event.distance()),
                        tilt: event.tilt_has_changed().then(|| event.tilt()),
                        rotation: event.rotation_has_changed().then(|| event.rotation()),
                        slider: event.slider_has_changed().then(|| event.slider_position()),
                        wheel: event
                            .wheel_has_changed()
                            .then(|| (event.wheel_delta(), event.wheel_delta_discrete())),
                    };

                    tool.proximity_in(
                        self,
                        under.surface,
                        tablet,
                        &smithay_tablet::tool::ProximityInEvent {
                            location: pos,
                            axis: Some(frame),
                            serial,
                            time,
                        },
                    );

                    // Is proximity in usually immediatelly followed by other events like button? If
                    // so, then it might be worth delaying this frame() until the loop callback to
                    // batch all of them in.
                    tool.frame(self, time);

                    self.swayward.pointer_visibility = PointerVisibility::Visible;
                    self.swayward.tablet_cursor_location = Some(pos);
                }
                ProximityState::Out => {
                    tool.proximity_out(
                        self,
                        &smithay_tablet::tool::ProximityOutEvent { serial, time },
                    );
                    tool.frame(self, time);

                    // Move the mouse pointer here to avoid discontinuity.
                    //
                    // Plus, Wayland SDL2 currently warps the pointer into some weird
                    // location on proximity out, so this should help it a little.
                    if let Some(pos) = self.swayward.tablet_cursor_location {
                        self.move_cursor(pos);
                    }

                    self.swayward.pointer_visibility = PointerVisibility::Visible;
                    self.swayward.tablet_cursor_location = None;
                }
            }

            // FIXME: granular.
            self.swayward.queue_redraw_all();
        }
    }

    pub(super) fn on_tablet_tool_button<I: InputBackend>(
        &mut self,
        event: I::TabletToolButtonEvent,
    ) {
        const BTN_STYLUS3: u32 = 0x149;
        const BTN_STYLUS: u32 = 0x14b;
        const BTN_STYLUS2: u32 = 0x14c;

        let tool = self.swayward.seat.tablet_seat().get_tool(&event.tool());

        if let Some(tool) = tool {
            let button = event.button();

            if self.swayward.suppressed_buttons.remove(&button) {
                return;
            }

            let trigger = match button {
                BTN_STYLUS => Some(Trigger::TabletStylusButton1),
                BTN_STYLUS2 => Some(Trigger::TabletStylusButton2),
                BTN_STYLUS3 => Some(Trigger::TabletStylusButton3),
                _ => None,
            };

            if let Some(trigger) = trigger {
                if event.button_state() == ButtonState::Pressed {
                    let mod_key = self.backend.mod_key(&self.swayward.config.borrow());
                    let mods = self.swayward.seat.get_keyboard().unwrap().modifier_state();
                    let modifiers = modifiers_from_state(mods);
                    let input_device = event.device().sway_identifier();

                    if self
                        .swayward
                        .mods_with_tablet_stylus_binds
                        .contains(&modifiers)
                    {
                        let bind = {
                            let config = self.swayward.config.borrow();
                            let bindings = config.binds.0.iter();
                            find_configured_bind_for_device(
                                bindings,
                                mod_key,
                                trigger,
                                mods,
                                &input_device,
                            )
                        }
                        .filter(|bind| {
                            !self.swayward.screenshot_ui.is_open()
                                || allowed_during_screenshot(&bind.action)
                        });
                        if let Some(bind) = bind {
                            self.swayward.suppressed_buttons.insert(button);
                            self.handle_bind(bind.clone());
                            return;
                        }
                    }
                }
            }

            let time = event.time();

            tool.button(
                self,
                &smithay_tablet::tool::ButtonEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    button,
                    state: event.button_state(),
                    time,
                },
            );

            tool.frame(self, time);
        }
    }
}

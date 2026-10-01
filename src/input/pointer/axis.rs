use super::*;

impl State {
    pub(in crate::input) fn on_pointer_axis<I: InputBackend>(
        &mut self,
        event: I::PointerAxisEvent,
    ) {
        let pointer = &self.swayward.seat.get_pointer().unwrap();

        let source = event.source();
        let input_device = event.device().sway_identifier();

        let mod_key = self.backend.mod_key(&self.swayward.config.borrow());

        // We received an event for the regular pointer, so show it now. This is also needed for
        // update_pointer_contents() below to return the real contents, necessary for the pointer
        // axis event to reach the window.
        self.swayward.pointer_visibility = PointerVisibility::Visible;
        self.swayward.tablet_cursor_location = None;

        let timestamp = Duration::from_micros(event.time().micros());

        let horizontal_amount_v120 = event.amount_v120(Axis::Horizontal);
        let vertical_amount_v120 = event.amount_v120(Axis::Vertical);

        let is_overview_open = self.swayward.layout.is_overview_open();

        // We should only handle scrolling in the overview if the pointer is not over a (top or
        // overlay) layer surface.
        let should_handle_in_overview = if is_overview_open {
            // FIXME: ideally this should happen after updating the pointer contents, which happens
            // below. However, our pointer actions are supposed to act on the old surface, before
            // updating the pointer contents.
            pointer
                .current_focus()
                .map(|surface| self.swayward.find_root_shell_surface(&surface))
                .is_none_or(|root| {
                    !self
                        .swayward
                        .mapped_layer_surfaces
                        .keys()
                        .any(|layer| *layer.wl_surface() == root)
                })
        } else {
            false
        };

        let is_mru_open = self.swayward.window_mru_ui.is_open();

        // Handle wheel scroll bindings.
        if source == AxisSource::Wheel {
            // If we have a scroll bind with current modifiers, then accumulate and don't pass to
            // Wayland. If there's no bind, reset the accumulator.
            let mods = self.modifier_state();
            let modifiers = modifiers_from_state(mods);
            let should_handle = should_handle_in_overview
                || is_mru_open
                || self.swayward.mods_with_wheel_binds.contains(&modifiers);
            if should_handle {
                let mut handled = false;
                let horizontal = horizontal_amount_v120.unwrap_or(0.);
                let ticks = self
                    .swayward
                    .horizontal_wheel_tracker
                    .accumulate(horizontal);
                if ticks != 0 {
                    let (bind_left, bind_right) =
                        if should_handle_in_overview && modifiers.is_empty() {
                            let bind_left = Some(synthetic_bind(
                                Trigger::WheelScrollLeft,
                                Action::FocusColumnLeftUnderMouse,
                                None,
                            ));
                            let bind_right = Some(synthetic_bind(
                                Trigger::WheelScrollRight,
                                Action::FocusColumnRightUnderMouse,
                                None,
                            ));
                            (bind_left, bind_right)
                        } else {
                            self.resolve_axis_binds(
                                (Trigger::WheelScrollLeft, Trigger::WheelScrollRight),
                                mods,
                                modifiers,
                                mod_key,
                                &input_device,
                                true,
                            )
                        };

                    handled |= self.fire_axis_ticks(ticks, bind_left, bind_right);
                }

                let vertical = vertical_amount_v120.unwrap_or(0.);
                let ticks = self.swayward.vertical_wheel_tracker.accumulate(vertical);
                if ticks != 0 {
                    let (bind_up, bind_down) = if should_handle_in_overview && modifiers.is_empty()
                    {
                        let bind_up = Some(synthetic_bind(
                            Trigger::WheelScrollUp,
                            Action::FocusWorkspaceUpUnderMouse,
                            Some(Duration::from_millis(50)),
                        ));
                        let bind_down = Some(synthetic_bind(
                            Trigger::WheelScrollDown,
                            Action::FocusWorkspaceDownUnderMouse,
                            Some(Duration::from_millis(50)),
                        ));
                        (bind_up, bind_down)
                    } else if should_handle_in_overview && modifiers == Modifiers::SHIFT {
                        let bind_up = Some(synthetic_bind(
                            Trigger::WheelScrollUp,
                            Action::FocusColumnLeftUnderMouse,
                            Some(Duration::from_millis(50)),
                        ));
                        let bind_down = Some(synthetic_bind(
                            Trigger::WheelScrollDown,
                            Action::FocusColumnRightUnderMouse,
                            Some(Duration::from_millis(50)),
                        ));
                        (bind_up, bind_down)
                    } else {
                        self.resolve_axis_binds(
                            (Trigger::WheelScrollUp, Trigger::WheelScrollDown),
                            mods,
                            modifiers,
                            mod_key,
                            &input_device,
                            true,
                        )
                    };

                    handled |= self.fire_axis_ticks(ticks, bind_up, bind_down);
                }

                if handled {
                    return;
                }
            } else {
                self.swayward.horizontal_wheel_tracker.reset();
                self.swayward.vertical_wheel_tracker.reset();
            }

            let contents = self.swayward.contents_under(pointer.current_location());
            if let Some((
                window,
                HitType::Activate {
                    is_tab_indicator: true,
                },
            )) = contents.window
            {
                let factor = self
                    .swayward
                    .config
                    .borrow()
                    .input
                    .mouse
                    .scroll_factor
                    .map(|factor| factor.h_v_factors().1)
                    .unwrap_or(1.);
                let steps = (factor * vertical_amount_v120.unwrap_or(0.) / 120.).round() as i32;
                if steps != 0 {
                    if let Some(window) = self.swayward.layout.scroll_tab_indicator(&window, steps)
                    {
                        self.swayward.layout.activate_window(&window);
                        self.swayward.queue_redraw_all();
                    }
                }
                return;
            }
        }

        let horizontal_amount = event.amount(Axis::Horizontal);
        let vertical_amount = event.amount(Axis::Vertical);

        // Handle touchpad and continuous scroll bindings.
        if source == AxisSource::Finger || source == AxisSource::Continuous {
            let mods = self.modifier_state();
            let modifiers = modifiers_from_state(mods);

            let horizontal = horizontal_amount.unwrap_or(0.);
            let vertical = vertical_amount.unwrap_or(0.);

            if should_handle_in_overview && modifiers.is_empty() {
                let mut redraw = false;

                let action = self
                    .swayward
                    .overview_scroll_swipe_gesture
                    .update(horizontal, vertical);
                let is_vertical = self.swayward.overview_scroll_swipe_gesture.is_vertical();

                if action.end() {
                    if !is_vertical {
                        redraw |= self
                            .swayward
                            .layout
                            .view_offset_gesture_end(Some(true))
                            .is_some();
                    }
                } else {
                    // Maybe begin, then update.
                    if !is_vertical {
                        if action.begin() {
                            if let Some((output, ws)) = self.swayward.workspace_under_cursor(true) {
                                let ws_id = ws.id();
                                let ws_idx =
                                    self.swayward.layout.find_workspace_by_id(ws_id).unwrap().0;

                                self.swayward.layout.view_offset_gesture_begin(
                                    &output,
                                    Some(ws_idx),
                                    true,
                                );
                                redraw = true;
                            }
                        }

                        let res = self
                            .swayward
                            .layout
                            .view_offset_gesture_update(horizontal, timestamp, true);
                        if let Some(Some(_)) = res {
                            redraw = true;
                        }
                    }
                }

                if redraw {
                    self.swayward.queue_redraw_all();
                }

                return;
            } else {
                let mut redraw = false;
                if self.swayward.overview_scroll_swipe_gesture.reset()
                    && !self.swayward.overview_scroll_swipe_gesture.is_vertical()
                {
                    redraw |= self
                        .swayward
                        .layout
                        .view_offset_gesture_end(Some(true))
                        .is_some();
                }
                if redraw {
                    self.swayward.queue_redraw_all();
                }
            }

            if is_mru_open
                || self
                    .swayward
                    .mods_with_finger_scroll_binds
                    .contains(&modifiers)
            {
                let ticks = self
                    .swayward
                    .horizontal_finger_scroll_tracker
                    .accumulate(horizontal);
                if ticks != 0 {
                    let (bind_left, bind_right) = self.resolve_axis_binds(
                        (Trigger::TouchpadScrollLeft, Trigger::TouchpadScrollRight),
                        mods,
                        modifiers,
                        mod_key,
                        &input_device,
                        false,
                    );

                    self.fire_axis_ticks(ticks, bind_left, bind_right);
                }

                let ticks = self
                    .swayward
                    .vertical_finger_scroll_tracker
                    .accumulate(vertical);
                if ticks != 0 {
                    let (bind_up, bind_down) = self.resolve_axis_binds(
                        (Trigger::TouchpadScrollUp, Trigger::TouchpadScrollDown),
                        mods,
                        modifiers,
                        mod_key,
                        &input_device,
                        false,
                    );

                    self.fire_axis_ticks(ticks, bind_up, bind_down);
                }

                return;
            } else {
                self.swayward.horizontal_finger_scroll_tracker.reset();
                self.swayward.vertical_finger_scroll_tracker.reset();
            }
        }

        self.update_pointer_contents();

        let device_scroll_factor = {
            let config = self.swayward.config.borrow();
            match source {
                AxisSource::Wheel => config.input.mouse.scroll_factor,
                AxisSource::Finger => config.input.touchpad.scroll_factor,
                _ => None,
            }
        };

        // Get window-specific scroll factor
        let window_scroll_factor = pointer
            .current_focus()
            .map(|focused| self.swayward.find_root_shell_surface(&focused))
            .and_then(|root| self.swayward.layout.find_window_and_output(&root).unzip().0)
            .and_then(|window| window.rules().scroll_factor)
            .unwrap_or(1.);

        // Determine final scroll factors based on configuration
        let (horizontal_factor, vertical_factor) = device_scroll_factor
            .map(|x| x.h_v_factors())
            .unwrap_or((1.0, 1.0));
        let (horizontal_factor, vertical_factor) = (
            horizontal_factor * window_scroll_factor,
            vertical_factor * window_scroll_factor,
        );

        let horizontal_amount = horizontal_amount.unwrap_or_else(|| {
            // Winit backend, discrete scrolling.
            horizontal_amount_v120.unwrap_or(0.0) / 120. * 15.
        }) * horizontal_factor;

        let vertical_amount = vertical_amount.unwrap_or_else(|| {
            // Winit backend, discrete scrolling.
            vertical_amount_v120.unwrap_or(0.0) / 120. * 15.
        }) * vertical_factor;

        let horizontal_amount_v120 = horizontal_amount_v120.map(|x| x * horizontal_factor);
        let vertical_amount_v120 = vertical_amount_v120.map(|x| x * vertical_factor);

        let mut frame = AxisFrame::new(event.time()).source(source);
        if horizontal_amount != 0.0 {
            frame = frame
                .relative_direction(Axis::Horizontal, event.relative_direction(Axis::Horizontal));
            frame = frame.value(Axis::Horizontal, horizontal_amount);
            if let Some(v120) = horizontal_amount_v120 {
                frame = frame.v120(Axis::Horizontal, v120 as i32);
            }
        }
        if vertical_amount != 0.0 {
            frame =
                frame.relative_direction(Axis::Vertical, event.relative_direction(Axis::Vertical));
            frame = frame.value(Axis::Vertical, vertical_amount);
            if let Some(v120) = vertical_amount_v120 {
                frame = frame.v120(Axis::Vertical, v120 as i32);
            }
        }

        if source == AxisSource::Finger {
            if event.amount(Axis::Horizontal) == Some(0.0) {
                frame = frame.stop(Axis::Horizontal);
            }
            if event.amount(Axis::Vertical) == Some(0.0) {
                frame = frame.stop(Axis::Vertical);
            }
        }

        pointer.axis(self, frame);
        pointer.frame(self);
    }
}

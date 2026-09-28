use super::*;

impl State {
    pub(super) fn compute_absolute_location<I: InputBackend>(
        &self,
        evt: &impl AbsolutePositionEvent<I>,
        fallback_output: Option<&Output>,
    ) -> Option<Point<f64, Logical>> {
        let output = evt.device().output(self);
        let output = output.filter(|output| self.swayward.output_exists(output));
        let output = output.as_ref().or(fallback_output)?;
        let output_geo = self.swayward.global_space.output_geometry(output).unwrap();
        let transform = output.current_transform();
        let size = transform.invert().transform_size(output_geo.size);
        Some(
            transform.transform_point_in(evt.position_transformed(size), &size.to_f64())
                + output_geo.loc.to_f64(),
        )
    }

    /// Computes the cursor position for the touch event.
    ///
    /// This function handles the touch output mapping, as well as coordinate transform
    pub(super) fn compute_touch_location<I: InputBackend>(
        &self,
        evt: &impl AbsolutePositionEvent<I>,
    ) -> Option<Point<f64, Logical>> {
        self.compute_absolute_location(evt, self.swayward.output_for_touch())
    }

    pub(super) fn on_touch_down<I: InputBackend>(&mut self, evt: I::TouchDownEvent) {
        let Some(handle) = self.swayward.seat.get_touch() else {
            return;
        };
        let Some(pos) = self.compute_touch_location(&evt) else {
            return;
        };
        let slot = evt.slot();

        let serial = SERIAL_COUNTER.next_serial();

        let under = self.swayward.contents_under(pos);

        let mod_key = self.backend.mod_key(&self.swayward.config.borrow());
        let mods = self.swayward.seat.get_keyboard().unwrap().modifier_state();
        let mods = modifiers_from_state(mods);
        let mod_down = mod_key.is_pressed(mods);

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
                    .pointer_down(output, point, Some(slot), mod_down)
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
        } else if !handle.is_grabbed() {
            if self.swayward.layout.is_overview_open()
                && !mod_down
                && under.layer.is_none()
                && under.output.is_some()
            {
                let (output, pos_within_output) = self.swayward.output_under(pos).unwrap();
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

                let start_data = TouchGrabStartData {
                    focus: None,
                    slot,
                    location: pos,
                };
                let start_data = AnyStartData::Touch(start_data);
                let start_timestamp = Duration::from_micros(evt.time().micros());
                let grab = TouchOverviewGrab::new(
                    start_data,
                    start_timestamp,
                    output,
                    pos_within_output,
                    ws_id,
                    matched_narrow,
                    window,
                );
                handle.set_grab(self, grab, serial);
            } else if let Some((window, _)) = under.window {
                self.swayward.layout.activate_window(&window);

                // Check if we need to start a touch move grab.
                if mod_down {
                    let start_data = TouchGrabStartData {
                        focus: None,
                        slot,
                        location: pos,
                    };
                    let start_data = AnyStartData::Touch(start_data);
                    if let Some(grab) = MoveGrab::new(self, start_data, window.clone(), true, None)
                    {
                        handle.set_grab(self, grab, serial);
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
        };

        handle.down(
            self,
            under.surface,
            &DownEvent {
                slot,
                location: pos,
                serial,
                time: evt.time(),
            },
        );

        // We're using touch, hide the pointer.
        self.swayward.pointer_visibility = PointerVisibility::Disabled;
    }
    pub(super) fn on_touch_up<I: InputBackend>(&mut self, evt: I::TouchUpEvent) {
        let Some(handle) = self.swayward.seat.get_touch() else {
            return;
        };
        let slot = evt.slot();

        if let Some(capture) = self.swayward.screenshot_ui.pointer_up(Some(slot)) {
            if capture {
                self.confirm_screenshot(true);
            } else {
                self.swayward.queue_redraw_all();
            }
        }

        let serial = SERIAL_COUNTER.next_serial();
        handle.up(
            self,
            &UpEvent {
                slot,
                serial,
                time: evt.time(),
            },
        )
    }
    pub(super) fn on_touch_motion<I: InputBackend>(&mut self, evt: I::TouchMotionEvent) {
        let Some(handle) = self.swayward.seat.get_touch() else {
            return;
        };
        let Some(pos) = self.compute_touch_location(&evt) else {
            return;
        };
        let slot = evt.slot();

        if let Some(output) = self.swayward.screenshot_ui.selection_output().cloned() {
            let geom = self.swayward.global_space.output_geometry(&output).unwrap();
            let point = (pos - geom.loc.to_f64())
                .to_physical(output.current_scale().fractional_scale())
                .to_i32_round::<i32>();

            self.swayward
                .screenshot_ui
                .pointer_motion(point, Some(slot));
            self.swayward.queue_redraw(&output);
        }

        let under = self.swayward.contents_under(pos);
        handle.motion(
            self,
            under.surface,
            &TouchMotionEvent {
                slot,
                location: pos,
                time: evt.time(),
            },
        );

        // Inform the layout of an ongoing DnD operation.
        let is_dnd_grab = handle
            .with_grab(|_, grab| Self::is_dnd_grab(grab.as_any()))
            .unwrap_or(false);
        if is_dnd_grab {
            if let Some((output, pos_within_output)) = self.swayward.output_under(pos) {
                let output = output.clone();
                self.swayward.layout.dnd_update(output, pos_within_output);
            }
        }
    }
    pub(super) fn on_touch_frame<I: InputBackend>(&mut self, _evt: I::TouchFrameEvent) {
        let Some(handle) = self.swayward.seat.get_touch() else {
            return;
        };
        handle.frame(self);
    }
    pub(super) fn on_touch_cancel<I: InputBackend>(&mut self, _evt: I::TouchCancelEvent) {
        let Some(handle) = self.swayward.seat.get_touch() else {
            return;
        };
        handle.cancel(self);
    }
}

use super::*;

impl State {
    pub(in crate::input) fn on_gesture_swipe_begin<I: InputBackend>(
        &mut self,
        event: I::GestureSwipeBeginEvent,
    ) {
        if self.swayward.window_mru_ui.is_open() {
            // Don't start swipe gestures while in the MRU.
            return;
        }

        if event.fingers() == 3 {
            self.swayward.gesture_swipe_3f_cumulative = Some((0., 0.));

            // We handled this event.
            return;
        } else if event.fingers() == 4 {
            self.swayward.layout.overview_gesture_begin();
            self.swayward.queue_redraw_all();

            // We handled this event.
            return;
        }

        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_swipe_begin(
            self,
            &GestureSwipeBeginEvent {
                serial,
                time: event.time(),
                fingers: event.fingers(),
            },
        );
    }

    pub(in crate::input) fn on_gesture_swipe_update<I: InputBackend + 'static>(
        &mut self,
        event: I::GestureSwipeUpdateEvent,
    ) where
        I::Device: 'static,
    {
        let mut delta_x = event.delta_x();
        let mut delta_y = event.delta_y();

        if let Some(libinput_event) =
            (&event as &dyn Any).downcast_ref::<input::event::gesture::GestureSwipeUpdateEvent>()
        {
            delta_x = libinput_event.dx_unaccelerated();
            delta_y = libinput_event.dy_unaccelerated();
        }

        let uninverted_delta_y = delta_y;

        let device = event.device();
        if let Some(device) = (&device as &dyn Any).downcast_ref::<input::Device>() {
            if device.config_scroll_natural_scroll_enabled() {
                delta_x = -delta_x;
                delta_y = -delta_y;
            }
        }

        let is_overview_open = self.swayward.layout.is_overview_open();

        if let Some((cx, cy)) = &mut self.swayward.gesture_swipe_3f_cumulative {
            *cx += delta_x;
            *cy += delta_y;

            // Check if the gesture moved far enough to decide. Threshold copied from GNOME Shell.
            let (cx, cy) = (*cx, *cy);
            if cx * cx + cy * cy >= 16. * 16. {
                self.swayward.gesture_swipe_3f_cumulative = None;

                if cx.abs() > cy.abs() {
                    let output_ws = if is_overview_open {
                        self.swayward.workspace_under_cursor(true)
                    } else {
                        // We don't want to accidentally "catch" the wrong workspace during
                        // animations.
                        self.swayward.output_under_cursor().and_then(|output| {
                            let mon = self.swayward.layout.monitor_for_output(&output)?;
                            Some((output, mon.active_workspace_ref()))
                        })
                    };

                    if let Some((output, ws)) = output_ws {
                        let ws_idx = self
                            .swayward
                            .layout
                            .find_workspace_by_id(ws.id())
                            .unwrap()
                            .0;
                        self.swayward
                            .layout
                            .view_offset_gesture_begin(&output, Some(ws_idx), true);
                    }
                }
            }
        }

        let timestamp = Duration::from_micros(event.time().micros());

        let mut handled = false;
        let res = self
            .swayward
            .layout
            .view_offset_gesture_update(delta_x, timestamp, true);
        if let Some(output) = res {
            if let Some(output) = output {
                self.swayward.queue_redraw(&output);
            }
            handled = true;
        }

        let res = self
            .swayward
            .layout
            .overview_gesture_update(-uninverted_delta_y, timestamp);
        if let Some(redraw) = res {
            if redraw {
                self.swayward.queue_redraw_all();
            }
            handled = true;
        }

        if handled {
            // We handled this event.
            return;
        }

        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_swipe_update(
            self,
            &GestureSwipeUpdateEvent {
                time: event.time(),
                delta: event.delta(),
            },
        );
    }

    pub(in crate::input) fn on_gesture_swipe_end<I: InputBackend>(
        &mut self,
        event: I::GestureSwipeEndEvent,
    ) {
        self.swayward.gesture_swipe_3f_cumulative = None;

        let mut handled = false;
        let res = self.swayward.layout.view_offset_gesture_end(Some(true));
        if let Some(output) = res {
            self.swayward.queue_redraw(&output);
            handled = true;
        }

        let res = self.swayward.layout.overview_gesture_end();
        if res {
            self.swayward.queue_redraw_all();
            handled = true;
        }

        if handled {
            // We handled this event.
            return;
        }

        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_swipe_end(
            self,
            &GestureSwipeEndEvent {
                serial,
                time: event.time(),
                cancelled: event.cancelled(),
            },
        );
    }

    pub(in crate::input) fn on_gesture_pinch_begin<I: InputBackend>(
        &mut self,
        event: I::GesturePinchBeginEvent,
    ) {
        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_pinch_begin(
            self,
            &GesturePinchBeginEvent {
                serial,
                time: event.time(),
                fingers: event.fingers(),
            },
        );
    }

    pub(in crate::input) fn on_gesture_pinch_update<I: InputBackend>(
        &mut self,
        event: I::GesturePinchUpdateEvent,
    ) {
        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_pinch_update(
            self,
            &GesturePinchUpdateEvent {
                time: event.time(),
                delta: event.delta(),
                scale: event.scale(),
                rotation: event.rotation(),
            },
        );
    }

    pub(in crate::input) fn on_gesture_pinch_end<I: InputBackend>(
        &mut self,
        event: I::GesturePinchEndEvent,
    ) {
        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_pinch_end(
            self,
            &GesturePinchEndEvent {
                serial,
                time: event.time(),
                cancelled: event.cancelled(),
            },
        );
    }

    pub(in crate::input) fn on_gesture_hold_begin<I: InputBackend>(
        &mut self,
        event: I::GestureHoldBeginEvent,
    ) {
        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_hold_begin(
            self,
            &GestureHoldBeginEvent {
                serial,
                time: event.time(),
                fingers: event.fingers(),
            },
        );
    }

    pub(in crate::input) fn on_gesture_hold_end<I: InputBackend>(
        &mut self,
        event: I::GestureHoldEndEvent,
    ) {
        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.swayward.seat.get_pointer().unwrap();

        if self.update_pointer_contents() {
            pointer.frame(self);
        }

        pointer.gesture_hold_end(
            self,
            &GestureHoldEndEvent {
                serial,
                time: event.time(),
                cancelled: event.cancelled(),
            },
        );
    }
}

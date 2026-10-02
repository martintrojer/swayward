use super::*;

impl State {
    pub fn move_cursor(&mut self, location: Point<f64, Logical>) {
        let mut under = match self.swayward.pointer_visibility {
            PointerVisibility::Disabled => PointContents::default(),
            _ => self.swayward.contents_under(location),
        };

        // Disable the hidden pointer if the contents underneath have changed.
        if !self.swayward.pointer_visibility.is_visible() && self.swayward.pointer_contents != under
        {
            self.swayward.pointer_visibility = PointerVisibility::Disabled;

            // When setting PointerVisibility::Hidden together with pointer contents changing,
            // we can change straight to nothing to avoid one frame of hover. Notably, this can
            // be triggered through warp-mouse-to-focus combined with hide-when-typing.
            under = PointContents::default();
        }

        self.swayward.pointer_contents.clone_from(&under);

        let pointer = &self.swayward.seat.get_pointer().unwrap();
        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: InputTime::now(),
            },
        );
        pointer.frame(self);

        self.swayward.maybe_activate_pointer_constraint();

        // We do not show the pointer on programmatic or keyboard movement.

        // FIXME: granular
        self.swayward.queue_redraw_all();
    }

    fn move_cursor_to_rect(&mut self, rect: Rectangle<f64, Logical>, mode: CenterCoords) -> bool {
        let pointer = &self.swayward.seat.get_pointer().unwrap();
        let cur_loc = pointer.current_location();
        let x_in_bound = cur_loc.x >= rect.loc.x && cur_loc.x <= rect.loc.x + rect.size.w;
        let y_in_bound = cur_loc.y >= rect.loc.y && cur_loc.y <= rect.loc.y + rect.size.h;

        let p = match mode {
            CenterCoords::Separately => {
                if x_in_bound && y_in_bound {
                    return false;
                } else if y_in_bound {
                    // adjust x
                    Point::from((rect.loc.x + rect.size.w / 2.0, cur_loc.y))
                } else if x_in_bound {
                    // adjust y
                    Point::from((cur_loc.x, rect.loc.y + rect.size.h / 2.0))
                } else {
                    // adjust x and y
                    center_f64(rect)
                }
            }
            CenterCoords::Both => {
                if x_in_bound && y_in_bound {
                    return false;
                } else {
                    // adjust x and y
                    center_f64(rect)
                }
            }
            CenterCoords::BothAlways => center_f64(rect),
        };

        self.move_cursor(p);
        true
    }

    pub fn move_cursor_to_focused_tile(&mut self, mode: CenterCoords) -> bool {
        if !self.swayward.keyboard_focus.is_layout() {
            return false;
        }

        if self.swayward.tablet_cursor_location.is_some() {
            return false;
        }

        let Some(output) = self.swayward.layout.active_output() else {
            return false;
        };
        let monitor = self.swayward.layout.monitor_for_output(output).unwrap();

        let mut rv = false;
        let rect = monitor.active_window_visual_rectangle();

        if let Some(rect) = rect {
            let output_geo = self.swayward.global_space.output_geometry(output).unwrap();
            let mut rect = rect;
            rect.loc += output_geo.loc.to_f64();
            rv = self.move_cursor_to_rect(rect, mode);
        }

        rv
    }

    pub fn focus_default_monitor(&mut self) {
        // Our default target is the first output in sorted order.
        let Some(target) = self.swayward.sorted_outputs.first().cloned() else {
            // No outputs are connected.
            return;
        };

        if !self.focus_configured_monitor() {
            self.swayward.layout.focus_output(&target);
            self.move_cursor_to_output(&target);
        }
    }

    pub fn focus_startup_monitor(&mut self) {
        if self.focus_configured_monitor() {
            return;
        }
        let target = {
            let config = self.swayward.config.borrow();
            config
                .outputs
                .0
                .iter()
                .find_map(|config| self.swayward.output_by_name_match(&config.name))
                .cloned()
        };
        if let Some(target) = target {
            self.swayward.layout.focus_output(&target);
            self.move_cursor_to_output(&target);
        }
    }

    pub fn focus_configured_monitor(&mut self) -> bool {
        let target = {
            let config = self.swayward.config.borrow();
            config.outputs.0.iter().find_map(|config| {
                config
                    .focus_at_startup
                    .then(|| self.swayward.output_by_name_match(&config.name))
                    .flatten()
                    .cloned()
            })
        };
        let Some(target) = target else {
            return false;
        };

        self.swayward.layout.focus_output(&target);
        self.move_cursor_to_output(&target);
        true
    }

    pub fn focus_window(&mut self, window: &Window) {
        let active_output = self.swayward.layout.active_output().cloned();

        self.swayward.layout.activate_window(window);

        let new_active = self.swayward.layout.active_output().cloned();
        if new_active != active_output {
            if !self.maybe_warp_cursor_to_focus_centered() {
                self.move_cursor_to_output(&new_active.unwrap());
            }
        } else {
            self.maybe_warp_cursor_to_focus();
        }

        // FIXME: granular
        self.swayward.queue_redraw_all();
    }

    pub fn confirm_mru(&mut self) {
        if let Some(window) = self.swayward.close_mru(MruCloseRequest::Confirm) {
            // focus_window() will warp the cursor to the window only when the keyboard focus is on
            // the layout. However, right now the keyboard focus is still on the MRU (that we had
            // just closed) since it's only updated at the end of the event loop cycle. Force-update
            // the keyboard focus here to make cursor warping work.
            self.update_keyboard_focus();

            self.focus_window(&window);
        }
    }

    fn sway_warp_mode(&self) -> Option<CenterCoords> {
        match self.swayward.config.borrow().input.mouse_warping {
            swayward_config::input::MouseWarping::No => None,
            swayward_config::input::MouseWarping::Container => Some(CenterCoords::Both),
            swayward_config::input::MouseWarping::Output => {
                let output = self.swayward.layout.active_output()?;
                let geometry = self.swayward.global_space.output_geometry(output)?;
                let pointer = self.swayward.seat.get_pointer()?.current_location();
                if geometry.to_f64().contains(pointer) {
                    // The pointer is already on the focused output, so this
                    // mode leaves it alone.
                    None
                } else {
                    Some(CenterCoords::Both)
                }
            }
        }
    }

    pub fn maybe_warp_cursor_to_focus(&mut self) -> bool {
        if let Some(mode) = self.sway_warp_mode() {
            return self.move_cursor_to_focused_tile(mode);
        }
        let focused = match self.swayward.config.borrow().input.warp_mouse_to_focus {
            None => return false,
            Some(inner) => match inner.mode {
                None => CenterCoords::Separately,
                Some(WarpMouseToFocusMode::CenterXy) => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXyAlways) => CenterCoords::BothAlways,
            },
        };
        self.move_cursor_to_focused_tile(focused)
    }

    pub fn maybe_warp_cursor_to_focus_centered(&mut self) -> bool {
        if let Some(mode) = self.sway_warp_mode() {
            return self.move_cursor_to_focused_tile(mode);
        }
        let focused = match self.swayward.config.borrow().input.warp_mouse_to_focus {
            None => return false,
            Some(inner) => match inner.mode {
                None => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXy) => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXyAlways) => CenterCoords::BothAlways,
            },
        };
        self.move_cursor_to_focused_tile(focused)
    }

    pub fn refresh_pointer_contents(&mut self) {
        // Don't move the mouse pointer while the user is interacting with the tablet, as it causes
        // unwanted jumps for the client.
        if self.swayward.tablet_cursor_location.is_some() {
            return;
        }

        let _span = tracy_client::span!("Swayward::refresh_pointer_contents");

        let pointer = &self.swayward.seat.get_pointer().unwrap();
        let location = pointer.current_location();

        if !self.swayward.exit_confirm_dialog.is_open()
            && !self.swayward.is_locked()
            && !self.swayward.screenshot_ui.is_open()
        {
            // Don't refresh cursor focus during transitions.
            if let Some((output, _)) = self.swayward.output_under(location) {
                let monitor = self.swayward.layout.monitor_for_output(output).unwrap();
                if monitor.are_transitions_ongoing() {
                    return;
                }
            }
        }

        if !self.update_pointer_contents() {
            return;
        }

        pointer.frame(self);

        // Pointer motion from a surface to nothing triggers a cursor change to default, which
        // means we may need to redraw.

        // FIXME: granular
        self.swayward.queue_redraw_all();
    }

    pub fn update_pointer_contents(&mut self) -> bool {
        let _span = tracy_client::span!("Swayward::update_pointer_contents");

        let pointer = &self.swayward.seat.get_pointer().unwrap();
        let location = pointer.current_location();
        let mut under = match self.swayward.pointer_visibility {
            PointerVisibility::Disabled => PointContents::default(),
            _ => self.swayward.contents_under(location),
        };

        // We're not changing the global cursor location here, so if the contents did not change,
        // then nothing changed.
        if self.swayward.pointer_contents == under {
            return false;
        }

        // Disable the hidden pointer if the contents underneath have changed.
        if !self.swayward.pointer_visibility.is_visible() {
            self.swayward.pointer_visibility = PointerVisibility::Disabled;

            // When setting PointerVisibility::Hidden together with pointer contents changing,
            // we can change straight to nothing to avoid one frame of hover. Notably, this can
            // be triggered through warp-mouse-to-focus combined with hide-when-typing.
            under = PointContents::default();
            if self.swayward.pointer_contents == under {
                return false;
            }
        }

        self.swayward.pointer_contents.clone_from(&under);

        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: InputTime::now(),
            },
        );

        self.swayward.maybe_activate_pointer_constraint();

        true
    }

    pub fn move_cursor_to_output(&mut self, output: &Output) {
        let geo = self.swayward.global_space.output_geometry(output).unwrap();
        self.move_cursor(center(geo).to_f64());
    }

    pub fn refresh_popup_grab(&mut self) {
        if let Some(grab) = &mut self.swayward.popup_grab {
            if grab.grab.has_ended() {
                self.swayward.popup_grab = None;
            }
        }
    }

    pub fn modifier_state(&self) -> smithay::input::keyboard::ModifiersState {
        self.swayward
            .seat
            .get_keyboard()
            .map(|keyboard| keyboard.modifier_state())
            .unwrap_or_default()
    }
}

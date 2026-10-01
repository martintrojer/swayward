use super::*;

impl State {
    pub(in crate::input) fn on_pointer_motion<I: InputBackend>(
        &mut self,
        event: I::PointerMotionEvent,
    ) {
        let was_inside_hot_corner = self.swayward.pointer_inside_hot_corner;
        // Any of the early returns here mean that the pointer is not inside the hot corner.
        self.swayward.pointer_inside_hot_corner = false;

        // We need an output to be able to move the pointer.
        if self.swayward.global_space.outputs().next().is_none() {
            return;
        }

        let serial = SERIAL_COUNTER.next_serial();

        let pointer = self.swayward.seat.get_pointer().unwrap();

        let pos = pointer.current_location();

        // We have an output, so we can compute the new location and focus.
        let mut new_pos = pos + event.delta();

        // We received an event for the regular pointer, so show it now.
        self.swayward.pointer_visibility = PointerVisibility::Visible;
        self.swayward.tablet_cursor_location = None;

        // Check if we have an active pointer constraint.
        //
        // FIXME: ideally this should use the pointer focus with up-to-date global location.
        let mut pointer_confined = None;
        if let Some(under) = &self.swayward.pointer_contents.surface {
            // No need to check if the pointer focus surface matches, because here we're checking
            // for an already-active constraint, and the constraint is deactivated when the focused
            // surface changes.
            let pos_within_surface = pos - under.1;

            let mut pointer_locked = false;
            with_pointer_constraint(&under.0, &pointer, |constraint| {
                let Some(constraint) = constraint else { return };
                if !constraint.is_active() {
                    return;
                }

                // Constraint does not apply if not within region.
                if let Some(region) = constraint.region() {
                    if !region.contains(pos_within_surface.to_i32_round()) {
                        return;
                    }
                }

                match &*constraint {
                    PointerConstraint::Locked(_locked) => {
                        pointer_locked = true;
                    }
                    PointerConstraint::Confined(confine) => {
                        pointer_confined = Some((under.clone(), confine.region().cloned()));
                    }
                }
            });

            // If the pointer is locked, only send relative motion.
            if pointer_locked {
                pointer.relative_motion(
                    self,
                    Some(under.clone()),
                    &RelativeMotionEvent {
                        delta: event.delta(),
                        delta_unaccel: event.delta_unaccel(),
                        time: event.time(),
                    },
                );

                pointer.frame(self);

                // I guess a redraw to hide the tablet cursor could be nice? Doesn't matter too
                // much here I think.
                return;
            }
        }

        // Warp pointer across the screen during the spatial movement grabs.
        let spatial_grab = pointer.with_grab(|_, grab| {
            let grab = grab.as_any();
            if let Some(grab) = grab.downcast_ref::<SpatialMovementGrab>() {
                if let Some(output) = grab.view_offset_output() {
                    return Some((output.clone(), true));
                }
            } else if let Some(grab) = grab.downcast_ref::<MoveGrab>() {
                if let Some(output) = grab.view_offset_output() {
                    return Some((output.clone(), true));
                }
            }
            None
        });
        if let Some((output, horizontal)) = spatial_grab.flatten() {
            if let Some(geo) = self.swayward.global_space.output_geometry(&output) {
                let geo = geo.to_f64();
                if horizontal {
                    new_pos.x = (new_pos.x - geo.loc.x).rem_euclid(geo.size.w) + geo.loc.x;
                    new_pos.y = new_pos.y.clamp(geo.loc.y, geo.loc.y + geo.size.h - 1.);
                } else {
                    new_pos.x = new_pos.x.clamp(geo.loc.x, geo.loc.x + geo.size.w - 1.);
                    new_pos.y = (new_pos.y - geo.loc.y).rem_euclid(geo.size.h) + geo.loc.y;
                }
            }
        }

        if self
            .swayward
            .global_space
            .output_under(new_pos)
            .next()
            .is_none()
        {
            // We ended up outside the outputs and need to clip the movement.
            if let Some(output) = self.swayward.global_space.output_under(pos).next() {
                // The pointer was previously on some output. Clip the movement against its
                // boundaries.
                let geom = self.swayward.global_space.output_geometry(output).unwrap();
                new_pos.x = new_pos
                    .x
                    .clamp(geom.loc.x as f64, (geom.loc.x + geom.size.w - 1) as f64);
                new_pos.y = new_pos
                    .y
                    .clamp(geom.loc.y as f64, (geom.loc.y + geom.size.h - 1) as f64);
            } else {
                // The pointer was not on any output in the first place. Find one for it.
                // Let's do the simple thing and just put it on the first output.
                let output = self.swayward.global_space.outputs().next().unwrap();
                let geom = self.swayward.global_space.output_geometry(output).unwrap();
                new_pos = center(geom).to_f64();
            }
        }

        if let Some(output) = self.swayward.screenshot_ui.selection_output() {
            let geom = self.swayward.global_space.output_geometry(output).unwrap();
            let point = (new_pos - geom.loc.to_f64())
                .to_physical(output.current_scale().fractional_scale())
                .to_i32_round::<i32>();

            self.swayward.screenshot_ui.pointer_motion(point, None);
        }

        if let Some(mru_output) = self.swayward.window_mru_ui.output() {
            if let Some((output, pos_within_output)) = self.swayward.output_under(new_pos) {
                if mru_output == output {
                    self.swayward
                        .window_mru_ui
                        .pointer_motion(pos_within_output);
                }
            }
        }

        let under = self.swayward.contents_under(new_pos);

        // Handle confined pointer.
        if let Some((focus_surface, region)) = pointer_confined {
            let mut prevent = false;

            // Prevent the pointer from leaving the focused surface.
            if Some(&focus_surface.0) != under.surface.as_ref().map(|(s, _)| s) {
                prevent = true;
            }

            // Prevent the pointer from leaving the confine region, if any.
            if let Some(region) = region {
                let new_pos_within_surface = new_pos - focus_surface.1;
                if !region.contains(new_pos_within_surface.to_i32_round()) {
                    prevent = true;
                }
            }

            if prevent {
                pointer.relative_motion(
                    self,
                    Some(focus_surface),
                    &RelativeMotionEvent {
                        delta: event.delta(),
                        delta_unaccel: event.delta_unaccel(),
                        time: event.time(),
                    },
                );

                pointer.frame(self);

                return;
            }
        }

        self.swayward.handle_focus_follows_mouse(&under);

        self.swayward.pointer_contents.clone_from(&under);

        pointer.motion(
            self,
            under.surface.clone(),
            &MotionEvent {
                location: new_pos,
                serial,
                time: event.time(),
            },
        );

        pointer.relative_motion(
            self,
            under.surface,
            &RelativeMotionEvent {
                delta: event.delta(),
                delta_unaccel: event.delta_unaccel(),
                time: event.time(),
            },
        );

        pointer.frame(self);

        // contents_under() will return no surface when the hot corner should trigger, so
        // pointer.motion() will set the current focus to None.
        if under.hot_corner && pointer.current_focus().is_none() {
            if !was_inside_hot_corner
                && pointer
                    .with_grab(|_, grab| grab_allows_hot_corner(grab))
                    .unwrap_or(true)
            {
                self.swayward.layout.toggle_overview();
            }
            self.swayward.pointer_inside_hot_corner = true;
        }

        self.update_border_resize_cursor(&pointer);

        // Activate a new confinement if necessary.
        self.swayward.maybe_activate_pointer_constraint();

        // Inform the layout of an ongoing DnD operation.
        let is_dnd_grab = pointer
            .with_grab(|_, grab| Self::is_dnd_grab(grab.as_any()))
            .unwrap_or(false);
        if is_dnd_grab {
            if let Some((output, pos_within_output)) = self.swayward.output_under(new_pos) {
                let output = output.clone();
                self.swayward.layout.dnd_update(output, pos_within_output);
            }
        }

        // Notify a11y.
        #[cfg(feature = "dbus")]
        self.a11y_notify_pointer_motion();

        // Redraw to update the cursor position.
        // FIXME: redraw only outputs overlapping the cursor.
        self.swayward.queue_redraw_all();
    }

    pub(in crate::input) fn on_pointer_motion_absolute<I: InputBackend>(
        &mut self,
        event: I::PointerMotionAbsoluteEvent,
    ) {
        let was_inside_hot_corner = self.swayward.pointer_inside_hot_corner;
        // Any of the early returns here mean that the pointer is not inside the hot corner.
        self.swayward.pointer_inside_hot_corner = false;

        let Some(pos) = self.compute_absolute_location(&event, None).or_else(|| {
            self.global_bounding_rectangle().map(|output_geo| {
                event.position_transformed(output_geo.size) + output_geo.loc.to_f64()
            })
        }) else {
            return;
        };

        let serial = SERIAL_COUNTER.next_serial();

        let pointer = self.swayward.seat.get_pointer().unwrap();

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

        self.swayward.handle_focus_follows_mouse(&under);

        self.swayward.pointer_contents.clone_from(&under);

        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location: pos,
                serial,
                time: event.time(),
            },
        );

        pointer.frame(self);

        // contents_under() will return no surface when the hot corner should trigger, so
        // pointer.motion() will set the current focus to None.
        if under.hot_corner && pointer.current_focus().is_none() {
            if !was_inside_hot_corner
                && pointer
                    .with_grab(|_, grab| grab_allows_hot_corner(grab))
                    .unwrap_or(true)
            {
                self.swayward.layout.toggle_overview();
            }
            self.swayward.pointer_inside_hot_corner = true;
        }

        self.update_border_resize_cursor(&pointer);

        self.swayward.maybe_activate_pointer_constraint();

        // We moved the pointer, show it.
        self.swayward.pointer_visibility = PointerVisibility::Visible;

        // We moved the regular pointer, so show it now.
        self.swayward.tablet_cursor_location = None;

        // Inform the layout of an ongoing DnD operation.
        let is_dnd_grab = pointer
            .with_grab(|_, grab| Self::is_dnd_grab(grab.as_any()))
            .unwrap_or(false);
        if is_dnd_grab {
            if let Some((output, pos_within_output)) = self.swayward.output_under(pos) {
                let output = output.clone();
                self.swayward.layout.dnd_update(output, pos_within_output);
            }
        }

        // Notify a11y.
        #[cfg(feature = "dbus")]
        self.a11y_notify_pointer_motion();

        // Redraw to update the cursor position.
        // FIXME: redraw only outputs overlapping the cursor.
        self.swayward.queue_redraw_all();
    }

    pub(in crate::input) fn update_border_resize_cursor(&mut self, pointer: &PointerHandle<State>) {
        if pointer.is_grabbed() {
            return;
        }
        let icon = self
            .border_resize_edges_under_pointer(pointer)
            .or_else(|| {
                self.gap_resize_edges_under_pointer(pointer)
                    .map(|(_, edges, _)| (false, edges))
            })
            .map(|(floating, edges)| {
                if floating {
                    edges.cursor_icon()
                } else if edges.intersects(ResizeEdge::LEFT_RIGHT) {
                    CursorIcon::ColResize
                } else {
                    CursorIcon::RowResize
                }
            });
        if let Some(icon) = icon {
            self.swayward.border_resize_cursor = true;
            self.swayward
                .cursor_manager
                .set_cursor_image(CursorImageStatus::Named(icon));
        } else if std::mem::take(&mut self.swayward.border_resize_cursor)
            && self.swayward.pointer_contents.surface.is_none()
        {
            // A surface under the pointer sets its own cursor on enter.
            self.swayward
                .cursor_manager
                .set_cursor_image(CursorImageStatus::default_named());
        }
    }
}

use super::*;

#[path = "pointer/scroll_binds.rs"]
mod scroll_binds;
use scroll_binds::synthetic_bind;

impl State {
    pub(super) fn on_pointer_motion<I: InputBackend>(&mut self, event: I::PointerMotionEvent) {
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

    pub(super) fn on_pointer_motion_absolute<I: InputBackend>(
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

    /// Shows sway's resize cursor over a resizable border
    /// (`sway/sway/input/cursor.c`, `cursor_update_image`): directional for
    /// floating windows, `col-resize` or `row-resize` for tiled ones.
    pub(super) fn update_border_resize_cursor(&mut self, pointer: &PointerHandle<State>) {
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

    /// Activates `window` and starts a pointer resize of `edges`, shared by
    /// the border and gap handles.
    pub(super) fn begin_edge_resize(
        &mut self,
        pointer: &PointerHandle<State>,
        window: Window,
        edges: ResizeEdge,
        location: Point<f64, Logical>,
        button_code: u32,
        serial: Serial,
    ) {
        self.swayward.layout.activate_window(&window);
        if !self
            .swayward
            .layout
            .interactive_resize_begin(window.clone(), edges)
        {
            return;
        }
        let start_data = PointerGrabStartData {
            focus: None,
            button: button_code,
            location,
        };
        let grab = ResizeGrab::new(AnyStartData::Pointer(start_data), window);
        pointer.set_grab(self, grab, serial, Focus::Clear);
        self.swayward
            .cursor_manager
            .set_cursor_image(CursorImageStatus::Named(edges.cursor_icon()));
    }

    /// The tiled window, edge and pointer location a plain left press in the
    /// gap under the pointer resizes, when `input { gap-resize }` is on.
    pub(super) fn gap_resize_edges_under_pointer(
        &self,
        pointer: &PointerHandle<State>,
    ) -> Option<(Window, ResizeEdge, Point<f64, Logical>)> {
        if !self.swayward.config.borrow().input.gap_resize
            || self.swayward.pointer_contents.window.is_some()
            || self.swayward.pointer_contents.layer.is_some()
            || self.swayward.layout.is_overview_open()
        {
            return None;
        }
        let location = pointer.current_location();
        let (output, pos) = self.swayward.output_under(location)?;
        let (mapped, edges) = self.swayward.layout.gap_resize_edges_under(output, pos)?;
        Some((mapped.window.clone(), edges, location))
    }

    /// Whether the pointer is over a border a plain left press resizes, and
    /// whether that window floats.
    pub(super) fn border_resize_edges_under_pointer(
        &self,
        pointer: &PointerHandle<State>,
    ) -> Option<(bool, ResizeEdge)> {
        if !self.swayward.config.borrow().input.border_resize
            || self.swayward.pointer_contents.surface.is_some()
            || self.swayward.layout.is_overview_open()
        {
            return None;
        }
        let (window, _) = self.swayward.pointer_contents.window.as_ref()?;
        let (output, pos) = self.swayward.output_under(pointer.current_location())?;
        let (mapped, edges) = self
            .swayward
            .layout
            .border_resize_edges_under(output, pos)?;
        (&mapped.window == window).then_some((mapped.is_floating(), edges))
    }

    pub(super) fn on_pointer_button<I: InputBackend>(&mut self, event: I::PointerButtonEvent) {
        let pointer = self.swayward.seat.get_pointer().unwrap();

        let serial = SERIAL_COUNTER.next_serial();

        let button = event.button();
        let input_device = event.device().sway_identifier();

        let button_code = event.button_code();

        let button_state = event.state();

        let mod_key = self.backend.mod_key(&self.swayward.config.borrow());

        if ButtonState::Released == button_state {
            let suppressed = self.swayward.suppressed_buttons.remove(&button_code);
            if let Some(bind) = self
                .swayward
                .held_release_buttons
                .remove(&(input_device.clone(), button_code))
            {
                self.handle_bind(bind);
                return;
            }
            // Ignore release events for mouse clicks that triggered a press bind.
            if suppressed {
                return;
            }
        }

        let mods = self.modifier_state();
        let modifiers = modifiers_from_state(mods);
        let mod_down = mod_key.is_pressed(modifiers);

        // Sway's `floating_modifier` is its own setting, independent of the
        // binding modifier, and it carries an inverse bit that swaps the move
        // and resize buttons (`sway/sway/input/seatop_default.c:359-363`).
        // When no command has set one, swayward keeps its inherited
        // behaviour: the compositor mod key, left to move, right to resize.
        let (drag_mod_down, drag_move_button, drag_resize_button) =
            match self.swayward.config.borrow().input.floating_modifier {
                None => (mod_down, MouseButton::Left, MouseButton::Right),
                Some(floating) => {
                    // `floating_modifier none` stores ModKey::None, whose
                    // is_pressed is always false, so the drag is off.
                    let down = floating.modifier.is_pressed(modifiers);
                    if floating.inverse {
                        (down, MouseButton::Right, MouseButton::Left)
                    } else {
                        (down, MouseButton::Left, MouseButton::Right)
                    }
                }
            };

        if ButtonState::Pressed == button_state {
            if let Some(mru_output) = self.swayward.window_mru_ui.output() {
                if let Some(MouseButton::Left) = button {
                    let location = pointer.current_location();
                    let (output, pos_within_output) = self.swayward.output_under(location).unwrap();
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

                    self.swayward.suppressed_buttons.insert(button_code);
                    return;
                }
            }

            {
                if let Some(bind) = match button {
                    Some(MouseButton::Left) => Some(Trigger::MouseLeft),
                    Some(MouseButton::Right) => Some(Trigger::MouseRight),
                    Some(MouseButton::Middle) => Some(Trigger::MouseMiddle),
                    Some(MouseButton::Back) => Some(Trigger::MouseBack),
                    Some(MouseButton::Forward) => Some(Trigger::MouseForward),
                    _ => None,
                }
                .map(|trigger| {
                    let config = self.swayward.config.borrow();
                    let bindings = make_binds_iter(
                        &config,
                        &self.swayward.binding_mode,
                        &mut self.swayward.window_mru_ui,
                        modifiers,
                    );
                    let release = find_configured_bind_for_device(
                        bindings.clone().filter(|bind| bind.release),
                        mod_key,
                        trigger,
                        mods,
                        &input_device,
                    );
                    let press = find_configured_bind_for_device(
                        bindings.filter(|bind| !bind.release),
                        mod_key,
                        trigger,
                        mods,
                        &input_device,
                    );
                    (press, release)
                })
                .map(|(press, release)| {
                    let allowed = |bind: &Bind| {
                        self.mouse_bind_matches_region(bind)
                            && (!self.swayward.screenshot_ui.is_open()
                                || allowed_during_screenshot(&bind.action))
                    };
                    (press.filter(allowed), release.filter(allowed))
                })
                .and_then(|(press, release)| {
                    if let Some(release) = release {
                        self.swayward
                            .held_release_buttons
                            .insert((input_device.clone(), button_code), release);
                    }
                    press
                }) {
                    self.swayward.suppressed_buttons.insert(button_code);
                    self.handle_bind(bind.clone());
                    return;
                }
                if self
                    .swayward
                    .held_release_buttons
                    .contains_key(&(input_device, button_code))
                {
                    return;
                }
            }

            // We received an event for the regular pointer, so show it now.
            self.swayward.pointer_visibility = PointerVisibility::Visible;
            self.swayward.tablet_cursor_location = None;

            let is_overview_open = self.swayward.layout.is_overview_open();

            if is_overview_open && !pointer.is_grabbed() && button == Some(MouseButton::Right) {
                if let Some((output, ws)) = self.swayward.workspace_under_cursor(true) {
                    let ws_id = ws.id();
                    let ws_idx = self.swayward.layout.find_workspace_by_id(ws_id).unwrap().0;

                    self.swayward.layout.focus_output(&output);

                    let location = pointer.current_location();
                    let start_data = PointerGrabStartData {
                        focus: None,
                        button: button_code,
                        location,
                    };
                    self.swayward
                        .layout
                        .view_offset_gesture_begin(&output, Some(ws_idx), false);
                    let grab = SpatialMovementGrab::new(start_data, output, ws_id, true);
                    pointer.set_grab(self, grab, serial, Focus::Clear);
                    self.swayward
                        .cursor_manager
                        .set_cursor_image(CursorImageStatus::Named(CursorIcon::AllScroll));

                    // FIXME: granular.
                    self.swayward.queue_redraw_all();
                    return;
                }
            }

            if button == Some(MouseButton::Middle) && !pointer.is_grabbed() && mod_down {
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
                    let ws_id = ws.id();

                    self.swayward.layout.focus_output(&output);

                    let location = pointer.current_location();
                    let start_data = PointerGrabStartData {
                        focus: None,
                        button: button_code,
                        location,
                    };
                    let grab = SpatialMovementGrab::new(start_data, output, ws_id, false);
                    pointer.set_grab(self, grab, serial, Focus::Clear);
                    self.swayward
                        .cursor_manager
                        .set_cursor_image(CursorImageStatus::Named(CursorIcon::AllScroll));

                    // FIXME: granular.
                    self.swayward.queue_redraw_all();

                    // Don't activate the window under the cursor to avoid unnecessary
                    // scrolling when e.g. Mod+MMB clicking on a partially off-screen window.
                    return;
                }
            }

            if let Some(mapped) = self.swayward.window_under_cursor() {
                let window = mapped.window.clone();

                // Check if we need to start an interactive move. The overview
                // is niri's and click-to-move there stays on the left button;
                // only the floating drag follows sway's inverse bit.
                let overview_move = is_overview_open && button == Some(MouseButton::Left);
                let is_tiling = !mapped.is_floating();
                let on_titlebar =
                    self.swayward
                        .pointer_contents
                        .window
                        .as_ref()
                        .is_some_and(|(_, hit)| {
                            matches!(
                                hit,
                                HitType::Activate {
                                    is_tab_indicator: true
                                }
                            )
                        });
                // Sway gates tiled modifier and titlebar drags independently of floating moves
                // (`sway/input/seatop_default.c:490-500`).
                let regular_move = !mapped.pending_sizing_mode().is_fullscreen()
                    && if is_tiling {
                        self.swayward.config.borrow().input.tiling_drag
                            && ((button == Some(drag_move_button) && drag_mod_down)
                                || (button == Some(MouseButton::Left) && on_titlebar))
                    } else {
                        button == Some(drag_move_button) && drag_mod_down
                    };
                // Sway resizes from a border on a plain left press, tiled before any modifier
                // move and floating after one (`sway/sway/input/seatop_default.c:396-474`).
                let border_resize = (!is_overview_open
                    && button == Some(MouseButton::Left)
                    && !pointer.is_grabbed()
                    && self.swayward.config.borrow().input.border_resize
                    && (is_tiling || !regular_move))
                    .then(|| {
                        let location = pointer.current_location();
                        let (output, pos) = self.swayward.output_under(location)?;
                        let (target, edges) = self
                            .swayward
                            .layout
                            .border_resize_edges_under(output, pos)?;
                        (target.window == window).then_some((location, edges))
                    })
                    .flatten();
                if let Some((location, edges)) = border_resize {
                    self.begin_edge_resize(
                        &pointer,
                        window.clone(),
                        edges,
                        location,
                        button_code,
                        serial,
                    );
                } else if (overview_move || regular_move) && !pointer.is_grabbed() {
                    let location = pointer.current_location();

                    if !is_overview_open {
                        self.swayward.layout.activate_window(&window);
                    }

                    let start_data = PointerGrabStartData {
                        focus: None,
                        button: button_code,
                        location,
                    };
                    let start_data = AnyStartData::Pointer(start_data);
                    let icon = CursorIcon::Grabbing;
                    let grab = if is_tiling {
                        let threshold = if drag_mod_down {
                            0.
                        } else {
                            self.swayward
                                .config
                                .borrow()
                                .input
                                .tiling_drag_threshold
                                .into()
                        };
                        MoveGrab::new_tiling(
                            self,
                            start_data,
                            window.clone(),
                            Some(icon),
                            threshold,
                        )
                    } else {
                        MoveGrab::new(self, start_data, window.clone(), false, Some(icon))
                    };
                    if let Some(grab) = grab {
                        pointer.set_grab(self, grab, serial, Focus::Clear);

                        // Set the cursor to Grabbing right away for Mod+LMB since it doesn't
                        // do any other gesture.
                        //
                        // In the overview, we click to activate window and close the overview,
                        // in this case setting the cursor right away would be distracting.
                        if !is_overview_open && (!is_tiling || drag_mod_down) {
                            self.swayward
                                .cursor_manager
                                .set_cursor_image(CursorImageStatus::Named(icon));
                        }
                    }
                }
                // Check if we need to start an interactive resize.
                else if button == Some(drag_resize_button)
                    && !pointer.is_grabbed()
                    && drag_mod_down
                {
                    let location = pointer.current_location();
                    let (output, pos_within_output) = self.swayward.output_under(location).unwrap();
                    let edges = self
                        .swayward
                        .layout
                        .resize_edges_under(output, pos_within_output)
                        .unwrap_or(ResizeEdge::empty());

                    // Sway has no double-click gestures here: every press
                    // resizes from the corner under the pointer.
                    if !edges.is_empty() {
                        self.begin_edge_resize(
                            &pointer,
                            window.clone(),
                            edges,
                            location,
                            button_code,
                            serial,
                        );
                    }
                }

                if !is_overview_open {
                    self.swayward.layout.activate_window(&window);
                }

                // FIXME: granular.
                self.swayward.queue_redraw_all();
            } else if let Some((output, ws)) = is_overview_open
                .then(|| self.swayward.workspace_under_cursor(false))
                .flatten()
            {
                let ws_idx = self
                    .swayward
                    .layout
                    .find_workspace_by_id(ws.id())
                    .unwrap()
                    .0;

                self.swayward.layout.focus_output(&output);
                self.swayward.layout.toggle_overview_to_workspace(ws_idx);

                // FIXME: granular.
                self.swayward.queue_redraw_all();
            } else if let Some((window, edges, location)) = (button == Some(MouseButton::Left)
                && !pointer.is_grabbed())
            .then(|| self.gap_resize_edges_under_pointer(&pointer))
            .flatten()
            {
                self.begin_edge_resize(&pointer, window, edges, location, button_code, serial);
                // FIXME: granular.
                self.swayward.queue_redraw_all();
            } else if let Some(output) = self.swayward.output_under_cursor() {
                self.swayward.layout.focus_output(&output);

                // FIXME: granular.
                self.swayward.queue_redraw_all();
            }
        };

        self.update_pointer_contents();

        if ButtonState::Pressed == button_state {
            let layer_under = self.swayward.pointer_contents.layer.clone();
            self.swayward.focus_layer_surface_if_on_demand(layer_under);
        }

        if button == Some(MouseButton::Left) && self.swayward.screenshot_ui.is_open() {
            if button_state == ButtonState::Pressed {
                let pos = pointer.current_location();

                // If we'll be moving the existing selection, use the selection output.
                let output = if mod_down {
                    self.swayward.screenshot_ui.selection_output()
                } else {
                    self.swayward.output_under(pos).map(|(out, _)| out)
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
            } else if let Some(capture) = self.swayward.screenshot_ui.pointer_up(None) {
                if capture {
                    self.confirm_screenshot(true);
                } else {
                    self.swayward.queue_redraw_all();
                }
            }
        }

        pointer.button(
            self,
            &ButtonEvent {
                button: button_code,
                state: button_state,
                serial,
                time: event.time(),
            },
        );
        pointer.frame(self);
    }

    pub(super) fn mouse_bind_matches_region(&self, bind: &Bind) -> bool {
        if bind.mouse_regions.is_empty() {
            return true;
        }

        let contents = self
            .swayward
            .contents_under(self.swayward.seat.get_pointer().unwrap().current_location());
        let (click_region, on_workspace) = match contents.window.as_ref().map(|(_, hit)| hit) {
            Some(HitType::Input { .. }) => (MouseRegions::CONTENTS, false),
            Some(HitType::Activate {
                is_tab_indicator: true,
            }) => (MouseRegions::TITLEBAR, false),
            Some(HitType::Activate {
                is_tab_indicator: false,
            }) => (MouseRegions::BORDER, false),
            None if contents.layer.is_none() => (MouseRegions::all(), true),
            None => (MouseRegions::empty(), false),
        };

        mouse_regions_match(bind.mouse_regions, click_region, on_workspace)
    }

    pub(super) fn on_pointer_axis<I: InputBackend>(&mut self, event: I::PointerAxisEvent) {
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

    pub(super) fn on_gesture_swipe_begin<I: InputBackend>(
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

    pub(super) fn on_gesture_swipe_update<I: InputBackend + 'static>(
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

    pub(super) fn on_gesture_swipe_end<I: InputBackend>(&mut self, event: I::GestureSwipeEndEvent) {
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

    pub(super) fn on_gesture_pinch_begin<I: InputBackend>(
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

    pub(super) fn on_gesture_pinch_update<I: InputBackend>(
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

    pub(super) fn on_gesture_pinch_end<I: InputBackend>(&mut self, event: I::GesturePinchEndEvent) {
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

    pub(super) fn on_gesture_hold_begin<I: InputBackend>(
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

    pub(super) fn on_gesture_hold_end<I: InputBackend>(&mut self, event: I::GestureHoldEndEvent) {
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

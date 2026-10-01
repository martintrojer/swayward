use super::*;

impl State {
    pub(in crate::input) fn begin_edge_resize(
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

    pub(in crate::input) fn gap_resize_edges_under_pointer(
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

    pub(in crate::input) fn border_resize_edges_under_pointer(
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

    pub(in crate::input) fn on_pointer_button<I: InputBackend>(
        &mut self,
        event: I::PointerButtonEvent,
    ) {
        let pointer = self.swayward.seat.get_pointer().unwrap();

        let serial = SERIAL_COUNTER.next_serial();

        let button = event.button();
        let input_device = event.device().sway_identifier();

        let button_code = event.button_code();

        let button_state = event.state();

        let mod_key = self.backend.mod_key(&self.swayward.config.borrow());

        if ButtonState::Released == button_state
            && self.take_release_button_bind(&input_device, button_code)
        {
            // Ignore releases for release binds and clicks that triggered a
            // press bind.
            return;
        }

        let mods = self.modifier_state();
        let modifiers = modifiers_from_state(mods);
        let mod_down = mod_key.is_pressed(modifiers);
        let drag_policy = floating_drag_policy(
            self.swayward.config.borrow().input.floating_modifier,
            mod_key,
            modifiers,
        );

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

            if let Some(bind) = self.resolve_button_bind(
                button,
                button_code,
                &input_device,
                mod_key,
                mods,
                modifiers,
            ) {
                self.swayward.suppressed_buttons.insert(button_code);
                self.handle_bind(bind);
                return;
            }
            if self
                .swayward
                .held_release_buttons
                .contains_key(&(input_device, button_code))
            {
                return;
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
                let border_resize = self
                    .swayward
                    .config
                    .borrow()
                    .input
                    .border_resize
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
                let (tiling_drag, tiling_drag_threshold) = {
                    let input = &self.swayward.config.borrow().input;
                    (input.tiling_drag, input.tiling_drag_threshold.into())
                };
                let intent = classify_press(
                    button,
                    drag_policy,
                    is_tiling,
                    mapped.pending_sizing_mode().is_fullscreen(),
                    on_titlebar,
                    is_overview_open,
                    pointer.is_grabbed(),
                    tiling_drag,
                    tiling_drag_threshold,
                    border_resize,
                );

                match intent {
                    PressIntent::BorderResize(location, edges) => {
                        self.begin_edge_resize(
                            &pointer,
                            window.clone(),
                            edges,
                            location,
                            button_code,
                            serial,
                        );
                    }
                    PressIntent::Move { tiling, threshold } => {
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
                        let grab = if tiling {
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
                            // Set the cursor immediately for modifier drags;
                            // overview click activation keeps the normal icon.
                            if !is_overview_open && (!tiling || drag_policy.mod_down) {
                                self.swayward
                                    .cursor_manager
                                    .set_cursor_image(CursorImageStatus::Named(icon));
                            }
                        }
                    }
                    PressIntent::CornerResize => {
                        let location = pointer.current_location();
                        let (output, pos_within_output) =
                            self.swayward.output_under(location).unwrap();
                        let edges = self
                            .swayward
                            .layout
                            .resize_edges_under(output, pos_within_output)
                            .unwrap_or(ResizeEdge::empty());
                        // Sway has no double-click gesture here: every press
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
                    PressIntent::None => {}
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
}

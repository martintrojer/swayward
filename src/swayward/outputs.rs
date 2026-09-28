use super::*;

impl Swayward {
    pub fn reposition_outputs(&mut self, new_output: Option<&Output>) {
        let _span = tracy_client::span!("Swayward::reposition_outputs");

        #[derive(Debug)]
        struct Data {
            output: Output,
            name: OutputName,
            position: Option<Point<i32, Logical>>,
            config: Option<swayward_config::Position>,
        }

        let config = self.config.borrow();
        let mut outputs = vec![];
        for output in self.global_space.outputs().chain(new_output) {
            let name = output.user_data().get::<OutputName>().unwrap();
            let position = self.global_space.output_geometry(output).map(|geo| geo.loc);
            let config = config.outputs.find(name).and_then(|c| c.position);

            outputs.push(Data {
                output: output.clone(),
                name: name.clone(),
                position,
                config,
            });
        }
        drop(config);

        for Data { output, .. } in &outputs {
            self.global_space.unmap_output(output);
        }

        // Connectors can appear in udev in any order. If we sort by name then we get output
        // positioning that does not depend on the order they appeared.
        //
        // This sorting first compares by make/model/serial so that it is stable regardless of the
        // connector name. However, if make/model/serial is equal or unknown, then it does fall
        // back to comparing the connector name, which should always be unique.
        outputs.sort_unstable_by(|a, b| a.name.compare(&b.name));

        // Place all outputs with explicitly configured position first, then the unconfigured ones.
        outputs.sort_by_key(|d| d.config.is_none());

        trace!(
            "placing outputs in order: {:?}",
            outputs.iter().map(|d| &d.name.connector)
        );

        self.sorted_outputs = outputs
            .iter()
            .map(|Data { output, .. }| output.clone())
            .collect();

        for data in outputs.into_iter() {
            let Data {
                output,
                name,
                position,
                config,
            } = data;

            let size = output_size(&output).to_i32_round();

            let new_position = config
                .map(|pos| Point::from((pos.x, pos.y)))
                .filter(|pos| {
                    // Ensure that the requested position does not overlap any existing output.
                    let target_geom = Rectangle::new(*pos, size);

                    let overlap = self
                        .global_space
                        .outputs()
                        .map(|output| self.global_space.output_geometry(output).unwrap())
                        .find(|geom| geom.overlaps(target_geom));

                    if let Some(overlap) = overlap {
                        warn!(
                            "output {} at x={} y={} sized {}x{} \
                             overlaps an existing output at x={} y={} sized {}x{}, \
                             falling back to automatic placement",
                            name.connector,
                            pos.x,
                            pos.y,
                            size.w,
                            size.h,
                            overlap.loc.x,
                            overlap.loc.y,
                            overlap.size.w,
                            overlap.size.h,
                        );

                        false
                    } else {
                        true
                    }
                })
                .unwrap_or_else(|| {
                    let x = self
                        .global_space
                        .outputs()
                        .map(|output| self.global_space.output_geometry(output).unwrap())
                        .map(|geom| geom.loc.x + geom.size.w)
                        .max()
                        .unwrap_or(0);

                    Point::from((x, 0))
                });

            self.global_space.map_output(&output, new_position);

            // By passing new_output as an Option, rather than mapping it into a bogus location
            // in global_space, we ensure that this branch always runs for it.
            if Some(new_position) != position {
                debug!(
                    "putting output {} at x={} y={}",
                    name.connector, new_position.x, new_position.y
                );
                output.change_current_state(None, None, None, Some(new_position));
                self.ipc_outputs_changed = true;
                self.queue_redraw(&output);
            }
        }
    }

    pub fn add_output(&mut self, output: Output, refresh_interval: Option<Duration>, vrr: bool) {
        let global = output.create_global::<State>(&self.display_handle);

        let name = output.user_data().get::<OutputName>().unwrap();

        let config = self.config.borrow();
        let c = config.outputs.find(name);
        let scale = c.and_then(|c| c.scale).map(|s| s.0).unwrap_or_else(|| {
            let size_mm = output.physical_properties().size;
            let resolution = output.current_mode().unwrap().size;
            guess_monitor_scale(size_mm, resolution)
        });
        let scale = closest_representable_scale(scale.clamp(0.1, 10.));

        let mut transform = panel_orientation(&output)
            + c.map(|c| ipc_transform_to_smithay(c.transform))
                .unwrap_or(Transform::Normal);

        let mut backdrop_color = c
            .and_then(|c| c.backdrop_color)
            .unwrap_or(config.overview.backdrop_color)
            .to_array_unpremul();
        backdrop_color[3] = 1.;

        // FIXME: fix winit damage on other transforms.
        if name.connector == "winit" {
            transform = Transform::Flipped180;
        }

        let mut layout_config = c.and_then(|c| c.layout.clone());
        // Support the deprecated non-layout background-color key.
        if let Some(layout) = &mut layout_config {
            if layout.background_color.is_none() {
                layout.background_color = c.and_then(|c| c.background_color);
            }
        }
        drop(config);

        // Set scale and transform before adding to the layout since that will read the output size.
        output.change_current_state(
            None,
            Some(transform),
            Some(output::Scale::Fractional(scale)),
            None,
        );

        self.layout.add_output(output.clone(), layout_config);

        let lock_render_state = if self.is_locked() {
            // We haven't rendered anything yet so it's as good as locked.
            LockRenderState::Locked
        } else {
            LockRenderState::Unlocked
        };

        let size = output_size(&output);
        let state = OutputState {
            global,
            redraw_state: RedrawState::Idle,
            on_demand_vrr_enabled: false,
            unfinished_animations_remain: false,
            frame_clock: FrameClock::new(refresh_interval, vrr),
            last_drm_sequence: None,
            vblank_throttle: VBlankThrottle::new(self.event_loop.clone(), name.connector.clone()),
            frame_callback_sequence: 0,
            backdrop_buffer: SolidColorBuffer::new(size, backdrop_color),
            xray: Xray::new(),
            lock_render_state,
            lock_surface: None,
            lock_color_buffer: SolidColorBuffer::new(size, CLEAR_COLOR_LOCKED),
            screen_transition: None,
            debug_damage_tracker: OutputDamageTracker::from_output(&output),
        };
        let rv = self.output_state.insert(output.clone(), state);
        assert!(rv.is_none(), "output was already tracked");

        // Must be last since it will call queue_redraw(output) which needs things to be filled-in.
        self.reposition_outputs(Some(&output));
    }

    pub fn ipc_output_changed(&self) {
        if let Some(server) = &self.ipc_server {
            server.send_event(swayward_ipc::legacy::Event::OutputChanged);
        }
    }

    pub fn output_exists(&self, output: &Output) -> bool {
        self.output_state.contains_key(output)
    }

    /// Converts a `WlOutput` to a corresponding `Output` if it exists.
    ///
    /// Compared to raw `Output::from_resource`, this method also verifies that the output still
    /// exists in niri. Right after the output global is disabled, but before it is removed for
    /// good, `Output::from_resource` will succeed, but since niri already forgot the output,
    /// accessing it can cause logic bugs.
    pub fn output_from_resource(&self, wl_output: &WlOutput) -> Option<Output> {
        Output::from_resource(wl_output).filter(|output| self.output_exists(output))
    }

    pub fn remove_output(&mut self, output: &Output) {
        for layer in layer_map_for_output(output).layers() {
            layer.layer_surface().send_close();
        }

        self.layout.remove_output(output);
        self.global_space.unmap_output(output);
        self.reposition_outputs(None);
        self.gamma_control_manager_state.output_removed(output);

        let state = self.output_state.remove(output).unwrap();

        match state.redraw_state {
            RedrawState::Idle => (),
            RedrawState::Queued => (),
            RedrawState::WaitingForVBlank { .. } => (),
            RedrawState::WaitingForEstimatedVBlank(token) => self.event_loop.remove(token),
            RedrawState::WaitingForEstimatedVBlankAndQueued(token) => self.event_loop.remove(token),
        }

        self.stop_casts_for_target(CastTarget::output(output));
        self.screencopy_state.remove_output(output);

        // Disable the output global and remove some time later to give the clients some time to
        // process it.
        let global = state.global;
        self.display_handle.disable_global::<State>(global.clone());
        self.event_loop
            .insert_source(
                Timer::from_duration(Duration::from_secs(10)),
                move |_, _, state| {
                    state
                        .swayward
                        .display_handle
                        .remove_global::<State>(global.clone());
                    TimeoutAction::Drop
                },
            )
            .unwrap();

        match mem::take(&mut self.lock_state) {
            LockState::Locking(confirmation) => {
                // We're locking and an output was removed, check if the requirements are now met.
                let all_locked = self
                    .output_state
                    .values()
                    .all(|state| state.lock_render_state == LockRenderState::Locked);

                if all_locked {
                    let lock = confirmation.ext_session_lock().clone();
                    confirmation.lock();
                    self.lock_state = LockState::Locked(lock);
                } else {
                    // Still waiting.
                    self.lock_state = LockState::Locking(confirmation);
                }
            }
            lock_state => {
                self.lock_state = lock_state;
                self.maybe_continue_to_locking();
            }
        }

        if self.screenshot_ui.close() {
            self.cursor_manager
                .set_cursor_image(CursorImageStatus::default_named());
            self.queue_redraw_all();
        }

        if self.window_mru_ui.output() == Some(output) {
            self.cancel_mru();
        }
    }

    pub fn output_resized(&mut self, output: &Output) {
        let output_size = output_size(output);
        let scale = output.current_scale();
        let transform = output.current_transform();

        {
            let mut layer_map = layer_map_for_output(output);
            for layer in layer_map.layers() {
                layer.with_surfaces(|surface, data| {
                    send_scale_transform(surface, data, scale, transform);
                });

                if let Some(mapped) = self.mapped_layer_surfaces.get_mut(layer) {
                    mapped.update_sizes(output_size, scale.fractional_scale());
                }
            }
            layer_map.arrange();
        }

        self.layout.update_output_size(output);

        if let Some(state) = self.output_state.get_mut(output) {
            state.backdrop_buffer.resize(output_size);

            state.lock_color_buffer.resize(output_size);
            if let Some(lock_surface) = &state.lock_surface {
                configure_lock_surface(lock_surface, output);
            }
        }

        // If the output size changed with an open screenshot UI, close the screenshot UI.
        if let Some((old_size, old_scale, old_transform)) = self.screenshot_ui.output_size(output) {
            let output_mode = output.current_mode().unwrap();
            let size = transform.transform_size(output_mode.size);
            let scale = output.current_scale().fractional_scale();
            // FIXME: scale changes and transform flips shouldn't matter but they currently do since
            // I haven't quite figured out how to draw the screenshot textures in
            // physical coordinates.
            if old_size != size || old_scale != scale || old_transform != transform {
                self.screenshot_ui.close();
                self.cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
                self.queue_redraw_all();
                return;
            }
        }

        self.queue_redraw(output);
    }

    pub fn deactivate_monitors(&mut self, backend: &mut Backend) {
        if !self.monitors_active {
            return;
        }

        self.monitors_active = false;
        backend.set_monitors_active(false);
    }

    pub fn activate_monitors(&mut self, backend: &mut Backend) {
        if self.monitors_active {
            return;
        }

        self.monitors_active = true;
        backend.set_monitors_active(true);

        self.queue_redraw_all();
    }

    pub fn output_under(&self, pos: Point<f64, Logical>) -> Option<(&Output, Point<f64, Logical>)> {
        let output = self.global_space.output_under(pos).next()?;
        let pos_within_output = pos
            - self
                .global_space
                .output_geometry(output)
                .unwrap()
                .loc
                .to_f64();

        Some((output, pos_within_output))
    }

    pub(super) fn is_inside_hot_corner(&self, output: &Output, pos: Point<f64, Logical>) -> bool {
        let config = self.config.borrow();
        let hot_corners = output
            .user_data()
            .get::<OutputName>()
            .and_then(|name| config.outputs.find(name))
            .and_then(|c| c.hot_corners)
            .unwrap_or(config.gestures.hot_corners);

        if hot_corners.off {
            return false;
        }

        // Use size from the ceiled output geometry, since that's what we currently use for pointer
        // motion clamping.
        let geom = self.global_space.output_geometry(output).unwrap();
        let size = geom.size.to_f64();

        let contains = move |corner: Point<f64, Logical>| {
            Rectangle::new(corner, Size::new(1., 1.)).contains(pos)
        };

        if hot_corners.top_right && contains(Point::new(size.w - 1., 0.)) {
            return true;
        }
        if hot_corners.bottom_left && contains(Point::new(0., size.h - 1.)) {
            return true;
        }
        if hot_corners.bottom_right && contains(Point::new(size.w - 1., size.h - 1.)) {
            return true;
        }

        // If the user didn't explicitly set any corners, we default to top-left.
        if (hot_corners.top_left
            || !(hot_corners.top_right || hot_corners.bottom_right || hot_corners.bottom_left))
            && contains(Point::new(0., 0.))
        {
            return true;
        }

        false
    }

    pub fn is_sticky_obscured_under(
        &self,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> bool {
        // The ordering here must be consistent with the ordering in render() so that input is
        // consistent with the visuals.

        // Check if some layer-shell surface is on top.
        let layers = layer_map_for_output(output);
        let layer_surface_under = |layer, popup| {
            layers
                .layers_on(layer)
                .rev()
                .find_map(|layer| {
                    let mapped = self.mapped_layer_surfaces.get(layer)?;

                    let mut layer_pos_within_output =
                        layers.layer_geometry(layer).unwrap().loc.to_f64();
                    layer_pos_within_output += mapped.bob_offset();

                    let surface_type = if popup {
                        WindowSurfaceType::POPUP
                    } else {
                        WindowSurfaceType::TOPLEVEL
                    } | WindowSurfaceType::SUBSURFACE;
                    layer.surface_under(pos_within_output - layer_pos_within_output, surface_type)
                })
                .is_some()
        };

        let layer_toplevel_under = |layer| layer_surface_under(layer, false);
        let layer_popup_under = |layer| layer_surface_under(layer, true);

        if layer_popup_under(Layer::Overlay) || layer_toplevel_under(Layer::Overlay) {
            return true;
        }

        let mon = self.layout.monitor_for_output(output).unwrap();
        if mon.render_above_top_layer() {
            return false;
        }

        if self.is_inside_hot_corner(output, pos_within_output) {
            return true;
        }

        if layer_popup_under(Layer::Top) || layer_toplevel_under(Layer::Top) {
            return true;
        }

        false
    }

    pub fn is_layout_obscured_under(
        &self,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> bool {
        if self.layout.is_overview_open() {
            return false;
        }

        // Check if some layer-shell surface is on top.
        let layers = layer_map_for_output(output);
        let layer_popup_under = |layer| {
            layers
                .layers_on(layer)
                .rev()
                .find_map(|layer_surface| {
                    let mapped = self.mapped_layer_surfaces.get(layer_surface)?;
                    if mapped.place_within_backdrop() {
                        return None;
                    }

                    let mut layer_pos_within_output =
                        layers.layer_geometry(layer_surface).unwrap().loc.to_f64();
                    layer_pos_within_output += mapped.bob_offset();

                    // Background and bottom layers move together with the workspaces.
                    let mon = self.layout.monitor_for_output(output)?;
                    let (_, geo) = mon.workspace_under(pos_within_output)?;
                    layer_pos_within_output += geo.loc;

                    let surface_type = WindowSurfaceType::POPUP | WindowSurfaceType::SUBSURFACE;
                    layer_surface
                        .surface_under(pos_within_output - layer_pos_within_output, surface_type)
                })
                .is_some()
        };

        if layer_popup_under(Layer::Bottom) || layer_popup_under(Layer::Background) {
            return true;
        }

        false
    }

    /// Returns the workspace under the position to be activated.
    ///
    /// The return value is an output and a workspace index on it.
    pub fn workspace_under(
        &self,
        extended_bounds: bool,
        pos: Point<f64, Logical>,
    ) -> Option<(Output, &Workspace<Mapped>)> {
        if self.exit_confirm_dialog.is_open() || self.is_locked() || self.screenshot_ui.is_open() {
            return None;
        }

        let (output, pos_within_output) = self.output_under(pos)?;

        if self.is_sticky_obscured_under(output, pos_within_output) {
            return None;
        }

        if self.is_layout_obscured_under(output, pos_within_output) {
            return None;
        }

        let ws = self
            .layout
            .workspace_under(extended_bounds, output, pos_within_output)?;
        Some((output.clone(), ws))
    }

    pub fn workspace_under_cursor(
        &self,
        extended_bounds: bool,
    ) -> Option<(Output, &Workspace<Mapped>)> {
        let pos = self.seat.get_pointer().unwrap().current_location();
        self.workspace_under(extended_bounds, pos)
    }

    /// Returns the window under the position to be activated.
    ///
    /// The cursor may be inside the window's activation region, but not within the window's input
    /// region.
    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<&Mapped> {
        if self.exit_confirm_dialog.is_open()
            || self.is_locked()
            || self.screenshot_ui.is_open()
            || self.window_mru_ui.is_open()
        {
            return None;
        }

        let (output, pos_within_output) = self.output_under(pos)?;

        if self.is_sticky_obscured_under(output, pos_within_output) {
            return None;
        }

        if let Some((window, _loc)) = self
            .layout
            .interactive_moved_window_under(output, pos_within_output)
        {
            return Some(window);
        }

        if self.is_layout_obscured_under(output, pos_within_output) {
            return None;
        }

        let (window, _loc) = self.layout.window_under(output, pos_within_output)?;
        Some(window)
    }

    /// Returns the window under the cursor to be activated.
    ///
    /// The cursor may be inside the window's activation region, but not within the window's input
    /// region.
    pub fn window_under_cursor(&self) -> Option<&Mapped> {
        let pos = self.seat.get_pointer().unwrap().current_location();
        self.window_under(pos)
    }

    /// Returns contents under the given point.
    ///
    /// We don't have a proper global space for all windows, so this function converts window
    /// locations to global space according to where they are rendered.
    ///
    /// This function does not take pointer or touch grabs into account.
    pub fn contents_under(&self, pos: Point<f64, Logical>) -> PointContents {
        let mut rv = PointContents::default();

        let Some((output, pos_within_output)) = self.output_under(pos) else {
            return rv;
        };
        rv.output = Some(output.clone());
        let output_pos_in_global_space = self.global_space.output_geometry(output).unwrap().loc;

        // The ordering here must be consistent with the ordering in render() so that input is
        // consistent with the visuals.

        if self.exit_confirm_dialog.is_open() {
            return rv;
        } else if self.is_locked() {
            let Some(state) = self.output_state.get(output) else {
                return rv;
            };
            let Some(surface) = state.lock_surface.as_ref() else {
                return rv;
            };

            rv.surface = under_from_surface_tree(
                surface.wl_surface(),
                pos_within_output,
                // We put lock surfaces at (0, 0).
                (0, 0),
                WindowSurfaceType::ALL,
            )
            .map(|(surface, pos_within_output)| {
                (
                    surface,
                    (pos_within_output + output_pos_in_global_space).to_f64(),
                )
            });

            return rv;
        }

        if self.screenshot_ui.is_open() || self.window_mru_ui.is_open() {
            return rv;
        }

        let layers = layer_map_for_output(output);
        let layer_surface_under = |layer, popup| {
            layers
                .layers_on(layer)
                .rev()
                .find_map(|layer_surface| {
                    let mapped = self.mapped_layer_surfaces.get(layer_surface)?;
                    if mapped.place_within_backdrop() {
                        return None;
                    }

                    let mut layer_pos_within_output =
                        layers.layer_geometry(layer_surface).unwrap().loc.to_f64();
                    layer_pos_within_output += mapped.bob_offset();

                    // Background and bottom layers move together with the workspaces.
                    if matches!(layer, Layer::Background | Layer::Bottom) {
                        let mon = self.layout.monitor_for_output(output)?;
                        let (_, geo) = mon.workspace_under(pos_within_output)?;
                        layer_pos_within_output += geo.loc;
                        // Don't need to deal with zoom here because in the overview background and
                        // bottom layers don't receive input.
                    }

                    let surface_type = if popup {
                        WindowSurfaceType::POPUP
                    } else {
                        WindowSurfaceType::TOPLEVEL
                    } | WindowSurfaceType::SUBSURFACE;

                    layer_surface
                        .surface_under(pos_within_output - layer_pos_within_output, surface_type)
                        .map(|(surface, pos_within_layer)| {
                            (
                                (surface, pos_within_layer.to_f64() + layer_pos_within_output),
                                layer_surface,
                            )
                        })
                })
                .map(|(s, l)| (Some(s), (None, Some(l.clone()))))
        };

        let layer_toplevel_under = |layer| layer_surface_under(layer, false);
        let layer_popup_under = |layer| layer_surface_under(layer, true);

        let mapped_hit_data = |(mapped, hit): (&Mapped, HitType)| {
            let window = &mapped.window;
            let surface_and_pos = if let HitType::Input { win_pos } = hit {
                let win_pos_within_output = win_pos;
                window
                    .surface_under(
                        pos_within_output - win_pos_within_output,
                        WindowSurfaceType::ALL,
                    )
                    .map(|(s, pos_within_window)| {
                        (s, pos_within_window.to_f64() + win_pos_within_output)
                    })
            } else {
                None
            };
            (surface_and_pos, (Some((window.clone(), hit)), None))
        };

        let interactive_moved_window_under = || {
            self.layout
                .interactive_moved_window_under(output, pos_within_output)
                .map(mapped_hit_data)
        };
        let window_under = || {
            self.layout
                .window_under(output, pos_within_output)
                .map(mapped_hit_data)
        };

        let mon = self.layout.monitor_for_output(output).unwrap();

        let mut under =
            layer_popup_under(Layer::Overlay).or_else(|| layer_toplevel_under(Layer::Overlay));

        let is_overview_open = self.layout.is_overview_open();

        // When rendering above the top layer, we put the regular monitor elements first.
        // Otherwise, we will render all layer-shell pop-ups and the top layer on top.
        if mon.render_above_top_layer() {
            under = under
                .or_else(interactive_moved_window_under)
                .or_else(window_under)
                .or_else(|| layer_popup_under(Layer::Top))
                .or_else(|| layer_toplevel_under(Layer::Top))
                .or_else(|| layer_popup_under(Layer::Bottom))
                .or_else(|| layer_popup_under(Layer::Background))
                .or_else(|| layer_toplevel_under(Layer::Bottom))
                .or_else(|| layer_toplevel_under(Layer::Background));
        } else {
            if self.is_inside_hot_corner(output, pos_within_output) {
                rv.hot_corner = true;
                return rv;
            }

            under = under
                .or_else(|| layer_popup_under(Layer::Top))
                .or_else(|| layer_toplevel_under(Layer::Top));

            under = under.or_else(interactive_moved_window_under);

            if !is_overview_open {
                under = under
                    .or_else(|| layer_popup_under(Layer::Bottom))
                    .or_else(|| layer_popup_under(Layer::Background));
            }

            under = under.or_else(window_under);

            if !is_overview_open {
                under = under
                    .or_else(|| layer_toplevel_under(Layer::Bottom))
                    .or_else(|| layer_toplevel_under(Layer::Background));
            }
        }

        let Some((mut surface_and_pos, (window, layer))) = under else {
            return rv;
        };

        if let Some((_, surface_pos)) = &mut surface_and_pos {
            *surface_pos += output_pos_in_global_space.to_f64();
        }

        rv.surface = surface_and_pos;
        rv.window = window;
        rv.layer = layer;
        rv
    }

    pub fn output_under_cursor(&self) -> Option<Output> {
        let pos = self.seat.get_pointer().unwrap().current_location();
        self.global_space.output_under(pos).next().cloned()
    }

    pub fn output_left_of(&self, current: &Output) -> Option<Output> {
        self.output_left_of_point(current, center(self.global_space.output_geometry(current)?))
    }

    pub fn output_left_of_point(
        &self,
        current: &Output,
        reference: Point<i32, Logical>,
    ) -> Option<Output> {
        self.output_in_direction(current, reference, true, false, true)
    }

    pub fn output_right_of(&self, current: &Output) -> Option<Output> {
        self.output_right_of_point(current, center(self.global_space.output_geometry(current)?))
    }

    pub fn output_right_of_point(
        &self,
        current: &Output,
        reference: Point<i32, Logical>,
    ) -> Option<Output> {
        self.output_in_direction(current, reference, true, true, true)
    }

    pub fn output_up_of(&self, current: &Output) -> Option<Output> {
        self.output_up_of_point(current, center(self.global_space.output_geometry(current)?))
    }

    pub fn output_up_of_point(
        &self,
        current: &Output,
        reference: Point<i32, Logical>,
    ) -> Option<Output> {
        self.output_in_direction(current, reference, false, false, true)
    }

    pub fn output_down_of(&self, current: &Output) -> Option<Output> {
        self.output_down_of_point(current, center(self.global_space.output_geometry(current)?))
    }

    pub fn output_down_of_point(
        &self,
        current: &Output,
        reference: Point<i32, Logical>,
    ) -> Option<Output> {
        self.output_in_direction(current, reference, false, true, true)
    }

    pub(crate) fn adjacent_output(
        &self,
        current: &Output,
        reference: Point<i32, Logical>,
        horizontal: bool,
        positive: bool,
    ) -> Option<Output> {
        self.output_in_direction(current, reference, horizontal, positive, false)
    }

    pub(crate) fn output_in_direction_wrapping(
        &self,
        current: &Output,
        reference: Point<i32, Logical>,
        horizontal: bool,
        positive: bool,
    ) -> Option<Output> {
        self.output_in_direction(current, reference, horizontal, positive, true)
    }

    pub(super) fn output_in_direction(
        &self,
        current: &Output,
        reference: Point<i32, Logical>,
        horizontal: bool,
        positive: bool,
        wrap: bool,
    ) -> Option<Output> {
        let current_geo = self.global_space.output_geometry(current)?;
        let candidates = || {
            self.global_space
                .outputs()
                .filter(|output| *output != current)
                .filter_map(|output| {
                    self.global_space
                        .output_geometry(output)
                        .map(|geometry| (output, geometry))
                })
        };
        let in_direction = |geometry: Rectangle<i32, Logical>| {
            if horizontal {
                if positive {
                    geometry.loc.x >= current_geo.loc.x + current_geo.size.w
                } else {
                    geometry.loc.x + geometry.size.w <= current_geo.loc.x
                }
            } else if positive {
                geometry.loc.y >= current_geo.loc.y + current_geo.size.h
            } else {
                geometry.loc.y + geometry.size.h <= current_geo.loc.y
            }
        };
        let distance = |geometry: Rectangle<i32, Logical>| {
            let closest = Point::from((
                reference
                    .x
                    .clamp(geometry.loc.x, geometry.loc.x + geometry.size.w),
                reference
                    .y
                    .clamp(geometry.loc.y, geometry.loc.y + geometry.size.h),
            ));
            let delta = closest - reference;
            i64::from(delta.x) * i64::from(delta.x) + i64::from(delta.y) * i64::from(delta.y)
        };

        candidates()
            .filter(|(_, geometry)| in_direction(*geometry))
            .min_by_key(|(_, geometry)| distance(*geometry))
            .or_else(|| {
                wrap.then(|| {
                    candidates()
                        .filter(|(_, geometry)| {
                            let positive = !positive;
                            if horizontal {
                                if positive {
                                    geometry.loc.x >= current_geo.loc.x + current_geo.size.w
                                } else {
                                    geometry.loc.x + geometry.size.w <= current_geo.loc.x
                                }
                            } else if positive {
                                geometry.loc.y >= current_geo.loc.y + current_geo.size.h
                            } else {
                                geometry.loc.y + geometry.size.h <= current_geo.loc.y
                            }
                        })
                        .max_by_key(|(_, geometry)| distance(*geometry))
                })
                .flatten()
            })
            .map(|(output, _)| output.clone())
    }

    pub fn output_previous_of(&self, current: &Output) -> Option<Output> {
        self.sorted_outputs
            .iter()
            .rev()
            .skip_while(|&output| output != current)
            .nth(1)
            .or(self.sorted_outputs.last())
            .filter(|&output| output != current)
            .cloned()
    }

    pub fn output_next_of(&self, current: &Output) -> Option<Output> {
        self.sorted_outputs
            .iter()
            .skip_while(|&output| output != current)
            .nth(1)
            .or(self.sorted_outputs.first())
            .filter(|&output| output != current)
            .cloned()
    }

    pub fn output_left(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_left_of(active)
    }

    pub fn adjacent_output_left(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.adjacent_output(
            active,
            center(self.global_space.output_geometry(active)?),
            true,
            false,
        )
    }

    pub fn output_right(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_right_of(active)
    }

    pub fn adjacent_output_right(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.adjacent_output(
            active,
            center(self.global_space.output_geometry(active)?),
            true,
            true,
        )
    }

    pub fn output_up(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_up_of(active)
    }

    pub fn adjacent_output_up(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.adjacent_output(
            active,
            center(self.global_space.output_geometry(active)?),
            false,
            false,
        )
    }

    pub fn output_down(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_down_of(active)
    }

    pub fn adjacent_output_down(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.adjacent_output(
            active,
            center(self.global_space.output_geometry(active)?),
            false,
            true,
        )
    }

    pub fn output_previous(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_previous_of(active)
    }

    pub fn output_next(&self) -> Option<Output> {
        let active = self.layout.active_output()?;
        self.output_next_of(active)
    }

    pub fn find_output_and_workspace_index(
        &self,
        workspace_reference: WorkspaceReference,
    ) -> Option<(Option<Output>, usize)> {
        let (target_workspace_index, target_workspace) = match workspace_reference {
            WorkspaceReference::Index(index) => {
                return Some((None, index.saturating_sub(1) as usize));
            }
            WorkspaceReference::Name(name) => self.layout.find_workspace_by_name(&name)?,
            WorkspaceReference::Id(id) => {
                let id = WorkspaceId::specific(id);
                self.layout.find_workspace_by_id(id)?
            }
        };

        let target_output = target_workspace.current_output();
        Some((target_output.cloned(), target_workspace_index))
    }

    pub fn find_window_by_id(&self, id: MappedId) -> Option<Window> {
        self.layout
            .windows()
            .find(|(_, m)| m.id() == id)
            .map(|(_, m)| m.window.clone())
    }

    pub fn output_for_tablet(&self) -> Option<&Output> {
        let config = self.config.borrow();
        if config.input.tablet.map_to_focused_output {
            self.layout.active_output()
        } else {
            let map_to_output = config.input.tablet.map_to_output.as_ref();
            map_to_output.and_then(|name| self.output_by_name_match(name))
        }
    }

    pub fn output_for_touch(&self) -> Option<&Output> {
        let config = self.config.borrow();
        let map_to_output = config.input.touch.map_to_output.as_ref();
        map_to_output
            .and_then(|name| self.output_by_name_match(name))
            .or_else(|| self.global_space.outputs().next())
    }

    pub fn output_by_name_match(&self, target: &str) -> Option<&Output> {
        self.global_space
            .outputs()
            .find(|output| output_matches_name(output, target))
    }

    pub fn output_for_root(&self, root: &WlSurface) -> Option<&Output> {
        // Check the main layout.
        let win_out = self.layout.find_window_and_output(root);
        let layout_output = win_out.map(|(_, output)| output);
        if let Some(output) = layout_output {
            return output;
        }

        // Check layer-shell.
        let has_layer_surface = |o: &&Output| {
            layer_map_for_output(o)
                .layer_for_surface(root, WindowSurfaceType::TOPLEVEL)
                .is_some()
        };
        self.layout.outputs().find(has_layer_surface)
    }

    pub fn lock_surface_focus(&self) -> Option<WlSurface> {
        let output_under_cursor = self.output_under_cursor();
        let output = output_under_cursor
            .as_ref()
            .or_else(|| self.layout.active_output())
            .or_else(|| self.global_space.outputs().next())?;

        let state = self.output_state.get(output)?;
        state.lock_surface.as_ref().map(|s| s.wl_surface()).cloned()
    }
}

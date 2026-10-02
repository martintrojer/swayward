use super::*;

impl Swayward {
    /// Schedules an immediate redraw on all outputs if one is not already scheduled.
    pub fn queue_redraw_all(&mut self) {
        for state in self.output_state.values_mut() {
            state.redraw_state = mem::take(&mut state.redraw_state).queue_redraw();
        }
    }

    /// Schedules an immediate redraw if one is not already scheduled.
    pub fn queue_redraw(&mut self, output: &Output) {
        let state = self.output_state.get_mut(output).unwrap();
        state.redraw_state = mem::take(&mut state.redraw_state).queue_redraw();
    }

    pub fn redraw_queued_outputs(&mut self, backend: &mut Backend) {
        let _span = tracy_client::span!("Swayward::redraw_queued_outputs");

        while let Some((output, _)) = self.output_state.iter().find(|(_, state)| {
            matches!(
                state.redraw_state,
                RedrawState::Queued | RedrawState::WaitingForEstimatedVBlankAndQueued(_)
            )
        }) {
            trace!("redrawing output");
            let output = output.clone();
            self.redraw(backend, &output);
        }
    }

    pub fn render_pointer<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        output: &Output,
        push: &mut dyn FnMut(PointerRenderElements<R>),
    ) {
        let _span = tracy_client::span!("Swayward::render_pointer");
        let output_scale = output.current_scale();
        let output_pos = self.global_space.output_geometry(output).unwrap().loc;

        // Check whether we need to draw the tablet cursor or the regular cursor.
        let pointer_pos = self
            .tablet_cursor_location
            .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());
        let pointer_pos = pointer_pos - output_pos.to_f64();

        // Get the render cursor to draw.
        let cursor_scale = output_scale.integer_scale();
        let render_cursor = self.cursor_manager.get_render_cursor(cursor_scale);

        let output_scale = Scale::from(output.current_scale().fractional_scale());

        match render_cursor {
            RenderCursor::Hidden => (),
            RenderCursor::Surface { surface, hotspot } => {
                let pointer_pos =
                    (pointer_pos - hotspot.to_f64()).to_physical_precise_round(output_scale);

                push_elements_from_surface_tree(
                    renderer,
                    &surface,
                    pointer_pos,
                    output_scale,
                    1.,
                    Kind::Cursor,
                    &mut |elem| push(elem.into()),
                );
            }
            RenderCursor::Named {
                icon,
                scale,
                cursor,
            } => {
                let (idx, frame) = cursor.frame(self.start_time.elapsed().as_millis() as u32);
                let hotspot = XCursor::hotspot(frame).to_logical(scale);
                let pointer_pos =
                    (pointer_pos - hotspot.to_f64()).to_physical_precise_round(output_scale);

                let texture = self.cursor_texture_cache.get(icon, scale, &cursor, idx);
                match MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    pointer_pos,
                    &texture,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    Ok(element) => push(element.into()),
                    Err(err) => {
                        warn!("error importing a cursor texture: {err:?}");
                    }
                }
            }
        }

        if let Some(dnd_icon) = self.dnd_icon.as_ref() {
            let pointer_pos =
                (pointer_pos + dnd_icon.offset.to_f64()).to_physical_precise_round(output_scale);
            push_elements_from_surface_tree(
                renderer,
                &dnd_icon.surface,
                pointer_pos,
                output_scale,
                1.,
                Kind::ScanoutCandidate,
                &mut |elem| push(elem.into()),
            );
        }
    }

    /// Checks if the pointer should be included on a window cast or screenshot.
    ///
    /// Returns `(cursor_global_pos, win_pos)` if the pointer should be included, or `None`
    /// otherwise.
    pub fn pointer_pos_for_window_cast(
        &self,
        mapped: &Mapped,
    ) -> Option<(Point<f64, Logical>, Point<f64, Logical>)> {
        // Tablet cursor.
        if let Some(tablet_pos) = self.tablet_cursor_location {
            let contents = self.contents_under(tablet_pos);
            if let Some((w, HitType::Input { win_pos })) = contents.window {
                if w == mapped.window {
                    // Tablet tools don't currently expose current focus, and don't currently
                    // have grabs. When those are implemented, this branch should be adjusted
                    // to look more similar to the branch below.
                    return Some((tablet_pos, win_pos));
                }
            }
        }
        // Regular cursor.
        else if let Some((w, HitType::Input { win_pos })) = &self.pointer_contents.window {
            if w == &mapped.window {
                // Grabs can modify the pointer focus, making it different from
                // pointer_contents. Notably, gestures like Mod+MMB will remove the pointer
                // focus, and ClickGrab will keep pointer focus on the clicked window even
                // while it's moving over a different window.
                //
                // So, double-check that current_focus() (after grabs) also matches the pointer
                // contents.
                let pointer = self.seat.get_pointer().unwrap();

                // The DnD grab is a bit special because it has its own focus (data device)
                // while the pointer focus is cleared. That focus is not currently exposed from
                // Smithay, and showing DnD icons on window screenshots seems useful, so let's
                // just allow it during DnD grabs.
                let is_dnd_grab = pointer
                    .with_grab(|_, grab| State::is_dnd_grab(grab.as_any()))
                    .unwrap_or(false);

                let current_focus_matches = is_dnd_grab
                    || pointer
                        .current_focus()
                        .map(|focused| self.find_root_shell_surface(&focused))
                        .is_some_and(|focused| mapped.is_wl_surface(&focused));
                if current_focus_matches {
                    // We don't check for pointer visibility because it can only be Visible or
                    // Hidden, and never Disabled (then it wouldn't have focus). Even when the
                    // pointer is Hidden, we want to render it, since the user explicitly
                    // requested show_pointer = true, and otherwise there's no easy way to
                    // screenshot a window with pointer with hide-when-typing because pressing
                    // the screenshot bind will hide the pointer.
                    return Some((pointer.current_location(), *win_pos));
                }
            }
        }

        None
    }

    pub fn refresh_pointer_outputs(&mut self) {
        if !self.pointer_visibility.is_visible() {
            return;
        }

        let _span = tracy_client::span!("Swayward::refresh_pointer_outputs");

        // Check whether we need to draw the tablet cursor or the regular cursor.
        let pointer_pos = self
            .tablet_cursor_location
            .unwrap_or_else(|| self.seat.get_pointer().unwrap().current_location());

        match self.cursor_manager.cursor_image() {
            CursorImageStatus::Surface(ref surface) => {
                let hotspot = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<CursorImageSurfaceData>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .hotspot
                });

                let surface_pos = pointer_pos.to_i32_round() - hotspot;
                let bbox = bbox_from_surface_tree(surface, surface_pos);

                let dnd = self
                    .dnd_icon
                    .as_ref()
                    .map(|icon| &icon.surface)
                    .map(|surface| (surface, bbox_from_surface_tree(surface, surface_pos)));

                // FIXME we basically need to pick the largest scale factor across the overlapping
                // outputs, this is how it's usually done in clients as well.
                let mut cursor_scale = 1.;
                let mut cursor_transform = Transform::Normal;
                let mut dnd_scale = 1.;
                let mut dnd_transform = Transform::Normal;
                for output in self.global_space.outputs() {
                    let geo = self.global_space.output_geometry(output).unwrap();

                    // Compute pointer surface overlap.
                    if let Some(mut overlap) = geo.intersection(bbox) {
                        overlap.loc -= surface_pos;
                        cursor_scale =
                            f64::max(cursor_scale, output.current_scale().fractional_scale());
                        // FIXME: using the largest overlapping or "primary" output transform would
                        // make more sense here.
                        cursor_transform = output.current_transform();
                        output_update(output, Some(overlap), surface);
                    } else {
                        output_update(output, None, surface);
                    }

                    // Compute DnD icon surface overlap.
                    if let Some((surface, bbox)) = dnd {
                        if let Some(mut overlap) = geo.intersection(bbox) {
                            overlap.loc -= surface_pos;
                            dnd_scale =
                                f64::max(dnd_scale, output.current_scale().fractional_scale());
                            // FIXME: using the largest overlapping or "primary" output transform
                            // would make more sense here.
                            dnd_transform = output.current_transform();
                            output_update(output, Some(overlap), surface);
                        } else {
                            output_update(output, None, surface);
                        }
                    }
                }

                with_states(surface, |data| {
                    send_scale_transform(
                        surface,
                        data,
                        output::Scale::Fractional(cursor_scale),
                        cursor_transform,
                    )
                });
                if let Some((surface, _)) = dnd {
                    with_states(surface, |data| {
                        send_scale_transform(
                            surface,
                            data,
                            output::Scale::Fractional(dnd_scale),
                            dnd_transform,
                        );
                    });
                }
            }
            cursor_image => {
                // There's no cursor surface, but there might be a DnD icon.
                let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) else {
                    return;
                };

                let icon = if let CursorImageStatus::Named(icon) = cursor_image {
                    *icon
                } else {
                    Default::default()
                };

                let mut dnd_scale = 1.;
                let mut dnd_transform = Transform::Normal;
                for output in self.global_space.outputs() {
                    let geo = self.global_space.output_geometry(output).unwrap();

                    // The default cursor is rendered at the right scale for each output, which
                    // means that it may have a different hotspot for each output.
                    let output_scale = output.current_scale().integer_scale();
                    let cursor = self
                        .cursor_manager
                        .get_cursor_with_name(icon, output_scale)
                        .unwrap_or_else(|| self.cursor_manager.get_default_cursor(output_scale));

                    // For simplicity, we always use frame 0 for this computation. Let's hope the
                    // hotspot doesn't change between frames.
                    let hotspot = XCursor::hotspot(&cursor.frames()[0]).to_logical(output_scale);

                    let surface_pos = pointer_pos.to_i32_round() - hotspot;
                    let bbox = bbox_from_surface_tree(surface, surface_pos);

                    if let Some(mut overlap) = geo.intersection(bbox) {
                        overlap.loc -= surface_pos;
                        dnd_scale = f64::max(dnd_scale, output.current_scale().fractional_scale());
                        // FIXME: using the largest overlapping or "primary" output transform would
                        // make more sense here.
                        dnd_transform = output.current_transform();
                        output_update(output, Some(overlap), surface);
                    } else {
                        output_update(output, None, surface);
                    }
                }

                with_states(surface, |data| {
                    send_scale_transform(
                        surface,
                        data,
                        output::Scale::Fractional(dnd_scale),
                        dnd_transform,
                    );
                });
            }
        }
    }

    pub fn refresh_layout(&mut self) {
        let layout_is_active = match &self.keyboard_focus {
            KeyboardFocus::Layout { .. } => true,
            KeyboardFocus::LayerShell { .. } => false,

            // Draw layout as active in these cases to reduce unnecessary window animations.
            // There's no confusion because these are both fullscreen modes.
            //
            // FIXME: when going into the screenshot UI from a layer-shell focus, and then back to
            // layer-shell, the layout will briefly draw as active, despite never having focus.
            KeyboardFocus::LockScreen { .. } => true,
            KeyboardFocus::ScreenshotUi => true,
            KeyboardFocus::ExitConfirmDialog => true,
            KeyboardFocus::Overview => true,
            KeyboardFocus::Mru => true,
        };

        self.layout.refresh(layout_is_active);
    }

    pub fn refresh_idle_inhibit(&mut self) {
        let _span = tracy_client::span!("Swayward::refresh_idle_inhibit");

        self.idle_inhibiting_surfaces.retain(|s| s.is_alive());

        let user_policies = self
            .layout
            .windows()
            .map(|(_, mapped)| {
                (
                    mapped.window.clone(),
                    mapped.inhibit_idle_mode(),
                    mapped.is_focused(),
                    mapped.pending_sizing_mode().is_fullscreen(),
                )
            })
            .collect::<Vec<_>>();
        let user_inhibited =
            user_policies
                .into_iter()
                .any(|(window, mode, focused, fullscreen)| {
                    use swayward_ipc::command::InhibitIdleMode;
                    match mode {
                        InhibitIdleMode::None => false,
                        InhibitIdleMode::Open => true,
                        InhibitIdleMode::Focus => focused,
                        InhibitIdleMode::Fullscreen => fullscreen,
                        InhibitIdleMode::Visible => {
                            self.layout.window_is_on_visible_workspace(&window)
                        }
                    }
                });
        let is_inhibited = self.is_fdo_idle_inhibited.load(Ordering::SeqCst)
            || user_inhibited
            || self.idle_inhibiting_surfaces.iter().any(|surface| {
                with_states(surface, |states| {
                    surface_primary_scanout_output(surface, states).is_some()
                })
            });
        self.idle_notifier_state.set_is_inhibited(is_inhibited);
    }

    pub fn refresh_window_states(&mut self) {
        let _span = tracy_client::span!("Swayward::refresh_window_states");

        let config = self.config.borrow();
        self.layout.with_windows_mut(|mapped, _output| {
            mapped.update_tiled_state(config.prefer_no_csd);
        });
        drop(config);
    }

    pub fn refresh_window_rules(&mut self) {
        let _span = tracy_client::span!("Swayward::refresh_window_rules");

        let config = self.config.borrow();
        let window_rules = &config.window_rules;

        let mut windows = vec![];
        let mut outputs = HashSet::new();
        self.layout.with_windows_mut(|mapped, output| {
            if mapped.recompute_window_rules_if_needed(window_rules, self.is_at_startup) {
                windows.push(mapped.window.clone());

                if let Some(output) = output {
                    outputs.insert(output.clone());
                }

                // Since refresh_window_rules() is called after refresh_layout(), we need to update
                // the tiled state right here, so that it's picked up by the following
                // send_pending_configure().
                mapped.update_tiled_state(config.prefer_no_csd);
            }
        });
        drop(config);

        for win in windows {
            self.layout.update_window(&win, None);
            win.toplevel()
                .expect("no X11 support")
                .send_pending_configure();
        }
        for output in outputs {
            self.queue_redraw(&output);
        }
    }

    pub fn advance_animations(&mut self) {
        let _span = tracy_client::span!("Swayward::advance_animations");

        self.layout.advance_animations();
        self.config_error_notification.advance_animations();
        self.exit_confirm_dialog.advance_animations();
        self.screenshot_ui.advance_animations();
        self.window_mru_ui.advance_animations();

        for state in self.output_state.values_mut() {
            if let Some(transition) = &mut state.screen_transition {
                if transition.is_done() {
                    state.screen_transition = None;
                }
            }
        }
    }

    pub fn update_render_elements(&mut self, output: Option<&Output>) {
        self.update_xray_render_elements(output);
        self.layout.update_render_elements(output);

        for (out, state) in self.output_state.iter_mut() {
            if output.is_none_or(|output| out == output) {
                let scale = Scale::from(out.current_scale().fractional_scale());
                let transform = out.current_transform();

                if let Some(transition) = &mut state.screen_transition {
                    transition.update_render_elements(scale, transform);
                }

                let layer_map = layer_map_for_output(out);
                for surface in layer_map.layers() {
                    let Some(mapped) = self.mapped_layer_surfaces.get_mut(surface) else {
                        continue;
                    };
                    let Some(geo) = layer_map.layer_geometry(surface) else {
                        continue;
                    };

                    mapped.update_render_elements(geo.size.to_f64());
                }
            }
        }
    }

    // Updates only those render elements that go in the xray buffer.
    pub fn update_xray_render_elements(&mut self, output: Option<&Output>) {
        for (out, state) in self.output_state.iter_mut() {
            if output.is_none_or(|output| out == output) {
                let scale = Scale::from(out.current_scale().fractional_scale());
                let mode = out.current_mode().unwrap();
                let transform = out.current_transform();
                let size = transform.transform_size(mode.size);

                state.xray.workspaces.clear();
                let mon = self.layout.monitor_for_output(out).unwrap();
                for (ws, geo) in mon.workspaces_with_render_geo() {
                    let bg_color = ws.render_background().color();
                    state.xray.workspaces.push((geo, bg_color));
                }
                state.xray.backdrop_color = state.backdrop_buffer.color();
                let blur_options = BlurOptions::from(self.config.borrow().blur);
                for buf in &state.xray.background {
                    let mut buffer = buf.borrow_mut();
                    buffer.update_size(size, scale);
                    buffer.update_blur_options(blur_options);
                }
                for buf in &state.xray.backdrop {
                    let mut buffer = buf.borrow_mut();
                    buffer.update_size(size, scale);
                    buffer.update_blur_options(blur_options);
                }

                let layer_map = layer_map_for_output(out);
                for surface in layer_map.layers_on(Layer::Background) {
                    let Some(mapped) = self.mapped_layer_surfaces.get_mut(surface) else {
                        continue;
                    };
                    let Some(geo) = layer_map.layer_geometry(surface) else {
                        continue;
                    };

                    mapped.update_render_elements(geo.size.to_f64());
                }
            }
        }
    }

    pub fn update_shaders(&mut self) {
        self.layout.update_shaders();

        for mapped in self.mapped_layer_surfaces.values_mut() {
            mapped.update_shaders();
        }
    }

    pub fn render_to_vec<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        output: &Output,
        include_pointer: bool,
    ) -> Vec<OutputRenderElements<R>> {
        let mut elements = Vec::new();
        self.render(ctx, output, include_pointer, &mut |elem| {
            elements.push(elem)
        });
        elements
    }

    pub fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        output: &Output,
        include_pointer: bool,
        push: &mut dyn FnMut(OutputRenderElements<R>),
    ) {
        let _span = tracy_client::span!("Swayward::render");

        if ctx.target == RenderTarget::Output {
            if let Some(preview) = self.config.borrow().debug.preview_render {
                ctx.target = match preview {
                    PreviewRender::Screencast => RenderTarget::Screencast,
                    PreviewRender::ScreenCapture => RenderTarget::ScreenCapture,
                };
            }
        }

        self.fill_xray_elements(ctx.as_gles(), output);

        // Reborrow to shorten lifetime to be able to put in xray.
        let mut ctx = ctx.r();
        let state = self.output_state.get(output).unwrap();
        ctx.xray = Some(&state.xray);

        self.render_inner(ctx, output, include_pointer, push);

        self.clear_xray_elements(output);
    }

    pub(super) fn render_inner<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        output: &Output,
        include_pointer: bool,
        push: &mut dyn FnMut(OutputRenderElements<R>),
    ) {
        let state = self.output_state.get(output).unwrap();
        let output_scale = Scale::from(output.current_scale().fractional_scale());

        let push = if self.debug_draw_opaque_regions {
            &mut move |elem| {
                push_opaque_regions(&elem, output_scale, push);
                push(elem);
            }
        } else {
            push
        };

        // The pointer goes on the top.
        if include_pointer && self.pointer_visibility.is_visible() {
            self.render_pointer(ctx.renderer, output, &mut |elem| push(elem.into()));
        }

        // Next, the screen transition texture.
        {
            if let Some(transition) = &state.screen_transition {
                push(transition.render(ctx.target).into());
            }
        }

        // Next, the exit confirm dialog.
        self.exit_confirm_dialog
            .render(ctx.renderer, output, &mut |elem| push(elem.into()));

        // Next, the config error notification too.
        if let Some(element) = self.config_error_notification.render(ctx.renderer, output) {
            push(element.into());
        }

        // If the session is locked, draw the lock surface.
        if self.is_locked() {
            if let Some(surface) = state.lock_surface.as_ref() {
                push_elements_from_surface_tree(
                    ctx.renderer,
                    surface.wl_surface(),
                    Point::new(0, 0),
                    output_scale,
                    1.,
                    Kind::ScanoutCandidate,
                    &mut |elem| push(elem.into()),
                );
            }

            // Draw the solid color background.
            push(
                SolidColorRenderElement::from_buffer(
                    &state.lock_color_buffer,
                    (0., 0.),
                    1.,
                    Kind::Unspecified,
                )
                .into(),
            );

            return;
        }

        // Prepare the background elements.
        let backdrop = SolidColorRenderElement::from_buffer(
            &state.backdrop_buffer,
            (0., 0.),
            1.,
            Kind::Unspecified,
        )
        .into();

        // If the screenshot UI is open, draw it.
        if self.screenshot_ui.is_open() {
            self.screenshot_ui
                .render_output(output, ctx.target, &mut |elem| push(elem.into()));

            // Add the backdrop for outputs that were connected while the screenshot UI was open.
            push(backdrop);

            return;
        }

        // Draw the hotkey overlay on top.
        if let Some(element) = self.hotkey_overlay.render(ctx.renderer, output) {
            push(element.into());
        }

        // Then, the Alt-Tab switcher.
        self.window_mru_ui
            .render_output(self, output, ctx.r(), &mut |elem| push(elem.into()));

        // Don't draw the focus ring on the workspaces while interactively moving above those
        // workspaces, since the interactively-moved window already has a focus ring.
        let focus_ring = !self.layout.interactive_move_is_moving_above_output(output);

        // Get monitor elements.
        let mon = self.layout.monitor_for_output(output).unwrap();
        let zoom = mon.overview_zoom();

        // Get layer-shell elements.
        let layer_map = layer_map_for_output(output);

        // We use macros instead of closures to avoid borrowing issues (renderer and push() go
        // into different functions).
        macro_rules! push_popups_from_layer {
            ($layer:expr, $ns:expr, $xray_pos:expr, $backdrop:expr, $push:expr) => {{
                self.render_layer_popups(
                    ctx.r(),
                    LayerRenderRequest {
                        ns: $ns,
                        layer_map: &layer_map,
                        layer: $layer,
                        xray_pos: $xray_pos,
                        for_backdrop: $backdrop,
                    },
                    $push,
                );
            }};
            ($layer:expr, true) => {{
                push_popups_from_layer!($layer, None, XrayPos::default(), true, &mut |elem| push(
                    elem.into()
                ));
            }};
            ($layer:expr, $ns:expr, $xray_pos:expr, $push:expr) => {{
                push_popups_from_layer!($layer, $ns, $xray_pos, false, $push);
            }};
            ($layer:expr) => {{
                push_popups_from_layer!($layer, None, XrayPos::default(), false, &mut |elem| push(
                    elem.into()
                ));
            }};
        }
        macro_rules! push_normal_from_layer {
            ($layer:expr, $ns:expr, $xray_pos:expr, $backdrop:expr, $push:expr) => {{
                self.render_layer_normal(
                    ctx.r(),
                    LayerRenderRequest {
                        ns: $ns,
                        layer_map: &layer_map,
                        layer: $layer,
                        xray_pos: $xray_pos,
                        for_backdrop: $backdrop,
                    },
                    $push,
                );
            }};
            ($layer:expr, true) => {{
                push_normal_from_layer!($layer, None, XrayPos::default(), true, &mut |elem| {
                    push(elem.into())
                });
            }};
            ($layer:expr, $ns:expr, $xray_pos:expr, $push:expr) => {{
                push_normal_from_layer!($layer, $ns, $xray_pos, false, $push);
            }};
            ($layer:expr) => {{
                push_normal_from_layer!($layer, None, XrayPos::default(), false, &mut |elem| {
                    push(elem.into())
                });
            }};
        }

        // The overlay layer elements go next.
        push_popups_from_layer!(Layer::Overlay);
        push_normal_from_layer!(Layer::Overlay);

        // When rendering above the top layer, we put the regular monitor elements first.
        // Otherwise, we will render all layer-shell pop-ups and the top layer on top.
        if mon.render_above_top_layer() {
            self.layout
                .render_interactive_move_for_output(ctx.r(), output, &mut |elem| push(elem.into()));

            mon.render_insert_hint_between_workspaces(ctx.renderer, &mut |elem| push(elem.into()));

            mon.render_workspaces(ctx.r(), focus_ring, &mut |elem| push(elem.into()));

            push_popups_from_layer!(Layer::Top);
            push_normal_from_layer!(Layer::Top);

            push_popups_from_layer!(Layer::Bottom);
            push_popups_from_layer!(Layer::Background);
            push_normal_from_layer!(Layer::Bottom);
            push_normal_from_layer!(Layer::Background);

            // We don't expect more than one workspace when render_above_top_layer().
            if let Some((ws, _geo)) = mon.workspaces_with_render_geo().next() {
                push(ws.render_background().into());
            }
        } else {
            push_popups_from_layer!(Layer::Top);
            push_normal_from_layer!(Layer::Top);

            self.layout
                .render_interactive_move_for_output(ctx.r(), output, &mut |elem| push(elem.into()));

            mon.render_insert_hint_between_workspaces(ctx.renderer, &mut |elem| push(elem.into()));

            // Macro instead of closure to avoid borrowing push().
            macro_rules! process {
                ($geo:expr) => {{
                    &mut |elem| {
                        if let Some(elem) = scale_relocate_crop(elem, output_scale, zoom, $geo) {
                            push(elem.into());
                        }
                    }
                }};
            }

            for (ws, geo) in mon.workspaces_with_render_geo() {
                let ns = Some(ws.id().get() as usize);
                let xray_pos = XrayPos::new(geo.loc, zoom);
                push_popups_from_layer!(Layer::Bottom, ns, xray_pos, process!(geo));
                push_popups_from_layer!(Layer::Background, ns, xray_pos, process!(geo));
            }

            mon.render_workspaces(ctx.r(), focus_ring, &mut |elem| push(elem.into()));

            for (ws, geo) in mon.workspaces_with_render_geo() {
                // The render element namespace. This will be set to the workspace index for
                // elements duplicated across workspaces (i.e. background and bottom layers) in
                // order to have their non-xray framebuffer effects separated from each other.
                //
                // This doesn't have to correspond exactly to workspace id or idx, the only
                // requirement is that there's only one framebuffer effect element with a given id +
                // namespace on the frame at once. Id + namespace is used as the cache key in the
                // damage tracker.
                let ns = Some(ws.id().get() as usize);
                let xray_pos = XrayPos::new(geo.loc, zoom);
                push_normal_from_layer!(Layer::Bottom, ns, xray_pos, process!(geo));
                push_normal_from_layer!(Layer::Background, ns, xray_pos, process!(geo));

                process!(geo)(ws.render_background());
            }
        }

        mon.render_workspace_shadows(ctx.renderer, &mut |elem| push(elem.into()));

        // Then the backdrop.
        push_popups_from_layer!(Layer::Background, true);
        push_normal_from_layer!(Layer::Background, true);

        push(backdrop);
    }

    pub fn fill_xray_elements(&self, mut ctx: RenderCtx<GlesRenderer>, output: &Output) {
        let _span = tracy_client::span!("Swayward::fill_xray_elements");

        // Make sure the xrayed elements themselves cannot use xray by mistake.
        ctx.xray = None;

        let state = self.output_state.get(output).unwrap();
        let xray = &state.xray;
        let layer_map = layer_map_for_output(output);

        // FIXME: it would be cool to call this code on-demand. It's even relatively simple to do:
        // move this function to after the render_inner() call, check if
        // Rc::strong_count(&xray.background) > 1, and only then construct the elements. This way,
        // only if something referenced the xray buffer will the elements get constructed.
        //
        // Unfortunately, currently this runs into an important limitation: offscreens are rendered
        // immediately deep inside render_inner(), and when they are, they already need the xray
        // elements filled.
        //
        // Perhaps in the future when offscreen rendering becomes on-demand, this optimization will
        // be possible.

        let mut buffer = xray.background[ctx.target as usize].borrow_mut();
        {
            let elements = buffer.elements();
            elements.clear();
            self.render_layer_normal(
                ctx.r(),
                LayerRenderRequest {
                    ns: None,
                    layer_map: &layer_map,
                    layer: Layer::Background,
                    xray_pos: XrayPos::default(),
                    for_backdrop: false,
                },
                &mut |elem| elements.push(elem.into()),
            );
            // Avoid unused capacity remaining forever.
            elements.shrink_to_fit();
        }

        let mut buffer = xray.backdrop[ctx.target as usize].borrow_mut();
        {
            let elements = buffer.elements();
            elements.clear();
            self.render_layer_normal(
                ctx.r(),
                LayerRenderRequest {
                    ns: None,
                    layer_map: &layer_map,
                    layer: Layer::Background,
                    xray_pos: XrayPos::default(),
                    for_backdrop: true,
                },
                &mut |elem| elements.push(elem.into()),
            );
            // Avoid unused capacity remaining forever.
            elements.shrink_to_fit();
        }
    }

    pub fn clear_xray_elements(&self, output: &Output) {
        let state = self.output_state.get(output).unwrap();
        let xray = &state.xray;

        // Clear the xray elements for all render targets after all rendering that could use them
        // did so.
        for buf in &xray.background {
            buf.borrow_mut().elements().clear();
        }
        for buf in &xray.backdrop {
            buf.borrow_mut().elements().clear();
        }
    }

    /// Checks if any background layer surface has `block_out_from` set.
    pub fn has_blocked_out_background_layers(&self, output: &Output) -> bool {
        let layer_map = layer_map_for_output(output);
        for for_backdrop in [false, true] {
            for (mapped, _geo) in
                self.layers_in_render_order(&layer_map, Layer::Background, for_backdrop)
            {
                if mapped.rules().block_out_from.is_some() {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn layers_in_render_order<'a>(
        &'a self,
        layer_map: &'a LayerMap,
        layer: Layer,
        for_backdrop: bool,
    ) -> impl Iterator<Item = (&'a MappedLayer, Rectangle<i32, Logical>)> {
        // LayerMap returns layers in reverse stacking order.
        layer_map.layers_on(layer).rev().filter_map(move |surface| {
            let mapped = self.mapped_layer_surfaces.get(surface)?;

            if for_backdrop != mapped.place_within_backdrop() {
                return None;
            }

            let geo = layer_map.layer_geometry(surface)?;
            Some((mapped, geo))
        })
    }

    pub(super) fn render_layer_normal<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        request: LayerRenderRequest<'_>,
        push: &mut dyn FnMut(LayerSurfaceRenderElement<R>),
    ) {
        let LayerRenderRequest {
            ns,
            layer_map,
            layer,
            xray_pos,
            for_backdrop,
        } = request;
        for (mapped, geo) in self.layers_in_render_order(layer_map, layer, for_backdrop) {
            let loc = geo.loc.to_f64();
            let xray_pos = xray_pos.offset(loc);
            mapped.render_normal(ctx.r(), ns, loc, xray_pos, push);
        }
    }

    pub(super) fn render_layer_popups<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        request: LayerRenderRequest<'_>,
        push: &mut dyn FnMut(LayerSurfaceRenderElement<R>),
    ) {
        let LayerRenderRequest {
            ns,
            layer_map,
            layer,
            xray_pos,
            for_backdrop,
        } = request;
        for (mapped, geo) in self.layers_in_render_order(layer_map, layer, for_backdrop) {
            let loc = geo.loc.to_f64();
            let xray_pos = xray_pos.offset(loc);
            mapped.render_popups(ctx.r(), ns, loc, xray_pos, push);
        }
    }

    pub(super) fn redraw(&mut self, backend: &mut Backend, output: &Output) {
        let _span = tracy_client::span!("Swayward::redraw");

        let powered = self
            .output_power
            .get(&output.name())
            .copied()
            .unwrap_or(true);
        let active = self.monitors_active && powered;

        // Verify our invariant.
        let state = self.output_state.get_mut(output).unwrap();
        assert!(matches!(
            state.redraw_state,
            RedrawState::Queued | RedrawState::WaitingForEstimatedVBlankAndQueued(_)
        ));

        let target_presentation_time = state.frame_clock.next_presentation_time();

        // Freeze the clock at the target time.
        self.clock.set_unadjusted(target_presentation_time);

        self.update_render_elements(Some(output));

        let mut res = RenderResult::Skipped;
        if active {
            let state = self.output_state.get_mut(output).unwrap();
            state.unfinished_animations_remain = self.layout.are_animations_ongoing(Some(output));
            state.unfinished_animations_remain |=
                self.config_error_notification.are_animations_ongoing();
            state.unfinished_animations_remain |= self.exit_confirm_dialog.are_animations_ongoing();
            state.unfinished_animations_remain |= self.screenshot_ui.are_animations_ongoing();
            state.unfinished_animations_remain |= self.window_mru_ui.are_animations_ongoing();
            state.unfinished_animations_remain |= state.screen_transition.is_some();

            // Also keep redrawing if the current cursor is animated.
            state.unfinished_animations_remain |= self
                .cursor_manager
                .is_current_cursor_animated(output.current_scale().integer_scale());

            // Also check layer surfaces.
            if !state.unfinished_animations_remain {
                state.unfinished_animations_remain |= layer_map_for_output(output)
                    .layers()
                    .filter_map(|surface| self.mapped_layer_surfaces.get(surface))
                    .any(|mapped| mapped.are_animations_ongoing());
            }

            // Render.
            res = backend.render(self, output, target_presentation_time);
        }

        let is_locked = self.is_locked();
        let state = self.output_state.get_mut(output).unwrap();

        if res == RenderResult::Skipped {
            // Update the redraw state on failed render.
            state.redraw_state = if let RedrawState::WaitingForEstimatedVBlank(token)
            | RedrawState::WaitingForEstimatedVBlankAndQueued(token) =
                state.redraw_state
            {
                RedrawState::WaitingForEstimatedVBlank(token)
            } else {
                RedrawState::Idle
            };
        }

        // Update the lock render state on successful render, or if this output is inactive. An
        // inactive TTY output has no framebuffer attached, so no sensitive data from a last render
        // is visible.
        if res != RenderResult::Skipped || !active {
            state.lock_render_state = if is_locked {
                LockRenderState::Locked
            } else {
                LockRenderState::Unlocked
            };
        }

        // If we're in process of locking the session, check if the requirements were met.
        match mem::take(&mut self.lock_state) {
            LockState::Locking(confirmation) => {
                if state.lock_render_state == LockRenderState::Unlocked {
                    // A transient render failure must not abandon the lock request: doing so drops
                    // the confirmation while the client waits forever for either locked or
                    // finished. Retry later rather than spinning in redraw_queued_outputs().
                    self.lock_state = LockState::Locking(confirmation);
                } else {
                    // Check if all outputs are now locked.
                    let all_locked = self
                        .output_state
                        .values()
                        .all(|state| state.lock_render_state == LockRenderState::Locked);

                    if all_locked {
                        // All outputs are locked, report success.
                        let lock = confirmation.ext_session_lock().clone();
                        confirmation.lock();
                        self.lock_state = LockState::Locked(lock);
                    } else {
                        // Still waiting for other outputs.
                        self.lock_state = LockState::Locking(confirmation);
                    }
                }
            }
            lock_state => self.lock_state = lock_state,
        }

        self.refresh_on_demand_vrr(backend, output);

        // Send the frame callbacks.
        //
        // FIXME: The logic here could be a bit smarter. Currently, during an animation, the
        // surfaces that are visible for the very last frame (e.g. because the camera is moving
        // away) will receive frame callbacks, and the surfaces that are invisible but will become
        // visible next frame will not receive frame callbacks (so they will show stale contents for
        // one frame). We could advance the animations for the next frame and send frame callbacks
        // according to the expected new positions.
        //
        // However, this should probably be restricted to sending frame callbacks to more surfaces,
        // to err on the safe side.
        self.send_frame_callbacks(output);
        backend.with_primary_renderer(|renderer| {
            #[cfg(feature = "xdp-gnome-screencast")]
            {
                // Render and send to PipeWire screencast streams.
                self.render_for_screen_cast(renderer, output, target_presentation_time);

                // FIXME: when a window is hidden, it should probably still receive frame callbacks
                // and get rendered for screen cast. This is currently
                // unimplemented, but happens to work by chance, since output
                // redrawing is more eager than it should be.
                self.render_windows_for_screen_cast(renderer, output, target_presentation_time);
            }

            self.render_for_screencopy_with_damage(renderer, output);
        });
    }

    pub fn refresh_on_demand_vrr(&mut self, backend: &mut Backend, output: &Output) {
        let _span = tracy_client::span!("Swayward::refresh_on_demand_vrr");

        let name = output.user_data().get::<OutputName>().unwrap();
        let on_demand = self
            .config
            .borrow()
            .outputs
            .find(name)
            .is_some_and(|output| output.is_vrr_on_demand());
        if !on_demand {
            return;
        }

        let current = self.layout.windows_for_output(output).any(|mapped| {
            mapped.rules().variable_refresh_rate == Some(true) && {
                let mut visible = false;
                mapped.window.with_surfaces(|surface, states| {
                    if !visible
                        && surface_primary_scanout_output(surface, states).as_ref() == Some(output)
                    {
                        visible = true;
                    }
                });
                visible
            }
        });

        backend.set_output_on_demand_vrr(self, output, current);
    }

    pub fn update_primary_scanout_output(
        &self,
        output: &Output,
        render_element_states: &RenderElementStates,
    ) {
        // FIXME: potentially tweak the compare function. The default one currently always prefers a
        // higher refresh-rate output, which is not always desirable (i.e. with a very small
        // overlap).
        //
        // While we only have cursors and DnD icons crossing output boundaries though, it doesn't
        // matter all that much.
        if let CursorImageStatus::Surface(surface) = &self.cursor_manager.cursor_image() {
            with_surface_tree_downward(
                surface,
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |surface, states, _| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        default_primary_scanout_output_compare,
                    );
                },
                |_, _, _| true,
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            with_surface_tree_downward(
                surface,
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |surface, states, _| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        default_primary_scanout_output_compare,
                    );
                },
                |_, _, _| true,
            );
        }

        // We're only updating the current output's windows and layer surfaces. This should be fine
        // as in niri they can only be rendered on a single output at a time.
        //
        // The reason to do this at all is that it keeps track of whether the surface is visible or
        // not in a unified way with the pointer surfaces, which makes the logic elsewhere simpler.

        for mapped in self.layout.windows_for_output(output) {
            let win = &mapped.window;
            let offscreen_data = mapped.offscreen_data();
            let offscreen_data = offscreen_data.as_ref();

            win.with_surfaces(|surface, states| {
                let primary_scanout_output = states
                    .data_map
                    .get_or_insert_threadsafe(Mutex::<PrimaryScanoutOutput>::default);
                let mut primary_scanout_output = primary_scanout_output.lock().unwrap();

                let mut id = Id::from_wayland_resource(surface);

                if let Some(data) = offscreen_data {
                    // We have offscreen data; it's likely that all surfaces are on it.
                    if data.states.element_was_presented(id.clone()) {
                        // If the surface was presented to the offscreen, use the offscreen's id.
                        id = data.id.clone();
                    }

                    // If we the surface wasn't presented to the offscreen it can mean:
                    //
                    // - The surface was invisible. For example, it's obscured by another surface on
                    //   the offscreen, or simply isn't mapped.
                    // - The surface is rendered separately from the offscreen, for example: popups
                    //   during the window resize animation.
                    //
                    // In both of these cases, using the original surface element id and the
                    // original states is the correct thing to do. We may find the surface in the
                    // original states (in the second case). Either way, we definitely know it is
                    // *not* in the offscreen, and we won't miss it.
                    //
                    // There's one edge case: if the surface is both in the offscreen and separate,
                    // and the offscreen itself is invisible, while the separate surface is
                    // visible. In this case we'll currently mark the surface as invisible. We
                    // don't really use offscreens like that however, and if we start, it's easy
                    // enough to fix (need an extra check).
                }

                primary_scanout_output.update_from_render_element_states(
                    id,
                    output,
                    None,
                    render_element_states,
                    |_, _, output, _| output,
                );
            });
        }

        let xray = &self.output_state[output].xray;
        let xray_bg = xray.background[RenderTarget::Output as usize].borrow();
        let xray_bd = xray.backdrop[RenderTarget::Output as usize].borrow();

        for layer in layer_map_for_output(output).layers() {
            let surface = layer.wl_surface();
            let is_background = layer.layer() == Layer::Background;

            with_surfaces_surface_tree(surface, |surface, states| {
                let primary_scanout_output = states
                    .data_map
                    .get_or_insert_threadsafe(Mutex::<PrimaryScanoutOutput>::default);
                let mut primary_scanout_output = primary_scanout_output.lock().unwrap();
                let mut id = Id::from_wayland_resource(surface);

                // Background layers may be invisible normally but visible through an xray
                // background effect. Try to find it and use the xray element's id in this case.
                //
                // FIXME: this won't work if there's another layer of offscreen (e.g. window with
                // an xray background during its opening animation). But hopefully with the
                // refactor to draw background effects outside offscreens it won't be a problem.
                if is_background && !render_element_states.element_was_presented(id.clone()) {
                    // A layer may be present either in background or backdrop, never in both.
                    if xray_bg
                        .render_element_states()
                        .is_some_and(|s| s.element_was_presented(id.clone()))
                    {
                        id = xray_bg.id().clone();
                    } else if xray_bd
                        .render_element_states()
                        .is_some_and(|s| s.element_was_presented(id.clone()))
                    {
                        id = xray_bd.id().clone();
                    }
                }

                primary_scanout_output.update_from_render_element_states(
                    id,
                    output,
                    None,
                    render_element_states,
                    // Layer surfaces are shown only on one output at a time.
                    |_, _, output, _| output,
                );
            });

            // Popups never go into xray buffers.
            for (popup, _) in PopupManager::popups_for_surface(surface) {
                let surface = popup.wl_surface();
                with_surfaces_surface_tree(surface, |surface, states| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        // Layer surfaces are shown only on one output at a time.
                        |_, _, output, _| output,
                    );
                });
            }
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            with_surface_tree_downward(
                surface.wl_surface(),
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |surface, states, _| {
                    update_surface_primary_scanout_output(
                        surface,
                        output,
                        states,
                        None,
                        render_element_states,
                        default_primary_scanout_output_compare,
                    );
                },
                |_, _, _| true,
            );
        }
    }

    pub fn send_dmabuf_feedbacks(
        &self,
        output: &Output,
        feedback: &SurfaceDmabufFeedback,
        render_element_states: &RenderElementStates,
    ) {
        let _span = tracy_client::span!("Swayward::send_dmabuf_feedbacks");

        // We can unconditionally send the current output's feedback to regular and layer-shell
        // surfaces, as they can only be displayed on a single output at a time. Even if a surface
        // is currently invisible, this is the DMABUF feedback that it should know about.
        for mapped in self.layout.windows_for_output(output) {
            mapped.window.send_dmabuf_feedback(
                output,
                |_, _| Some(output.clone()),
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        for surface in layer_map_for_output(output).layers() {
            surface.send_dmabuf_feedback(
                output,
                |_, _| Some(output.clone()),
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            send_dmabuf_feedback_surface_tree(
                surface.wl_surface(),
                output,
                |_, _| Some(output.clone()),
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            send_dmabuf_feedback_surface_tree(
                surface,
                output,
                surface_primary_scanout_output,
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }

        if let CursorImageStatus::Surface(surface) = &self.cursor_manager.cursor_image() {
            send_dmabuf_feedback_surface_tree(
                surface,
                output,
                surface_primary_scanout_output,
                |surface, _| {
                    select_dmabuf_feedback(
                        surface,
                        render_element_states,
                        &feedback.render,
                        &feedback.scanout,
                    )
                },
            );
        }
    }

    pub fn send_frame_callbacks(&mut self, output: &Output) {
        let _span = tracy_client::span!("Swayward::send_frame_callbacks");

        let state = self.output_state.get(output).unwrap();
        let sequence = state.frame_callback_sequence;

        let should_send = |surface: &WlSurface, states: &SurfaceData| {
            // Do the standard primary scanout output check. For pointer surfaces it deduplicates
            // the frame callbacks across potentially multiple outputs, and for regular windows and
            // layer-shell surfaces it avoids sending frame callbacks to invisible surfaces.
            let current_primary_output = surface_primary_scanout_output(surface, states);
            if current_primary_output.as_ref() != Some(output) {
                return None;
            }

            // Next, check the throttling status.
            let frame_throttling_state = states
                .data_map
                .get_or_insert(SurfaceFrameThrottlingState::default);
            let mut last_sent_at = frame_throttling_state.last_sent_at.borrow_mut();

            let mut send = true;

            // If we already sent a frame callback to this surface this output refresh
            // cycle, don't send one again to prevent empty-damage commit busy loops.
            if let Some((last_output, last_sequence)) = &*last_sent_at {
                if last_output == output && *last_sequence == sequence {
                    send = false;
                }
            }

            if send {
                *last_sent_at = Some((output.clone(), sequence));
                Some(output.clone())
            } else {
                None
            }
        };

        let frame_callback_time = get_monotonic_time();

        for mapped in self.layout.windows_for_output_mut(output) {
            mapped.send_frame(
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        for surface in layer_map_for_output(output).layers() {
            surface.send_frame(
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            send_frames_surface_tree(
                surface.wl_surface(),
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }

        if let CursorImageStatus::Surface(surface) = self.cursor_manager.cursor_image() {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                should_send,
            );
        }
    }

    pub fn send_frame_callbacks_on_fallback_timer(&mut self) {
        let _span = tracy_client::span!("Swayward::send_frame_callbacks_on_fallback_timer");

        // Make up a bogus output; we don't care about it here anyway, just the throttling timer.
        let output = Output::new(
            String::new(),
            PhysicalProperties {
                size: Size::from((0, 0)),
                subpixel: Subpixel::Unknown,
                make: String::new(),
                model: String::new(),
                serial_number: String::new(),
            },
        );
        let output = &output;

        let frame_callback_time = get_monotonic_time();

        self.layout.with_windows_mut(|mapped, _| {
            mapped.send_frame(
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                |_, _| None,
            );
        });

        for (output, state) in self.output_state.iter() {
            for surface in layer_map_for_output(output).layers() {
                surface.send_frame(
                    output,
                    frame_callback_time,
                    FRAME_CALLBACK_THROTTLE,
                    |_, _| None,
                );
            }

            if let Some(surface) = &state.lock_surface {
                send_frames_surface_tree(
                    surface.wl_surface(),
                    output,
                    frame_callback_time,
                    FRAME_CALLBACK_THROTTLE,
                    |_, _| None,
                );
            }
        }

        if let Some(surface) = &self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                |_, _| None,
            );
        }

        if let CursorImageStatus::Surface(surface) = self.cursor_manager.cursor_image() {
            send_frames_surface_tree(
                surface,
                output,
                frame_callback_time,
                FRAME_CALLBACK_THROTTLE,
                |_, _| None,
            );
        }
    }

    pub fn take_presentation_feedbacks(
        &mut self,
        output: &Output,
        render_element_states: &RenderElementStates,
    ) -> OutputPresentationFeedback {
        let mut feedback = OutputPresentationFeedback::new(output);

        if let CursorImageStatus::Surface(surface) = &self.cursor_manager.cursor_image() {
            take_presentation_feedback_surface_tree(
                surface,
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        if let Some(surface) = self.dnd_icon.as_ref().map(|icon| &icon.surface) {
            take_presentation_feedback_surface_tree(
                surface,
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        for mapped in self.layout.windows_for_output(output) {
            mapped.window.take_presentation_feedback(
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            )
        }

        for surface in layer_map_for_output(output).layers() {
            surface.take_presentation_feedback(
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        if let Some(surface) = &self.output_state[output].lock_surface {
            take_presentation_feedback_surface_tree(
                surface.wl_surface(),
                &mut feedback,
                surface_primary_scanout_output,
                |surface, _| {
                    surface_presentation_feedback_flags_from_states(
                        surface,
                        None,
                        render_element_states,
                    )
                },
            );
        }

        feedback
    }
}

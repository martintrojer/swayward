impl Mapped {
    pub fn new(window: Window, rules: ResolvedWindowRules, hook: HookId, config: &Config) -> Self {
        let surface = window.wl_surface().expect("no X11 support");
        let credentials = get_credentials_for_surface(&surface);
        let security_context = surface.client().and_then(|client| {
            client
                .get_data::<ClientState>()
                .and_then(|data| data.security_context.clone())
        });
        let has_xdg_decoration = window.toplevel().is_some_and(|toplevel| {
            toplevel.with_pending_state(|state| {
                state.decoration_mode == Some(zxdg_toplevel_decoration_v1::Mode::ClientSide)
            })
        });
        let natural_size = window.geometry().size;
        let mut rv = Self {
            window,
            id: MappedId::next(),
            credentials,
            pre_commit_hook: hook,
            rules,
            need_to_recompute_rules: false,
            needs_configure: false,
            needs_frame_callback: false,
            offscreen_data: RefCell::new(None),
            urgent_since: None,
            titlebar_marks: Vec::new(),
            is_focused: false,
            is_active_in_column: true,
            is_floating: false,
            natural_size,
            has_xdg_decoration,
            is_window_cast_target: false,
            shortcuts_inhibit_policy: ShortcutsInhibitPolicy::Default,
            ignore_opacity_window_rule: false,
            command_opacity: 1.,
            block_out_buffer: RefCell::new(SolidColorBuffer::new((0., 0.), [0., 0., 0., 1.])),
            blur_config: config.blur,
            animate_next_configure: false,
            animate_serials: Vec::new(),
            animation_snapshot: None,
            request_size_once: None,
            transaction_for_next_configure: None,
            pending_transactions: Vec::new(),
            interactive_resize: None,
            last_interactive_resize_start: Cell::new(None),
            is_windowed_fullscreen: false,
            is_pending_windowed_fullscreen: false,
            uncommitted_windowed_fullscreen: Vec::new(),
            is_maximized: false,
            is_pending_maximized: false,
            uncommitted_maximized: Vec::new(),
            title_format: None,
            security_context,
            focus_timestamp: None,
        };

        rv.is_maximized = rv.sizing_mode().is_maximized();
        rv.is_pending_maximized = rv.pending_sizing_mode().is_maximized();

        rv
    }

    pub fn toplevel(&self) -> &ToplevelSurface {
        self.window.toplevel().expect("no X11 support")
    }

    /// Recomputes the resolved window rules and returns whether they changed.
    pub fn recompute_window_rules(&mut self, rules: &[WindowRule], is_at_startup: bool) -> bool {
        self.need_to_recompute_rules = false;

        let new_rules = ResolvedWindowRules::compute(rules, WindowRef::Mapped(self), is_at_startup);
        if new_rules == self.rules {
            return false;
        }

        // If the opacity window rule no longer makes the window semitransparent, reset the ignore
        // flag to reduce surprises down the line.
        if !new_rules.opacity.is_some_and(|o| o < 1.) {
            self.ignore_opacity_window_rule = false;
        }

        self.rules = new_rules;
        true
    }

    pub fn recompute_window_rules_if_needed(
        &mut self,
        rules: &[WindowRule],
        is_at_startup: bool,
    ) -> bool {
        if !self.need_to_recompute_rules {
            return false;
        }

        self.recompute_window_rules(rules, is_at_startup)
    }

    pub fn set_needs_configure(&mut self) {
        self.needs_configure = true;
    }

    pub fn id(&self) -> MappedId {
        self.id
    }

    pub fn credentials(&self) -> Option<&Credentials> {
        self.credentials.as_ref()
    }

    pub fn security_context(&self) -> Option<&SecurityContextMetadata> {
        self.security_context.as_ref()
    }

    pub fn tag(&self) -> Option<std::sync::Arc<str>> {
        use smithay::wayland::xdg_toplevel_tag::XdgToplevelTagSurfaceData;

        with_states(self.toplevel().wl_surface(), |states| {
            states
                .data_map
                .get::<XdgToplevelTagSurfaceData>()
                .and_then(|data| data.tag())
        })
    }

    pub fn set_title_format(&mut self, format: String) {
        self.title_format = (format != "%title").then_some(format);
    }

    pub fn formatted_title(&self) -> String {
        let (title, app_id) = with_toplevel_role(self.toplevel(), |role| {
            (
                role.title.clone().unwrap_or_default(),
                role.app_id.clone().unwrap_or_default(),
            )
        });
        let Some(format) = &self.title_format else {
            return title;
        };
        let security_context = self.security_context.as_ref();
        format_title(
            format,
            &title,
            &app_id,
            "xdg_shell",
            security_context.and_then(|context| context.sandbox_engine.as_deref()),
            security_context.and_then(|context| context.app_id.as_deref()),
            security_context.and_then(|context| context.instance_id.as_deref()),
        )
    }

    pub fn offscreen_data(&self) -> Ref<'_, Option<OffscreenData>> {
        self.offscreen_data.borrow()
    }

    pub fn resolved_rules(&self) -> &ResolvedWindowRules {
        &self.rules
    }

    pub fn is_focused(&self) -> bool {
        self.is_focused
    }

    pub fn is_active_in_column(&self) -> bool {
        self.is_active_in_column
    }

    pub fn is_floating(&self) -> bool {
        self.is_floating
    }

    /// Publish the float decision made at map time, before the layout's own
    /// update sets it, so `for_window` criteria can see it.
    pub fn set_floating_for_rules(&mut self, floating: bool) {
        let changed = self.is_floating != floating;
        self.is_floating = floating;
        self.need_to_recompute_rules |= changed;
    }

    pub fn is_window_cast_target(&self) -> bool {
        self.is_window_cast_target
    }

    pub fn shortcuts_inhibit_policy(&self) -> ShortcutsInhibitPolicy {
        self.shortcuts_inhibit_policy
    }

    pub fn set_shortcuts_inhibit_policy(&mut self, policy: ShortcutsInhibitPolicy) {
        self.shortcuts_inhibit_policy = policy;
    }

    pub fn toggle_ignore_opacity_window_rule(&mut self) {
        self.ignore_opacity_window_rule = !self.ignore_opacity_window_rule;
    }

    pub fn command_opacity(&self) -> f32 {
        self.command_opacity
    }

    pub fn set_command_opacity(&mut self, opacity: f32) {
        self.command_opacity = opacity;
    }

    pub fn set_titlebar_marks(&mut self, marks: Vec<String>) {
        self.titlebar_marks = marks;
    }

    pub fn set_is_focused(&mut self, is_focused: bool) {
        if self.is_focused == is_focused {
            return;
        }

        self.is_focused = is_focused;
        self.need_to_recompute_rules = true;
    }

    pub fn set_is_window_cast_target(&mut self, value: bool) {
        if self.is_window_cast_target == value {
            return;
        }

        self.is_window_cast_target = value;
        self.need_to_recompute_rules = true;
    }

    /// Renders a snapshot of the window without popups.
    fn render_snapshot(&self, renderer: &mut GlesRenderer) -> LayoutElementRenderSnapshot {
        let _span = tracy_client::span!("Mapped::render_snapshot");

        let size = self.size().to_f64();

        let mut buffer = self.block_out_buffer.borrow_mut();
        buffer.resize(size);
        let blocked_out_contents = vec![BakedBuffer {
            buffer: buffer.clone(),
            location: Point::from((0., 0.)),
            src: None,
            dst: None,
        }];

        let buf_pos = self.window.geometry().loc.upscale(-1).to_f64();

        let mut contents = vec![];

        let surface = self.toplevel().wl_surface();
        render_snapshot_from_surface_tree(renderer, surface, buf_pos, &mut contents);

        RenderSnapshot {
            contents,
            contents_with_blocked_out_bg: None,
            blocked_out_contents,
            block_out_from: self.rules().block_out_from,
            size,
            texture: Default::default(),
            texture_with_blocked_out_bg: Default::default(),
            blocked_out_texture: Default::default(),
        }
    }

    pub fn should_animate_commit(&mut self, commit_serial: Serial) -> bool {
        let mut should_animate = false;
        self.animate_serials.retain_mut(|serial| {
            if commit_serial.is_no_older_than(serial) {
                should_animate = true;
                false
            } else {
                true
            }
        });
        should_animate
    }

    pub fn store_animation_snapshot(&mut self, renderer: &mut GlesRenderer) {
        self.animation_snapshot = Some(self.render_snapshot(renderer));
    }

    pub fn take_pending_transaction(&mut self, commit_serial: Serial) -> Option<Transaction> {
        let mut rv = None;

        // Pending transactions are appended in order by serial, so we can loop from the start
        // until we hit a serial that is too new.
        while let Some((serial, _)) = self.pending_transactions.first() {
            // In this loop, we will complete the transaction corresponding to the commit, as well
            // as all transactions corresponding to previous serials. This can happen when we
            // request resizes too quickly, and the surface only responds to the last one.
            //
            // Note that in this case, completing the previous transactions can result in an
            // inconsistent visual state, if another window is waiting for this window to assume a
            // specific size (in a previous transaction), which is now different (in this commit).
            //
            // However, there isn't really a good way to deal with that. We cannot cancel any
            // transactions because we need to keep sending frame callbacks, and cancelling a
            // transaction will make the corresponding frame callbacks get lost, and the window
            // will hang.
            //
            // This is why resize throttling (implemented separately) is important: it prevents
            // visually inconsistent states by way of never having more than one transaction in
            // flight.
            if commit_serial.is_no_older_than(serial) {
                let (_, transaction) = self.pending_transactions.remove(0);
                // Previous transaction is dropped here, signaling completion.
                rv = Some(transaction);
            } else {
                break;
            }
        }

        rv
    }

    pub fn last_interactive_resize_start(&self) -> &Cell<Option<(Duration, ResizeEdge)>> {
        &self.last_interactive_resize_start
    }

    pub fn render_for_screen_cast<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        scale: Scale<f64>,
        push: &mut dyn FnMut(WindowCastRenderElements<R>),
    ) {
        let bbox = self.window.bbox_with_popups().to_physical_precise_up(scale);

        let has_border_shader = BorderRenderElement::has_shader(renderer);
        let radius = self.geometry_corner_radius();
        let window_size = self
            .size()
            .to_f64()
            .to_physical_precise_round(scale)
            .to_logical(scale);
        let radius = radius.fit_to(window_size.w as f32, window_size.h as f32);
        let location = self.window.geometry().loc.to_f64() - bbox.loc.to_logical(scale);

        let use_border = |elem| {
            if let LayoutElementRenderElement::SolidColor(elem) = &elem {
                // In this branch we're rendering a blocked-out window with a solid color. We need
                // to render it with a rounded corner shader even if clip_to_geometry is false,
                // because in this case we're assuming that the unclipped window CSD already has
                // corners rounded to the user-provided radius, so our blocked-out rendering should
                // match that radius.
                if radius != CornerRadius::default() && has_border_shader {
                    let geo = elem.geo();
                    return BorderRenderElement::new(
                        geo.size,
                        Rectangle::from_size(geo.size),
                        GradientInterpolation::default(),
                        Color::from_color32f(elem.color()),
                        Color::from_color32f(elem.color()),
                        0.,
                        Rectangle::from_size(geo.size),
                        0.,
                        radius,
                        scale.x as f32,
                        1.,
                    )
                    .with_location(geo.loc)
                    .into();
                }
            }

            WindowCastRenderElements::from(elem)
        };

        self.render(
            RenderCtx {
                renderer,
                target: RenderTarget::Screencast,
                xray: None,
            },
            location,
            scale,
            1.,
            XrayPos::default(),
            &mut |elem| push(use_border(elem)),
        );
    }

    pub fn get_focus_timestamp(&self) -> Option<Duration> {
        self.focus_timestamp
    }

    pub fn set_focus_timestamp(&mut self, timestamp: Duration) {
        self.focus_timestamp.replace(timestamp);
    }

    pub fn send_frame<T, F>(
        &mut self,
        output: &Output,
        time: T,
        throttle: Option<Duration>,
        mut primary_scan_out_output: F,
    ) where
        T: Into<Duration>,
        F: FnMut(&WlSurface, &SurfaceData) -> Option<Output> + Copy,
    {
        let needs_frame_callback = self.needs_frame_callback;
        self.needs_frame_callback = false;

        let should_send = move |surface: &WlSurface, states: &SurfaceData| {
            // Let primary_scan_out_output() run its logic and update internal state.
            if let Some(output) = primary_scan_out_output(surface, states) {
                return Some(output);
            }

            // Send unconditionally to all surfaces if the window needs a surface callback.
            needs_frame_callback.then(|| output.clone())
        };
        self.window.send_frame(output, time, throttle, should_send);
    }

    pub fn update_tiled_state(&self, prefer_no_csd: bool) {
        update_tiled_state(self.toplevel(), prefer_no_csd, self.rules.tiled_state);
    }

    pub fn is_windowed_fullscreen(&self) -> bool {
        self.is_windowed_fullscreen
    }

    pub fn set_urgent(&mut self, urgent: bool) {
        self.set_urgent_at(urgent, get_monotonic_time());
    }

    fn set_urgent_at(&mut self, urgent: bool, now: Duration) {
        if self.is_focused && urgent {
            return;
        }

        let was_urgent = self.urgent_since.is_some();
        self.urgent_since = urgent.then_some(now);
        self.need_to_recompute_rules |= was_urgent != urgent;
    }

    #[cfg(test)]
    pub fn set_urgent_for_test(&mut self, urgent: bool, now: Duration) {
        self.set_urgent_at(urgent, now);
    }

    pub fn is_urgent(&self) -> bool {
        self.urgent_since.is_some()
    }

    pub fn urgent_since(&self) -> Option<Duration> {
        self.urgent_since
    }
}

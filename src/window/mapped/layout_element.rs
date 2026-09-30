impl Drop for Mapped {
    fn drop(&mut self) {
        remove_pre_commit_hook(self.toplevel().wl_surface(), &self.pre_commit_hook);
    }
}

impl LayoutElement for Mapped {
    type Id = Window;

    fn id(&self) -> &Self::Id {
        &self.window
    }

    fn focus_timestamp(&self) -> Option<Duration> {
        self.focus_timestamp
    }

    fn update_config(&mut self, blur_config: swayward_config::Blur) {
        self.blur_config = blur_config;
    }

    fn title(&self) -> String {
        self.formatted_title()
    }

    fn marks(&self) -> Vec<String> {
        self.titlebar_marks.clone()
    }

    fn size(&self) -> Size<i32, Logical> {
        self.window.geometry().size
    }

    fn buf_loc(&self) -> Point<i32, Logical> {
        Point::from((0, 0)) - self.window.geometry().loc
    }

    fn is_in_input_region(&self, point: Point<f64, Logical>) -> bool {
        let surface_local = point + self.window.geometry().loc.to_f64();
        self.window.is_in_input_region(&surface_local)
    }

    fn is_in_popup_input_region(&self, point: Point<f64, Logical>) -> bool {
        let surface_local = point + self.window.geometry().loc.to_f64();
        self.window
            .surface_under(surface_local, WindowSurfaceType::POPUP)
            .is_some()
    }

    fn render_normal<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        scale: Scale<f64>,
        alpha: f32,
        push: &mut dyn FnMut(LayoutElementRenderElement<R>),
    ) {
        if ctx.target.should_block_out(self.rules.block_out_from) {
            let mut buffer = self.block_out_buffer.borrow_mut();
            buffer.resize(self.window.geometry().size.to_f64());
            let elem =
                SolidColorRenderElement::from_buffer(&buffer, location, alpha, Kind::Unspecified);
            push(elem.into());
        } else {
            let buf_pos = location - self.window.geometry().loc.to_f64();
            let surface = self.toplevel().wl_surface();
            let mut push = |elem: WaylandSurfaceRenderElement<R>| push(elem.into());
            push_elements_from_surface_tree(
                ctx.renderer,
                surface,
                buf_pos.to_physical_precise_round(scale),
                scale,
                alpha,
                Kind::ScanoutCandidate,
                &mut push,
            )
        }
    }

    fn render_popups<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        scale: Scale<f64>,
        alpha: f32,
        xray_pos: XrayPos,
        push: &mut dyn FnMut(LayoutElementRenderElement<R>),
    ) {
        if ctx.target.should_block_out(self.rules.block_out_from) {
            return;
        }

        let surface = self.toplevel().wl_surface();
        for (popup, offset) in PopupManager::popups_for_surface(surface) {
            let popup_rules = match popup {
                PopupKind::Xdg(_) => self.rules.popups,
                // IME popups aren't affected by rules for regular popups.
                PopupKind::InputMethod(_) => swayward_config::ResolvedPopupsRules::default(),
            };
            let alpha = alpha * popup_rules.opacity.unwrap_or(1.).clamp(0., 1.);

            let surface = popup.wl_surface();
            let popup_geo = popup.geometry();
            let surface_loc = location + (offset - popup.geometry().loc).to_f64();

            push_elements_from_surface_tree(
                ctx.renderer,
                surface,
                surface_loc.to_physical_precise_round(scale),
                scale,
                alpha,
                Kind::ScanoutCandidate,
                &mut |elem| push(elem.into()),
            );

            let geometry = Rectangle::new(location + offset.to_f64(), popup_geo.size.to_f64());
            let surface_off = popup_geo.loc.upscale(-1).to_f64();
            let surface_anim_scale = Scale::from(1.);
            let mut effect = popup_rules.background_effect;
            // Default xray to false for pop-ups since they're always on top of something.
            if effect.xray.is_none() {
                effect.xray = Some(false);
            }
            let xray_pos = xray_pos.offset(offset.to_f64());
            background_effect::render_for_tile(
                ctx.as_gles(),
                None,
                geometry,
                scale.x,
                false,
                surface,
                surface_off,
                surface_anim_scale,
                self.blur_config,
                popup_rules.geometry_corner_radius.unwrap_or_default(),
                effect,
                false,
                xray_pos,
                &mut |elem| push(elem.into()),
            );
        }
    }

    fn render_background_effect(
        &self,
        ctx: RenderCtx<GlesRenderer>,
        geometry: Rectangle<f64, Logical>,
        scale: f64,
        clip_to_geometry: bool,
        surface_anim_scale: Scale<f64>,
        radius: CornerRadius,
        xray_pos: XrayPos,
        push: &mut dyn FnMut(BackgroundEffectElement),
    ) {
        let should_block_out = ctx.target.should_block_out(self.rules.block_out_from);
        background_effect::render_for_tile(
            ctx,
            None,
            geometry,
            scale,
            clip_to_geometry,
            self.toplevel().wl_surface(),
            self.buf_loc().to_f64(),
            surface_anim_scale,
            self.blur_config,
            radius,
            self.rules.background_effect,
            should_block_out,
            xray_pos,
            push,
        );
    }

    fn request_size(
        &mut self,
        size: Size<i32, Logical>,
        mode: SizingMode,
        animate: bool,
        transaction: Option<Transaction>,
    ) {
        // Going into real fullscreen resets windowed fullscreen.
        if mode == SizingMode::Fullscreen {
            self.is_pending_windowed_fullscreen = false;

            if self.is_windowed_fullscreen {
                // Make sure we receive a commit to update self.is_windowed_fullscreen to false
                // later on.
                self.needs_configure = true;
            }
        }

        self.is_pending_maximized = mode == SizingMode::Maximized;
        if self.is_maximized != self.is_pending_maximized {
            // Make sure we receive a commit to update self.is_maximized later on.
            self.needs_configure = true;
        }

        let changed = self.toplevel().with_pending_state(|state| {
            let changed = state.size != Some(size);
            state.size = Some(size);

            if mode.is_fullscreen() || self.is_pending_windowed_fullscreen {
                state.states.set(xdg_toplevel::State::Fullscreen);
                state.states.unset(xdg_toplevel::State::Maximized);
            } else if mode.is_maximized() {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.states.set(xdg_toplevel::State::Maximized);
            } else {
                state.states.unset(xdg_toplevel::State::Fullscreen);
                state.states.unset(xdg_toplevel::State::Maximized);
            }

            changed
        });

        if changed && animate {
            self.animate_next_configure = true;
        }

        self.request_size_once = None;

        // Store the transaction regardless of whether the size changed. This is because with 3+
        // windows in a column, the size may change among windows 1 and 2 and then right away among
        // windows 2 and 3, and we want all windows 1, 2 and 3 to use the last transaction, rather
        // than window 1 getting stuck with the previous transaction that is immediately released
        // by 2.
        if let Some(transaction) = transaction {
            self.transaction_for_next_configure = Some(transaction);
        }
    }

    fn request_size_once(&mut self, size: Size<i32, Logical>, animate: bool) {
        // Assume that when calling this function, the window is going floating, so it can no
        // longer participate in any transactions with other windows.
        self.transaction_for_next_configure = None;

        self.is_pending_maximized = false;
        if self.is_maximized != self.is_pending_maximized {
            // Make sure we receive a commit to update self.is_maximized later on.
            self.needs_configure = true;
        }

        // If our last requested size already matches the size we want to request-once, clear the
        // size request right away. However, we must also check if we're unfullscreening, because
        // in that case the window itself will restore its previous size upon receiving a (0, 0)
        // configure, whereas what we potentially want is to unfullscreen the window into its
        // fullscreen size.
        //
        // The pending state must match as well. A fullscreen or maximize
        // request that has not been sent yet lives only in the pending state;
        // returning early would leave it there, and the next configure would
        // make a floating window fullscreen.
        let wanted = |state: &smithay::wayland::shell::xdg::ToplevelState| {
            state.size.unwrap_or_default() == size
                && state.states.contains(xdg_toplevel::State::Fullscreen)
                    == self.is_pending_windowed_fullscreen
                && !state.states.contains(xdg_toplevel::State::Maximized)
        };
        let pending_matches = self.toplevel().with_pending_state(|state| wanted(state));
        let already_sent = with_toplevel_last_uncommitted_configure(self.toplevel(), |configure| {
            let ToplevelConfigure { state, serial } = configure?;
            (pending_matches && wanted(state)).then_some(*serial)
        });

        if let Some(serial) = already_sent {
            let current_serial = with_states(self.toplevel().wl_surface(), |states| {
                states
                    .cached_state
                    .get::<ToplevelCachedState>()
                    .current()
                    .last_acked
                    .as_ref()
                    .map(|c| c.serial)
            });
            if let Some(current_serial) = current_serial {
                // God this triple negative...
                if !current_serial.is_no_older_than(&serial) {
                    // We have already sent a request for the new size, but the surface has not
                    // committed in response yet, so we will wait for that commit.
                    self.request_size_once = Some(RequestSizeOnce::WaitingForCommit(serial));
                } else {
                    // We have already sent a request for the new size, and the surface has
                    // committed in response, so we will start using the current size right away.
                    self.request_size_once = Some(RequestSizeOnce::UseWindowSize);
                }
            } else {
                warn!("no current serial; did the surface not ack the initial configure?");
                self.request_size_once = Some(RequestSizeOnce::UseWindowSize);
            };
            return;
        }

        let changed = self.toplevel().with_pending_state(|state| {
            let changed = state.size != Some(size);
            state.size = Some(size);
            if !self.is_pending_windowed_fullscreen {
                state.states.unset(xdg_toplevel::State::Fullscreen);
            }
            state.states.unset(xdg_toplevel::State::Maximized);
            changed
        });

        if changed && animate {
            self.animate_next_configure = true;
        }

        self.request_size_once = Some(RequestSizeOnce::WaitingForConfigure);
    }

    fn min_size(&self) -> Size<i32, Logical> {
        let min_size = with_states(self.toplevel().wl_surface(), |state| {
            let mut guard = state.cached_state.get::<SurfaceCachedState>();
            guard.current().min_size
        });

        self.rules.apply_min_size(min_size)
    }

    fn max_size(&self) -> Size<i32, Logical> {
        let max_size = with_states(self.toplevel().wl_surface(), |state| {
            let mut guard = state.cached_state.get::<SurfaceCachedState>();
            guard.current().max_size
        });

        self.rules.apply_max_size(max_size)
    }

    fn is_wl_surface(&self, wl_surface: &WlSurface) -> bool {
        self.toplevel().wl_surface() == wl_surface
    }

    fn set_preferred_scale_transform(&self, scale: output::Scale, transform: Transform) {
        self.window.with_surfaces(|surface, data| {
            send_scale_transform(surface, data, scale, transform);
        });
    }

    fn has_ssd(&self) -> bool {
        let toplevel = self.toplevel();
        let mode = self
            .toplevel()
            .with_committed_state(|current| current.and_then(|s| s.decoration_mode));

        match mode {
            Some(zxdg_toplevel_decoration_v1::Mode::ServerSide) => true,
            // Check KDE decorations when XDG are not in use.
            None => with_states(toplevel.wl_surface(), |states| {
                states
                    .data_map
                    .get::<KdeDecorationsModeState>()
                    .map(KdeDecorationsModeState::is_server)
                    == Some(true)
            }),
            _ => false,
        }
    }

    fn output_enter(&self, output: &Output) {
        let overlap = Rectangle::from_size(Size::from((i32::MAX, i32::MAX)));
        self.window.output_enter(output, overlap)
    }

    fn output_leave(&self, output: &Output) {
        self.window.output_leave(output)
    }

    fn set_offscreen_data(&self, data: Option<OffscreenData>) {
        let Some(data) = data else {
            self.offscreen_data.replace(None);
            return;
        };

        let mut offscreen_data = self.offscreen_data.borrow_mut();
        match &mut *offscreen_data {
            None => {
                *offscreen_data = Some(data);
            }
            Some(existing) => {
                // Replace the id, amend existing element states. This is necessary to handle
                // multiple layers of offscreen (e.g. resize animation + alpha animation).
                existing.id = data.id;
                existing.states.states.extend(data.states.states);
            }
        }
    }

    fn is_urgent(&self) -> bool {
        self.is_urgent()
    }

    fn set_activated(&mut self, active: bool) {
        let changed = self.toplevel().with_pending_state(|state| {
            if active {
                state.states.set(xdg_toplevel::State::Activated)
            } else {
                state.states.unset(xdg_toplevel::State::Activated)
            }
        });
        self.need_to_recompute_rules |= changed;
    }

    fn set_active_in_column(&mut self, active: bool) {
        let changed = self.is_active_in_column != active;
        self.is_active_in_column = active;
        self.need_to_recompute_rules |= changed;
    }

    fn set_floating(&mut self, floating: bool) {
        let changed = self.is_floating != floating;
        self.is_floating = floating;
        self.need_to_recompute_rules |= changed;
    }

    fn has_xdg_decoration(&self) -> bool {
        self.has_xdg_decoration
    }

    fn request_server_decoration(&mut self, server_side: bool) {
        self.toplevel().with_pending_state(|state| {
            state.decoration_mode = Some(if server_side {
                zxdg_toplevel_decoration_v1::Mode::ServerSide
            } else {
                zxdg_toplevel_decoration_v1::Mode::ClientSide
            });
        });
        self.set_needs_configure();
    }

    fn set_bounds(&self, bounds: Size<i32, Logical>) {
        self.toplevel().with_pending_state(|state| {
            state.bounds = Some(bounds);
        });
    }

    fn configure_intent(&self) -> ConfigureIntent {
        let _span =
            trace_span!("configure_intent", surface = ?self.toplevel().wl_surface().id()).entered();

        if self.needs_configure {
            trace!("the window needs_configure");
            return ConfigureIntent::ShouldSend;
        }

        with_toplevel_role_and_current(self.toplevel(), |attributes, current_committed| {
            if let Some(server_pending) = &attributes.server_pending {
                let current_server = attributes.current_server_state();
                if *server_pending != current_server {
                    // Something changed. Check if the only difference is the size, and if the
                    // current server size matches the current committed size.
                    let mut current_server_same_size = current_server.clone();
                    current_server_same_size.size = server_pending.size;
                    if current_server_same_size == *server_pending {
                        // Only the size changed. Check if the window committed our previous size
                        // request.
                        let Some(current_committed) = current_committed else {
                            error!("mapped must have had initial commit");
                            return ConfigureIntent::ShouldSend;
                        };

                        if current_committed.size == current_server.size {
                            // The window had committed for our previous size change, so we can
                            // change the size again.
                            trace!(
                                "current size matches server size: {:?}",
                                current_committed.size
                            );
                            ConfigureIntent::CanSend
                        } else {
                            // The window had not committed for our previous size change yet. Since
                            // nothing else changed, do not send the new size request yet. This
                            // throttling is done because some clients do not batch size requests,
                            // leading to bad behavior with very fast input devices (i.e. a 1000 Hz
                            // mouse). This throttling also helps interactive resize transactions
                            // preserve visual consistency.
                            trace!("throttling resize");
                            ConfigureIntent::Throttled
                        }
                    } else {
                        // Something else changed other than the size; send it.
                        trace!("something changed other than the size");
                        ConfigureIntent::ShouldSend
                    }
                } else {
                    // Nothing changed since the last configure.
                    ConfigureIntent::NotNeeded
                }
            } else {
                // Nothing changed since the last configure.
                ConfigureIntent::NotNeeded
            }
        })
    }

    fn send_pending_configure(&mut self) {
        let toplevel = self.toplevel();
        let _span =
            trace_span!("send_pending_configure", surface = ?toplevel.wl_surface().id()).entered();

        // If the window needs a configure, send it regardless.
        let has_pending_changes = self.needs_configure
            || with_toplevel_role(self.toplevel(), |role| {
                // Check for pending changes manually to account for RequestSizeOnce::UseWindowSize.
                if role.server_pending.is_none() {
                    return false;
                }

                let current_server_size = role.current_server_state().size;
                let server_pending = role.server_pending.as_mut().unwrap();

                // With UseWindowSize, we do not consider size-only changes, because we will
                // request the current window size and do not expect it to actually change.
                if let Some(RequestSizeOnce::UseWindowSize) = self.request_size_once {
                    server_pending.size = current_server_size;
                }

                let server_pending = role.server_pending.as_ref().unwrap();
                *server_pending != role.current_server_state()
            });

        if has_pending_changes {
            // If needed, replace the pending size with the current window size.
            if let Some(RequestSizeOnce::UseWindowSize) = self.request_size_once {
                let size = self.window.geometry().size;
                toplevel.with_pending_state(|state| {
                    state.size = Some(size);
                });
            }

            let serial = toplevel.send_configure();
            trace!(?serial, "sending configure");

            self.needs_configure = false;

            // Send the window a frame callback unconditionally to let it respond to size changes
            // and such immediately, even when it's hidden. This especially matters for cases like
            // tabbed columns which compute their width based on all windows in the column, even
            // hidden ones.
            self.needs_frame_callback = true;

            if self.animate_next_configure {
                self.animate_serials.push(serial);
            }

            if let Some(transaction) = self.transaction_for_next_configure.take() {
                self.pending_transactions.push((serial, transaction));
            }

            self.interactive_resize = match self.interactive_resize.take() {
                Some(InteractiveResize::WaitingForLastConfigure(data)) => {
                    Some(InteractiveResize::WaitingForLastCommit { data, serial })
                }
                x => x,
            };

            if let Some(RequestSizeOnce::WaitingForConfigure) = self.request_size_once {
                self.request_size_once = Some(RequestSizeOnce::WaitingForCommit(serial));
            }

            // If is_pending_windowed_fullscreen changed compared to the last value that we "sent"
            // to the window, store the configure serial.
            let last_sent_windowed_fullscreen = self
                .uncommitted_windowed_fullscreen
                .last()
                .map(|(_, value)| *value)
                .unwrap_or(self.is_windowed_fullscreen);
            if last_sent_windowed_fullscreen != self.is_pending_windowed_fullscreen {
                self.uncommitted_windowed_fullscreen
                    .push((serial, self.is_pending_windowed_fullscreen));
            }

            // If is_pending_maximized changed compared to the last value that we "sent" to the
            // window, store the configure serial.
            let last_sent_maximized = self
                .uncommitted_maximized
                .last()
                .map(|(_, value)| *value)
                .unwrap_or(self.is_maximized);
            if last_sent_maximized != self.is_pending_maximized {
                self.uncommitted_maximized
                    .push((serial, self.is_pending_maximized));
            }
        } else {
            self.interactive_resize = match self.interactive_resize.take() {
                // We probably started and stopped resizing in the same loop cycle without anything
                // changing.
                Some(InteractiveResize::WaitingForLastConfigure { .. }) => None,
                x => x,
            };
        }

        self.animate_next_configure = false;
        self.transaction_for_next_configure = None;
    }

    fn sizing_mode(&self) -> SizingMode {
        if self.is_windowed_fullscreen {
            return if self.is_maximized {
                SizingMode::Maximized
            } else {
                SizingMode::Normal
            };
        }

        self.toplevel().with_committed_state(|state| {
            // This must always be Some() for mapped windows. However, this function is called on
            // the code path when removing a just-unmapped window in the commit handler, at which
            // point state is already None.
            let Some(state) = state else {
                return SizingMode::Normal;
            };

            if state.states.contains(xdg_toplevel::State::Fullscreen) {
                SizingMode::Fullscreen
            } else if state.states.contains(xdg_toplevel::State::Maximized) {
                SizingMode::Maximized
            } else {
                SizingMode::Normal
            }
        })
    }

    fn pending_sizing_mode(&self) -> SizingMode {
        if self.is_pending_windowed_fullscreen {
            return if self.is_pending_maximized {
                SizingMode::Maximized
            } else {
                SizingMode::Normal
            };
        }

        self.toplevel().with_pending_state(|state| {
            if state.states.contains(xdg_toplevel::State::Fullscreen) {
                SizingMode::Fullscreen
            } else if state.states.contains(xdg_toplevel::State::Maximized) {
                SizingMode::Maximized
            } else {
                SizingMode::Normal
            }
        })
    }

    fn is_ignoring_opacity_window_rule(&self) -> bool {
        self.ignore_opacity_window_rule
    }

    fn command_opacity(&self) -> f32 {
        self.command_opacity
    }

    fn requested_size(&self) -> Option<Size<i32, Logical>> {
        self.toplevel().with_pending_state(|state| state.size)
    }

    fn natural_size(&self) -> Size<i32, Logical> {
        self.natural_size
    }

    fn ipc_size(&self) -> Size<i32, Logical> {
        if self.request_size_once.is_some() {
            self.requested_size().unwrap_or_else(|| self.size())
        } else {
            self.size()
        }
    }

    fn expected_size(&self) -> Option<Size<i32, Logical>> {
        // We can only use current size if it's not maximized or fullscreen.
        let current_size = (self.sizing_mode().is_normal()).then(|| self.window.geometry().size);

        // Check if we should be using the current window size.
        //
        // This branch can be useful (give different result than the logic below) in this example
        // case:
        //
        // 1. We request_size_once a size change.
        // 2. We send a second configure requesting a state change.
        // 3. The window acks and commits-to the first configure but not the second, with a
        //    different size.
        //
        // In this case self.request_size_once will already flip to UseWindowSize and this branch
        // will return the window's own new size, but the logic below would see an uncommitted size
        // change and return our size.
        if let Some(RequestSizeOnce::UseWindowSize) = self.request_size_once {
            return current_size;
        }

        let pending = with_states(self.toplevel().wl_surface(), |states| {
            let role = states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap();

            // If we have a server-pending size change that we haven't sent yet, use that size.
            let server_pending = role.server_pending.as_ref()?;

            let current_server = role.current_server_state();
            if server_pending.size != current_server.size {
                return Some((
                    server_pending.size.unwrap_or_default(),
                    server_pending
                        .states
                        .contains(xdg_toplevel::State::Fullscreen),
                    server_pending
                        .states
                        .contains(xdg_toplevel::State::Maximized),
                ));
            }

            None
        })
        .or_else(|| {
            with_toplevel_last_uncommitted_configure(self.toplevel(), |configure| {
                // If we have a sent-but-not-committed-to size, use that.
                let ToplevelConfigure { state, .. } = configure?;

                Some((
                    state.size.unwrap_or_default(),
                    state.states.contains(xdg_toplevel::State::Fullscreen),
                    state.states.contains(xdg_toplevel::State::Maximized),
                ))
            })
        });

        if let Some((mut size, fullscreen, maximized)) = pending {
            // If the pending change is maximized or fullscreen, we can't use that size.
            //
            // Pending windowed fullscreen is good (means not real fullscreen), unless it's also
            // pending maximized (means maximized windowed fullscreen, so maximized size, bad).
            if maximized
                || (fullscreen
                    && (!self.is_pending_windowed_fullscreen || self.is_pending_maximized))
            {
                return None;
            }

            // If some component of the pending size is zero, substitute it with the current window
            // size. But only if the current size is not fullscreen.
            if size.w == 0 {
                size.w = current_size?.w;
            }
            if size.h == 0 {
                size.h = current_size?.h;
            }

            Some(size)
        } else {
            // No pending size, return the current size if it's non-fullscreen.
            current_size
        }
    }

    fn is_windowed_fullscreen(&self) -> bool {
        self.is_windowed_fullscreen
    }

    fn is_pending_windowed_fullscreen(&self) -> bool {
        self.is_pending_windowed_fullscreen
    }

    fn request_windowed_fullscreen(&mut self, value: bool) {
        if self.is_pending_windowed_fullscreen == value {
            return;
        }

        self.is_pending_windowed_fullscreen = value;

        // Set the fullscreen state to match.
        //
        // When going from windowed to real fullscreen, we'll use request_size() which will set the
        // fullscreen state back.
        self.toplevel().with_pending_state(|state| {
            if value {
                state.states.set(xdg_toplevel::State::Fullscreen);
                state.states.unset(xdg_toplevel::State::Maximized);
            } else {
                state.states.unset(xdg_toplevel::State::Fullscreen);

                if self.is_pending_maximized {
                    state.states.set(xdg_toplevel::State::Maximized);
                }
            }
        });

        // Make sure we receive a commit later to update self.is_windowed_fullscreen.
        self.needs_configure = true;
    }

    fn is_child_of(&self, parent: &Self) -> bool {
        self.toplevel().parent().as_ref() == Some(parent.toplevel().wl_surface())
    }

    fn refresh(&self) {
        self.window.refresh();
    }

    fn rules(&self) -> &ResolvedWindowRules {
        &self.rules
    }

    fn take_animation_snapshot(&mut self) -> Option<LayoutElementRenderSnapshot> {
        self.animation_snapshot.take()
    }

    fn set_interactive_resize(&mut self, data: Option<InteractiveResizeData>) {
        self.toplevel().with_pending_state(|state| {
            if data.is_some() {
                state.states.set(xdg_toplevel::State::Resizing);
            } else {
                state.states.unset(xdg_toplevel::State::Resizing);
            }
        });

        if let Some(data) = data {
            self.interactive_resize = Some(InteractiveResize::Ongoing(data));
        } else {
            self.interactive_resize = match self.interactive_resize.take() {
                Some(InteractiveResize::Ongoing(data)) => {
                    Some(InteractiveResize::WaitingForLastConfigure(data))
                }
                x => x,
            }
        }
    }

    fn cancel_interactive_resize(&mut self) {
        self.set_interactive_resize(None);
        self.interactive_resize = None;
    }

    fn interactive_resize_data(&self) -> Option<InteractiveResizeData> {
        Some(self.interactive_resize.as_ref()?.data())
    }

    fn on_commit(&mut self, commit_serial: Serial) {
        if let Some(InteractiveResize::WaitingForLastCommit { serial, .. }) =
            &self.interactive_resize
        {
            if commit_serial.is_no_older_than(serial) {
                self.interactive_resize = None;
            }
        }

        if let Some(RequestSizeOnce::WaitingForCommit(serial)) = &self.request_size_once {
            if commit_serial.is_no_older_than(serial) {
                self.request_size_once = Some(RequestSizeOnce::UseWindowSize);
            }
        }

        // "Commit" our "acked" pending windowed fullscreen state.
        self.uncommitted_windowed_fullscreen
            .retain_mut(|(serial, value)| {
                if commit_serial.is_no_older_than(serial) {
                    self.is_windowed_fullscreen = *value;
                    false
                } else {
                    true
                }
            });

        // "Commit" our "acked" pending maximized state.
        self.uncommitted_maximized.retain_mut(|(serial, value)| {
            if commit_serial.is_no_older_than(serial) {
                self.is_maximized = *value;
                false
            } else {
                true
            }
        });
    }
}

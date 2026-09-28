use super::*;

impl State {
    pub(super) fn window_is_or_will_be_floating(&self, toplevel: &ToplevelSurface) -> bool {
        if let Some((mapped, _)) = self
            .swayward
            .layout
            .find_window_and_output(toplevel.wl_surface())
        {
            return mapped.is_floating();
        }

        let Some(unmapped) = self.swayward.unmapped_windows.get(toplevel.wl_surface()) else {
            return false;
        };
        match &unmapped.state {
            InitialConfigureState::Configured { rules, .. } => {
                rules.compute_open_floating(toplevel)
            }
            InitialConfigureState::NotConfigured { .. } => {
                let config = self.swayward.config.borrow();
                ResolvedWindowRules::compute(
                    &config.window_rules,
                    WindowRef::Unmapped(unmapped),
                    self.swayward.is_at_startup,
                )
                .compute_open_floating(toplevel)
            }
        }
    }

    pub fn send_initial_configure(&mut self, toplevel: &ToplevelSurface) {
        let _span = tracy_client::span!("State::send_initial_configure");

        let Some(unmapped) = self.swayward.unmapped_windows.get(toplevel.wl_surface()) else {
            error!("window must be present in unmapped_windows in send_initial_configure()");
            return;
        };

        let config = self.swayward.config.borrow();
        let rules = ResolvedWindowRules::compute(
            &config.window_rules,
            WindowRef::Unmapped(unmapped),
            self.swayward.is_at_startup,
        );
        let (title, app_id) = crate::utils::with_toplevel_role(toplevel, |role| {
            (role.title.clone(), role.app_id.clone())
        });
        let pid = crate::utils::get_credentials_for_surface(toplevel.wl_surface())
            .and_then(|credentials| u32::try_from(credentials.pid).ok());
        let assignment = self
            .swayward
            .runtime_window_rules
            .iter()
            .find_map(|rule| match rule {
                crate::swayward::RuntimeWindowRule::Assign(criteria, target)
                    if criteria.matches_unmapped(title.as_deref(), app_id.as_deref(), pid)
                        && match target {
                            AssignmentTarget::Output(name) => self
                                .swayward
                                .global_space
                                .outputs()
                                .any(|output| output_matches_name(output, name)),
                            _ => true,
                        } =>
                {
                    Some(target)
                }
                _ => None,
            });
        let configured_workspace_name = rules
            .open_on_workspace_number
            .as_deref()
            .and_then(|number| {
                self.swayward
                    .layout
                    .find_workspace_by_number(number)
                    .and_then(|(_, workspace)| workspace.sway_name())
            })
            .or_else(|| rules.open_on_workspace_number.clone())
            .or_else(|| rules.open_on_workspace.clone());
        let workspace_name = configured_workspace_name.or_else(|| {
            assignment.and_then(|target| match target {
                AssignmentTarget::Workspace(name) => Some(name.clone()),
                AssignmentTarget::WorkspaceNumber(number) => self
                    .swayward
                    .layout
                    .find_workspace_by_number(number)
                    .and_then(|(_, workspace)| workspace.sway_name())
                    .or_else(|| Some(number.clone())),
                AssignmentTarget::Output(_) => None,
            })
        });
        drop(config);
        if let Some(name) = workspace_name.as_deref() {
            self.swayward.layout.ensure_sway_workspace(name);
        }
        let config = self.swayward.config.borrow();
        let unmapped = self
            .swayward
            .unmapped_windows
            .get_mut(toplevel.wl_surface())
            .unwrap();
        let Unmapped { window, state, .. } = unmapped;

        let InitialConfigureState::NotConfigured {
            wants_fullscreen,
            wants_maximized,
        } = state
        else {
            error!("window must not be already configured in send_initial_configure()");
            return;
        };

        // Pick the target monitor. First, check if we had a workspace set in the window rules.
        let mon = workspace_name
            .as_deref()
            .and_then(|name| self.swayward.layout.monitor_for_workspace(name));

        // If not, check if we had an output set in the window rules.
        let output_assignment = assignment.and_then(|target| match target {
            AssignmentTarget::Output(output) => Some(output.as_str()),
            _ => None,
        });
        let mon = mon.or_else(|| {
            rules
                .open_on_output
                .as_deref()
                .or(output_assignment)
                .and_then(|name| {
                    self.swayward
                        .global_space
                        .outputs()
                        .find(|output| output_matches_name(output, name))
                })
                .and_then(|o| self.swayward.layout.monitor_for_output(o))
        });

        // If not, check if the window requested one for fullscreen.
        let mon = mon.or_else(|| {
            wants_fullscreen
                .as_ref()
                .and_then(|x| x.as_ref())
                // The monitor might not exist if the output was disconnected.
                .and_then(|o| self.swayward.layout.monitor_for_output(o))
        });

        // If not, check if this is a dialog with a parent, to place it next to the parent.
        let mon = mon.map(|mon| (mon, false)).or_else(|| {
            toplevel
                .parent()
                .and_then(|parent| self.swayward.layout.find_window_and_output(&parent))
                .and_then(|(_win, output)| output)
                .and_then(|o| self.swayward.layout.monitor_for_output(o))
                .map(|mon| (mon, true))
        });

        // If not, use the active monitor.
        let mon = mon.or_else(|| {
            self.swayward
                .layout
                .active_monitor_ref()
                .map(|mon| (mon, false))
        });

        // If we're following the parent, don't set the target output, so that when the window is
        // mapped, it fetches the possibly changed parent's output again, and shows up there.
        let output = mon
            .filter(|(_, parent)| !parent)
            .map(|(mon, _)| mon.output().clone());
        let mon = mon.map(|(mon, _)| mon);

        let mut width = None;
        let mut floating_width = None;
        let mut height = None;
        let mut floating_height = None;
        let is_full_width = rules.open_maximized.unwrap_or(false);
        let is_floating = rules.compute_open_floating(toplevel);

        // Tell the surface the preferred size and bounds for its likely output.
        let ws = workspace_name
            .as_deref()
            .and_then(|name| mon.map(|mon| mon.find_named_workspace(name)))
            .unwrap_or_else(|| {
                mon.map(|mon| mon.active_workspace_ref())
                    .or_else(|| self.swayward.layout.active_workspace())
            });

        let mut is_pending_maximized = false;
        if let Some(ws) = ws {
            // Set a fullscreen and maximized state based on window request and window rule.
            is_pending_maximized = (*wants_maximized && rules.open_maximized_to_edges.is_none())
                || rules.open_maximized_to_edges == Some(true);

            if (wants_fullscreen.is_some() && rules.open_fullscreen.is_none())
                || rules.open_fullscreen == Some(true)
            {
                toplevel.with_pending_state(|state| {
                    state.states.set(xdg_toplevel::State::Fullscreen);
                });
            } else if is_pending_maximized {
                toplevel.with_pending_state(|state| {
                    state.states.set(xdg_toplevel::State::Maximized);
                });
            }

            width = ws.resolve_default_width(rules.default_width, false);
            floating_width = ws.resolve_default_width(rules.default_width, true);
            height = ws.resolve_default_height(rules.default_height, false);
            floating_height = ws.resolve_default_height(rules.default_height, true);

            let configure_width = if is_floating {
                floating_width
            } else if is_full_width {
                Some(PresetSize::Proportion(1.))
            } else {
                width
            };
            let configure_height = if is_floating { floating_height } else { height };
            ws.configure_new_window(
                window,
                configure_width,
                configure_height,
                is_floating,
                &rules,
            );
        }

        // Set the tiled state for the initial configure.
        update_tiled_state(toplevel, config.prefer_no_csd, rules.tiled_state);

        // Set the configured settings.
        *state = InitialConfigureState::Configured {
            rules,
            width,
            height,
            floating_width,
            floating_height,
            is_full_width,
            output,
            workspace_name: ws.and_then(|workspace| workspace.sway_name()),
            is_pending_maximized,
        };

        trace!(surface = %toplevel.wl_surface().id(), "sending initial configure");
        toplevel.send_configure();
    }

    pub fn queue_initial_configure(&self, toplevel: ToplevelSurface) {
        // Send the initial configure in an idle, in case the client sent some more info after the
        // initial commit.
        self.swayward.event_loop.insert_idle(move |state| {
            if !toplevel.alive() {
                return;
            }

            if let Some(unmapped) = state.swayward.unmapped_windows.get(toplevel.wl_surface()) {
                if unmapped.needs_initial_configure() {
                    state.send_initial_configure(&toplevel);
                }
            }
        });
    }

    /// Should be called on `WlSurface::commit`
    pub fn popups_handle_commit(&mut self, surface: &WlSurface) {
        self.swayward.popups.commit(surface);

        if let Some(popup) = self.swayward.popups.find_popup(surface) {
            match popup {
                PopupKind::Xdg(ref popup) => {
                    if !popup.is_initial_configure_sent() {
                        if let Some(output) = self.output_for_popup(&PopupKind::Xdg(popup.clone()))
                        {
                            let scale = output.current_scale();
                            let transform = output.current_transform();
                            with_states(surface, |data| {
                                send_scale_transform(surface, data, scale, transform);
                            });
                        }
                        popup.send_configure().expect("initial configure failed");
                    }
                }
                // Input method popup can arbitrary change its geometry, so we need to unconstrain
                // it on commit.
                PopupKind::InputMethod(_) => {
                    self.unconstrain_popup(&popup);
                }
            }
        }
    }

    pub fn output_for_popup(&self, popup: &PopupKind) -> Option<&Output> {
        let root = find_popup_root_surface(popup).ok()?;
        self.swayward.output_for_root(&root)
    }

    pub fn unconstrain_popup(&self, popup: &PopupKind) {
        let _span = tracy_client::span!("Swayward::unconstrain_popup");

        // Popups with a NULL parent will get repositioned in their respective protocol handlers
        // (i.e. layer-shell).
        let Ok(root) = find_popup_root_surface(popup) else {
            return;
        };

        // Figure out if the root is a window or a layer surface.
        if let Some((mapped, _)) = self.swayward.layout.find_window_and_output(&root) {
            self.unconstrain_window_popup(popup, &mapped.window);
        } else if let Some((layer_surface, output)) = self.swayward.layout.outputs().find_map(|o| {
            let map = layer_map_for_output(o);
            let layer_surface = map.layer_for_surface(&root, WindowSurfaceType::TOPLEVEL)?;
            Some((layer_surface.clone(), o))
        }) {
            self.unconstrain_layer_shell_popup(popup, &layer_surface, output);
        }
    }

    fn unconstrain_window_popup(&self, popup: &PopupKind, window: &Window) {
        // The target geometry for the positioner should be relative to its parent's geometry, so
        // we will compute that here.
        let mut target = self.swayward.layout.popup_target_rect(window);
        target.loc -= get_popup_toplevel_coords(popup).to_f64();

        self.position_popup_within_rect(popup, target, true);
    }

    pub fn unconstrain_layer_shell_popup(
        &self,
        popup: &PopupKind,
        layer_surface: &LayerSurface,
        output: &Output,
    ) {
        let output_geo = self.swayward.global_space.output_geometry(output).unwrap();
        let map = layer_map_for_output(output);
        let Some(layer_geo) = map.layer_geometry(layer_surface) else {
            return;
        };

        // The target geometry for the positioner should be relative to its parent's geometry, so
        // we will compute that here.
        let mut target = Rectangle::from_size(output_geo.size);

        // Background and bottom layer popups render below the top and the overlay layer, so let's
        // put them into the non-exclusive zone.
        //
        // FIXME: ideally this should use the "top and overlay layer" non-exclusive zone, but
        // Smithay only computes the "all layers" non-exclusive zone atm.
        //
        // FIXME: related to the above, top layer popups should use the "overlay layer"
        // non-exclusive zone.
        if matches!(layer_surface.layer(), Layer::Background | Layer::Bottom) {
            target = map.non_exclusive_zone();
        }

        target.loc -= layer_geo.loc;
        target.loc -= get_popup_toplevel_coords(popup);

        // Don't add padding to layer-shell popups. It's not really needed, and it's unexpected.
        self.position_popup_within_rect(popup, target.to_f64(), false);
    }

    fn position_popup_within_rect(
        &self,
        popup: &PopupKind,
        target: Rectangle<f64, Logical>,
        padding: bool,
    ) {
        match popup {
            PopupKind::Xdg(popup) => {
                popup.with_pending_state(|state| {
                    state.geometry = if padding {
                        unconstrain_with_padding(state.positioner, target)
                    } else {
                        state
                            .positioner
                            .get_unconstrained_geometry(target.to_i32_round())
                    };
                });
            }
            PopupKind::InputMethod(popup) => {
                let text_input_rectangle = popup.text_input_rectangle();
                let mut bbox =
                    utils::bbox_from_surface_tree(popup.wl_surface(), text_input_rectangle.loc)
                        .to_f64();

                // Position bbox horizontally first.
                let overflow_x = (bbox.loc.x + bbox.size.w) - (target.loc.x + target.size.w);
                if overflow_x > 0. {
                    bbox.loc.x -= overflow_x;
                }

                // Ensure that the popup starts within the window.
                bbox.loc.x = f64::max(bbox.loc.x, target.loc.x);

                // Try to position IME popup below the text input rectangle.
                let mut below = bbox;
                below.loc.y += f64::from(text_input_rectangle.size.h);

                let mut above = bbox;
                above.loc.y -= bbox.size.h;

                if target.loc.y + target.size.h >= below.loc.y + below.size.h {
                    popup.set_location(below.loc.to_i32_round());
                } else {
                    popup.set_location(above.loc.to_i32_round());
                }
            }
        }
    }

    pub fn update_reactive_popups(&self, window: &Window) {
        let _span = tracy_client::span!("Swayward::update_reactive_popups");

        for (popup, _) in PopupManager::popups_for_surface(
            window.toplevel().expect("no x11 support").wl_surface(),
        ) {
            match &popup {
                xdg_popup @ PopupKind::Xdg(popup) => {
                    if popup.with_pending_state(|state| state.positioner.reactive) {
                        self.unconstrain_window_popup(xdg_popup, window);
                        if let Err(err) = popup.send_pending_configure() {
                            warn!("error re-configuring reactive popup: {err:?}");
                        }
                    }
                }
                PopupKind::InputMethod(_) => (),
            }
        }
    }

    pub(super) fn refresh_formatted_title(&mut self, toplevel: &ToplevelSurface) {
        if let Some((mapped, output)) = self
            .swayward
            .layout
            .find_window_and_output_mut(toplevel.wl_surface())
        {
            let output = output.cloned();
            let window = mapped.window.clone();
            self.swayward.layout.update_window(&window, None);
            if let Some(output) = output {
                self.swayward.queue_redraw(&output);
            }
        }
        self.ipc_refresh_layout();
    }

    pub fn update_window_rules(&mut self, toplevel: &ToplevelSurface) {
        let config = self.swayward.config.borrow();
        let window_rules = &config.window_rules;

        if let Some(unmapped) = self
            .swayward
            .unmapped_windows
            .get_mut(toplevel.wl_surface())
        {
            let new_rules = ResolvedWindowRules::compute(
                window_rules,
                WindowRef::Unmapped(unmapped),
                self.swayward.is_at_startup,
            );
            if let InitialConfigureState::Configured { rules, .. } = &mut unmapped.state {
                *rules = new_rules;
            }
        } else if let Some((mapped, output)) = self
            .swayward
            .layout
            .find_window_and_output_mut(toplevel.wl_surface())
        {
            if mapped.recompute_window_rules(window_rules, self.swayward.is_at_startup) {
                drop(config);
                let output = output.cloned();
                let window = mapped.window.clone();
                self.swayward.layout.update_window(&window, None);

                if let Some(output) = output {
                    self.swayward.queue_redraw(&output);
                }
            }
        }
    }
}

fn unconstrain_with_padding(
    positioner: PositionerState,
    target: Rectangle<f64, Logical>,
) -> Rectangle<i32, Logical> {
    // Try unconstraining with a small padding first which looks nicer, then if it doesn't fit try
    // unconstraining without padding.
    const PADDING: f64 = 8.;

    let mut padded = target;
    if PADDING * 2. < padded.size.w {
        padded.loc.x += PADDING;
        padded.size.w -= PADDING * 2.;
    }
    if PADDING * 2. < padded.size.h {
        padded.loc.y += PADDING;
        padded.size.h -= PADDING * 2.;
    }

    // No padding, so just unconstrain with the original target.
    if padded == target {
        return positioner.get_unconstrained_geometry(target.to_i32_round());
    }

    // Do not try to resize to fit the padded target rectangle.
    let mut no_resize = positioner;
    no_resize
        .constraint_adjustment
        .remove(ConstraintAdjustment::ResizeX);
    no_resize
        .constraint_adjustment
        .remove(ConstraintAdjustment::ResizeY);

    let geo = no_resize.get_unconstrained_geometry(padded.to_i32_round());
    if padded.contains_rect(geo.to_f64()) {
        return geo;
    }

    // Could not unconstrain into the padded target, so resort to the regular one.
    positioner.get_unconstrained_geometry(target.to_i32_round())
}

pub fn add_mapped_toplevel_pre_commit_hook(toplevel: &ToplevelSurface) -> HookId {
    add_pre_commit_hook::<State, _>(toplevel.wl_surface(), move |state, _dh, surface| {
        let _span = tracy_client::span!("mapped toplevel pre-commit");
        let span =
            trace_span!("toplevel pre-commit", surface = %surface.id(), serial = Empty).entered();

        let Some((mapped, output)) = state.swayward.layout.find_window_and_output_mut(surface)
        else {
            error!("pre-commit hook for mapped surfaces must be removed upon unmapping");
            return;
        };

        let (got_unmapped, dmabuf, commit_serial) = with_states(surface, |states| {
            let (got_unmapped, dmabuf) = {
                let mut guard = states.cached_state.get::<SurfaceAttributes>();
                match guard.pending().buffer.as_ref() {
                    Some(BufferAssignment::NewBuffer(buffer)) => {
                        let dmabuf = get_dmabuf(buffer).cloned().ok();
                        (false, dmabuf)
                    }
                    Some(BufferAssignment::Removed) => (true, None),
                    None => (false, None),
                }
            };

            let role = states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .unwrap()
                .lock()
                .unwrap();
            let serial = role.last_acked.as_ref().map(|c| c.serial);

            (got_unmapped, dmabuf, serial)
        });

        let mut transaction_for_dmabuf = None;
        let mut animate = false;
        if let Some(serial) = commit_serial {
            if !span.is_disabled() {
                span.record("serial", format!("{serial:?}"));
            }

            // trace!("taking pending transaction");
            if let Some(transaction) = mapped.take_pending_transaction(serial) {
                // Transaction can be already completed if it ran past the deadline.
                let disable = state.swayward.config.borrow().debug.disable_transactions;
                if !transaction.is_completed() && !disable {
                    // Register the deadline even if this is the last pending, since dmabuf
                    // rendering can still run over the deadline.
                    transaction.register_deadline_timer(&state.swayward.event_loop);

                    let is_last = transaction.is_last();

                    // If this is the last transaction, we don't need to add a separate
                    // notification, because the transaction will complete in our dmabuf blocker
                    // callback, which already calls blocker_cleared(), or by the end of this
                    // function, in which case there would be no blocker in the first place.
                    if !is_last {
                        // Waiting for some other surface; register a notification and add a
                        // transaction blocker.
                        if let Some(client) = surface.client() {
                            transaction.add_notification(
                                state.swayward.blocker_cleared_tx.clone(),
                                client.clone(),
                            );
                            add_blocker(surface, transaction.blocker());
                        }
                    }

                    // Delay dropping (and completing) the transaction until the dmabuf is ready.
                    // If there's no dmabuf, this will be dropped by the end of this pre-commit
                    // hook.
                    transaction_for_dmabuf = Some(transaction);
                }
            }

            animate = mapped.should_animate_commit(serial);
        } else if !got_unmapped {
            error!("commit on a mapped surface without a configured serial");
        };

        if let Some((blocker, source)) =
            dmabuf.and_then(|dmabuf| dmabuf.generate_blocker(Interest::READ).ok())
        {
            if let Some(client) = surface.client() {
                let res = state
                    .swayward
                    .event_loop
                    .insert_source(source, move |_, _, state| {
                        // This surface is now ready for the transaction.
                        drop(transaction_for_dmabuf.take());

                        let display_handle = state.swayward.display_handle.clone();
                        state
                            .client_compositor_state(&client)
                            .blocker_cleared(state, &display_handle);

                        Ok(())
                    });
                if res.is_ok() {
                    add_blocker(surface, blocker);
                    trace!("added dmabuf blocker");
                }
            }
        }

        let window = mapped.window.clone();
        if got_unmapped {
            let output = output.cloned();
            state.store_unmap_snapshot(&window, output.as_ref());
        } else {
            if animate {
                state.backend.with_primary_renderer(|renderer| {
                    mapped.store_animation_snapshot(renderer);
                });
            }

            // The toplevel remains mapped; clear any stored unmap snapshot.
            state.swayward.layout.clear_unmap_snapshot(&window);
        }
    })
}

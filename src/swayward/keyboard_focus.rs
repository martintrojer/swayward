use super::*;

impl State {
    pub fn update_keyboard_focus(&mut self) {
        if self.swayward.seat.get_keyboard().is_none() {
            return;
        }
        let focus = self.compute_keyboard_focus();
        if self.swayward.keyboard_focus != focus {
            self.apply_keyboard_focus_change(focus);
        }
    }

    fn compute_keyboard_focus(&mut self) -> KeyboardFocus {
        // Clean up on-demand layer surface focus if necessary.
        if let Some(surface) = &self.swayward.layer_shell_on_demand_focus {
            // Still alive and has on-demand interactivity.
            let mut good = surface.alive()
                && surface.cached_state().keyboard_interactivity
                    == wlr_layer::KeyboardInteractivity::OnDemand;

            if let Some(mapped) = self.swayward.mapped_layer_surfaces.get(surface) {
                // Check if it moved to the overview backdrop.
                if mapped.place_within_backdrop() {
                    good = false;
                }
            } else {
                // The layer surface is alive but it got unmapped.
                good = false;
            }

            if !good {
                self.swayward.layer_shell_on_demand_focus = None;
            }
        }

        // Compute the current focus.
        let focus = if self.swayward.exit_confirm_dialog.is_open() {
            KeyboardFocus::ExitConfirmDialog
        } else if self.swayward.is_locked() {
            KeyboardFocus::LockScreen {
                surface: self.swayward.lock_surface_focus(),
            }
        } else if self.swayward.screenshot_ui.is_open() {
            KeyboardFocus::ScreenshotUi
        } else if self.swayward.window_mru_ui.is_open() {
            KeyboardFocus::Mru
        } else if let Some(output) = self.swayward.layout.active_output() {
            let mon = self.swayward.layout.monitor_for_output(output).unwrap();
            let layers = layer_map_for_output(output);

            // Explicitly check for layer-shell popup grabs here, our keyboard focus will stay on
            // the root layer surface while it has grabs.
            let layer_grab = self.swayward.popup_grab.as_ref().and_then(|g| {
                layers
                    .layer_for_surface(&g.root, WindowSurfaceType::TOPLEVEL)
                    .and_then(|l| l.can_receive_keyboard_focus().then(|| (&g.root, l.layer())))
            });
            let grab_on_layer = |layer: Layer| {
                layer_grab
                    .and_then(move |(s, l)| if l == layer { Some(s.clone()) } else { None })
                    .map(|surface| KeyboardFocus::LayerShell { surface })
            };

            let layout_focus = || {
                self.swayward
                    .layout
                    .focus()
                    .map(|win| win.toplevel().wl_surface().clone())
                    .map(|surface| KeyboardFocus::Layout {
                        surface: Some(surface),
                    })
            };

            let excl_focus_on_layer = |layer| {
                layers.layers_on(layer).find_map(|surface| {
                    if surface.cached_state().keyboard_interactivity
                        != wlr_layer::KeyboardInteractivity::Exclusive
                    {
                        return None;
                    }

                    let mapped = self.swayward.mapped_layer_surfaces.get(surface)?;
                    if mapped.place_within_backdrop() {
                        return None;
                    }

                    let surface = surface.wl_surface().clone();
                    Some(KeyboardFocus::LayerShell { surface })
                })
            };

            let on_d_focus_on_layer = |layer| {
                layers.layers_on(layer).find_map(|surface| {
                    let is_on_demand_surface =
                        Some(surface) == self.swayward.layer_shell_on_demand_focus.as_ref();
                    is_on_demand_surface
                        .then(|| surface.wl_surface().clone())
                        .map(|surface| KeyboardFocus::LayerShell { surface })
                })
            };

            // Prefer exclusive focus on a layer, then check on-demand focus.
            let focus_on_layer =
                |layer| excl_focus_on_layer(layer).or_else(|| on_d_focus_on_layer(layer));

            let is_overview_open = self.swayward.layout.is_overview_open();

            let mut surface = grab_on_layer(Layer::Overlay);
            // FIXME: we shouldn't prioritize the top layer grabs over regular overlay input or a
            // fullscreen layout window. This will need tracking in grab() to avoid handing it out
            // in the first place. Or a better way to structure this code.
            surface = surface.or_else(|| grab_on_layer(Layer::Top));

            if !is_overview_open {
                surface = surface.or_else(|| grab_on_layer(Layer::Bottom));
                surface = surface.or_else(|| grab_on_layer(Layer::Background));
            }

            surface = surface.or_else(|| focus_on_layer(Layer::Overlay));

            if mon.render_above_top_layer() {
                surface = surface.or_else(layout_focus);
                surface = surface.or_else(|| focus_on_layer(Layer::Top));
                surface = surface.or_else(|| focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| focus_on_layer(Layer::Background));
            } else {
                surface = surface.or_else(|| focus_on_layer(Layer::Top));

                if is_overview_open {
                    surface = Some(surface.unwrap_or(KeyboardFocus::Overview));
                }

                surface = surface.or_else(|| on_d_focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| on_d_focus_on_layer(Layer::Background));
                surface = surface.or_else(layout_focus);

                // Bottom and background layers can only receive exclusive focus when there are no
                // layout windows.
                surface = surface.or_else(|| excl_focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| excl_focus_on_layer(Layer::Background));
            }

            surface.unwrap_or(KeyboardFocus::Layout { surface: None })
        } else {
            KeyboardFocus::Layout { surface: None }
        };
        focus
    }

    fn apply_keyboard_focus_change(&mut self, focus: KeyboardFocus) {
        let keyboard = self.swayward.seat.get_keyboard().unwrap();

        trace!(
            "keyboard focus changed from {:?} to {:?}",
            self.swayward.keyboard_focus,
            focus
        );

        let workspace_for_surface = |surface: &WlSurface| {
            self.swayward.layout.windows().find_map(|(_, mapped)| {
                mapped
                    .is_wl_surface(surface)
                    .then(|| self.swayward.layout.window_workspace_id(&mapped.window))
                    .flatten()
            })
        };
        let last_workspace = self
            .swayward
            .keyboard_focus
            .surface()
            .and_then(workspace_for_surface);
        let new_workspace = focus.surface().and_then(workspace_for_surface);

        // Tell the windows their new focus state for window rule purposes.
        if let KeyboardFocus::Layout {
            surface: Some(surface),
        } = &self.swayward.keyboard_focus
        {
            if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface) {
                mapped.set_is_focused(false);
            }
        }
        if let KeyboardFocus::Layout {
            surface: Some(surface),
        } = &focus
        {
            // Sway clears urgency when seat focus reaches the view
            // (`sway/sway/input/seat.c:260-315`; `sway/sway/tree/view.c`).
            self.swayward.focus_clears_urgency(
                surface,
                last_workspace.is_some() && last_workspace != new_workspace,
            );
            if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface) {
                mapped.set_is_focused(true);

                // Structural focus fallback follows sway's seat-wide stack immediately. The
                // recent-windows UI still uses the debounce below before committing its order.
                let stamp = get_monotonic_time();
                mapped.set_focus_timestamp(stamp);

                let debounce = self.swayward.config.borrow().recent_windows.debounce_ms;
                let debounce = Duration::from_millis(u64::from(debounce));

                if !debounce.is_zero() {
                    let timer = Timer::from_duration(debounce);

                    let focus_token = self
                        .swayward
                        .event_loop
                        .insert_source(timer, move |_, _, state| {
                            state.swayward.mru_apply_keyboard_commit();
                            TimeoutAction::Drop
                        })
                        .unwrap();
                    if let Some(PendingMruCommit { token, .. }) =
                        self.swayward.pending_mru_commit.replace(PendingMruCommit {
                            id: mapped.id(),
                            token: focus_token,
                            stamp,
                        })
                    {
                        self.swayward.event_loop.remove(token);
                    }
                }
            }
        }

        if let Some(grab) = self.swayward.popup_grab.as_mut() {
            if grab.has_keyboard_grab && Some(&grab.root) != focus.surface() {
                trace!(
                    "grab root {:?} is not the new focus {:?}, ungrabbing",
                    grab.root,
                    focus
                );

                grab.grab.ungrab(PopupUngrabStrategy::All);
                keyboard.unset_grab(self);
                self.swayward.seat.get_pointer().unwrap().unset_grab(
                    self,
                    SERIAL_COUNTER.next_serial(),
                    InputTime::now(),
                );
                self.swayward.popup_grab = None;
            }
        }

        if self.swayward.config.borrow().input.keyboard.track_layout == TrackLayout::Window {
            let current_layout = keyboard.with_xkb_state(self, |context| {
                let xkb = context.xkb().lock().unwrap();
                xkb.active_layout()
            });

            let mut new_layout = current_layout;
            // Store the currently active layout for the surface.
            if let Some(current_focus) = self.swayward.keyboard_focus.surface() {
                with_states(current_focus, |data| {
                    let cell = data
                        .data_map
                        .get_or_insert::<Cell<KeyboardLayout>, _>(Cell::default);
                    cell.set(current_layout);
                });
            }

            if let Some(focus) = focus.surface() {
                new_layout = with_states(focus, |data| {
                    let cell = data.data_map.get_or_insert::<Cell<KeyboardLayout>, _>(|| {
                        // The default layout is effectively the first layout in the
                        // keymap, so use it for new windows.
                        Cell::new(KeyboardLayout::default())
                    });
                    cell.get()
                });
            }
            if new_layout != current_layout && focus.surface().is_some() {
                keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
                keyboard.with_xkb_state(self, |mut context| {
                    context.set_layout(new_layout);
                });
            }
        }

        self.swayward.keyboard_focus.clone_from(&focus);
        keyboard.set_focus(self, focus.into_surface(), SERIAL_COUNTER.next_serial());

        // FIXME: can be more granular.
        self.swayward.queue_redraw_all();
    }
}

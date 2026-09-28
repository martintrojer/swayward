use super::*;

impl State {
    /// Loads the xkb keymap from a file config setting.
    pub(super) fn set_xkb_file(&mut self, xkb_file: String) -> anyhow::Result<()> {
        let xkb_file = PathBuf::from(xkb_file);
        let xkb_file = expand_home(&xkb_file)
            .context("failed to expand ~")?
            .unwrap_or(xkb_file);

        let keymap = std::fs::read_to_string(xkb_file).context("failed to read xkb_file")?;

        let keyboard = self.swayward.seat.get_keyboard().unwrap();
        let num_lock = keyboard.modifier_state().num_lock;

        keyboard
            .set_keymap_from_string(self, keymap)
            .context("failed to set keymap")?;

        // Restore num lock to its previous value.
        let mut mods_state = keyboard.modifier_state();
        if mods_state.num_lock != num_lock {
            mods_state.num_lock = num_lock;
            keyboard.set_modifier_state(mods_state);
        }

        Ok(())
    }

    pub(super) fn load_xkb_file(&mut self) {
        let xkb_file = self
            .swayward
            .config
            .borrow()
            .input
            .keyboard
            .xkb
            .file
            .clone();
        if let Some(xkb_file) = xkb_file {
            if let Err(err) = self.set_xkb_file(xkb_file) {
                warn!("error loading xkb_file: {err:?}");
            }
        }
    }

    pub fn set_xkb_config(&mut self, xkb: XkbConfig) {
        let keyboard = self.swayward.seat.get_keyboard().unwrap();
        let num_lock = keyboard.modifier_state().num_lock;
        if let Err(err) = keyboard.set_xkb_config(self, xkb) {
            warn!("error updating xkb config: {err:?}");
            return;
        }

        // Restore num lock to its previous value.
        let mut mods_state = keyboard.modifier_state();
        if mods_state.num_lock != num_lock {
            mods_state.num_lock = num_lock;
            keyboard.set_modifier_state(mods_state);
        }
    }

    pub fn reload_config(&mut self, config: Result<Config, ()>) {
        let _span = tracy_client::span!("State::reload_config");

        let mut config = match config {
            Ok(config) => config,
            Err(()) => {
                self.swayward.config_error_notification.show();
                self.swayward.queue_redraw_all();

                #[cfg(feature = "dbus")]
                self.swayward.a11y_announce_config_error();

                return;
            }
        };

        self.swayward.config_error_notification.hide();

        // Find & orphan removed named workspaces.
        let mut removed_workspaces: Vec<String> = vec![];
        for ws in &self.swayward.config.borrow().workspaces {
            if !config.workspaces.iter().any(|w| w.name == ws.name) {
                removed_workspaces.push(ws.name.0.clone());
            }
        }
        for name in removed_workspaces {
            self.swayward.layout.unname_workspace(&name);
        }

        self.swayward.layout.update_config(&config);
        for mapped in self.swayward.mapped_layer_surfaces.values_mut() {
            mapped.update_config(&config);
        }

        // Create native named workspaces eagerly. Sway output assignments take effect when the
        // workspace is first selected, matching sway/sway/tree/workspace.c:153-182.
        for ws_config in &config.workspaces {
            if ws_config.sway_output_assignment.is_none() {
                self.swayward.layout.ensure_named_workspace(ws_config);
            }
        }

        let rate = 1.0 / config.animations.slowdown.max(0.001);
        self.swayward.clock.set_rate(rate);
        self.swayward
            .clock
            .set_complete_instantly(config.animations.off);

        *CHILD_ENV.write().unwrap() = mem::take(&mut config.environment);

        let mut reload_xkb = None;
        let mut libinput_config_changed = false;
        let mut output_config_changed = false;
        let mut preserved_output_config = None;
        let mut window_rules_changed = false;
        let mut layer_rules_changed = false;
        let mut shaders_changed = false;
        let mut cursor_inactivity_timeout_changed = false;
        let mut recent_windows_changed = false;
        let mut xwls_changed = false;
        let mut old_config = self.swayward.config.borrow_mut();

        // Reload the cursor.
        if config.cursor != old_config.cursor {
            self.swayward
                .cursor_manager
                .reload(&config.cursor.xcursor_theme, config.cursor.xcursor_size);
            self.swayward.cursor_texture_cache.clear();
        }

        // We need &mut self to reload the xkb config, so just store it here.
        if config.input.keyboard.xkb != old_config.input.keyboard.xkb {
            reload_xkb = Some(config.input.keyboard.xkb.clone());
        }

        // Reload the repeat info.
        if config.input.keyboard.repeat_rate != old_config.input.keyboard.repeat_rate
            || config.input.keyboard.repeat_delay != old_config.input.keyboard.repeat_delay
        {
            let keyboard = self.swayward.seat.get_keyboard().unwrap();
            keyboard.change_repeat_info(
                config.input.keyboard.repeat_rate.into(),
                config.input.keyboard.repeat_delay.into(),
            );
        }

        if config.input.touchpad != old_config.input.touchpad
            || config.input.mouse != old_config.input.mouse
            || config.input.trackball != old_config.input.trackball
            || config.input.trackpoint != old_config.input.trackpoint
            || config.input.tablet != old_config.input.tablet
            || config.input.touch != old_config.input.touch
        {
            libinput_config_changed = true;
        }

        let ignored_nodes_changed =
            config.debug.ignored_drm_devices != old_config.debug.ignored_drm_devices;

        if config.outputs != self.swayward.config_file_output_config {
            output_config_changed = true;
            self.swayward
                .config_file_output_config
                .clone_from(&config.outputs);
        } else {
            // Output config did not change from the last disk load, so we need to preserve the
            // transient changes.
            preserved_output_config = Some(mem::take(&mut old_config.outputs));
        }

        let binds_changed = config.binds != old_config.binds;
        let new_mod_key = self.backend.mod_key(&config);
        if new_mod_key != self.backend.mod_key(&old_config) || binds_changed {
            self.swayward
                .hotkey_overlay
                .on_hotkey_config_updated(new_mod_key);
            self.swayward.mods_with_mouse_binds = mods_with_mouse_binds(new_mod_key, &config.binds);
            self.swayward.mods_with_wheel_binds = mods_with_wheel_binds(new_mod_key, &config.binds);
            self.swayward.mods_with_tablet_stylus_binds =
                mods_with_tablet_stylus_binds(new_mod_key, &config.binds);
            self.swayward.mods_with_finger_scroll_binds =
                mods_with_finger_scroll_binds(new_mod_key, &config.binds);
        }

        if config.window_rules != old_config.window_rules {
            window_rules_changed = true;
        }

        if config.layer_rules != old_config.layer_rules {
            layer_rules_changed = true;
        }

        if config.animations.window_resize.custom_shader
            != old_config.animations.window_resize.custom_shader
        {
            let src = config.animations.window_resize.custom_shader.as_deref();
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_resize_program(renderer, src);
            });
            shaders_changed = true;
        }

        if config.animations.window_close.custom_shader
            != old_config.animations.window_close.custom_shader
        {
            let src = config.animations.window_close.custom_shader.as_deref();
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_close_program(renderer, src);
            });
            shaders_changed = true;
        }

        if config.animations.window_open.custom_shader
            != old_config.animations.window_open.custom_shader
        {
            let src = config.animations.window_open.custom_shader.as_deref();
            self.backend.with_primary_renderer(|renderer| {
                shaders::set_custom_open_program(renderer, src);
            });
            shaders_changed = true;
        }

        if config.cursor.hide_after_inactive_ms != old_config.cursor.hide_after_inactive_ms {
            cursor_inactivity_timeout_changed = true;
        }

        if config.debug.keep_laptop_panel_on_when_lid_is_closed
            != old_config.debug.keep_laptop_panel_on_when_lid_is_closed
        {
            output_config_changed = true;
        }

        if config.debug.ignored_drm_devices != old_config.debug.ignored_drm_devices {
            output_config_changed = true;
        }

        // FIXME: move backdrop rendering into layout::Monitor, then this will become unnecessary.
        if config.overview.backdrop_color != old_config.overview.backdrop_color {
            output_config_changed = true;
        }
        if config.layout.background_color != old_config.layout.background_color {
            output_config_changed = true;
        }

        if config.recent_windows != old_config.recent_windows {
            recent_windows_changed = true;
        }

        if config.xwayland_satellite != old_config.xwayland_satellite {
            xwls_changed = true;
        }

        *old_config = config;
        // Runtime-added criteria are part of sway's active config and vanish
        // when reload replaces it. A failed reload returns above and keeps both
        // the rules and their per-view execution history.
        let runtime_for_window = mem::take(&mut self.swayward.runtime_for_window);
        self.swayward.for_window.retain(|(raw, command, _)| {
            !runtime_for_window.contains(&(raw.clone(), command.clone()))
        });
        self.swayward.runtime_window_rules.clear();
        self.swayward.executed_for_window.clear();

        if let Some(outputs) = preserved_output_config {
            old_config.outputs = outputs;
        }

        // Release the borrow.
        drop(old_config);
        self.swayward.output_power.clear();
        for output in self
            .swayward
            .global_space
            .outputs()
            .cloned()
            .collect::<Vec<_>>()
        {
            self.backend.set_output_power(&output, true);
            self.swayward.queue_redraw(&output);
        }
        // Sway frees the symbol table on reload and rebuilds it from the file
        // (`sway/sway/config.c:111-115`), so a runtime `set` does not outlive a
        // reload. swayward has no KDL variables to rebuild, so the table is
        // simply emptied.
        self.swayward.sway_variables.clear();
        // Sway frees and rebuilds each mode's switch binding list on reload.
        self.swayward.runtime_switch_bindings.clear();
        let mode_changed = self.swayward.binding_mode != "default";
        self.swayward.binding_mode = "default".into();
        self.ipc_refresh_config();
        if mode_changed {
            if let Some(server) = &self.swayward.ipc_server {
                server.send_event(swayward_ipc::legacy::Event::BindingModeChanged {
                    mode: "default".into(),
                    pango_markup: false,
                });
            }
        }
        // Held release bindings own their action so a reload cannot invalidate them.

        // Now with a &mut self we can reload the xkb config.
        if let Some(mut xkb) = reload_xkb {
            let mut set_xkb_config = true;

            // It's fine to .take() the xkb file, as this is a
            // clone and the file field is not used in the XkbConfig.
            if let Some(xkb_file) = xkb.file.take() {
                if let Err(err) = self.set_xkb_file(xkb_file) {
                    warn!("error reloading xkb_file: {err:?}");
                } else {
                    // We successfully set xkb file so we don't need to fallback to XkbConfig.
                    set_xkb_config = false;
                }
            }

            if set_xkb_config {
                // If xkb is unset in the niri config, use settings from locale1.
                if xkb == Xkb::default() {
                    trace!("using xkb from locale1");
                    xkb = self.swayward.xkb_from_locale1.clone().unwrap_or_default();
                }

                self.set_xkb_config(xkb.to_xkb_config());
            }

            self.ipc_keyboard_layouts_changed();
        }

        if libinput_config_changed {
            let config = self.swayward.config.borrow();
            for mut device in self.swayward.devices.iter().cloned() {
                apply_libinput_settings(&config.input, &mut device);
            }
        }

        if ignored_nodes_changed {
            self.backend.update_ignored_nodes_config(&mut self.swayward);
        }

        if output_config_changed {
            self.reload_output_config();
        }

        if window_rules_changed {
            self.swayward.recompute_window_rules();
        }

        if layer_rules_changed {
            self.swayward.recompute_layer_rules();
        }

        if shaders_changed {
            self.swayward.update_shaders();
        }

        if cursor_inactivity_timeout_changed {
            // Force reset due to timeout change.
            self.swayward.pointer_inactivity_timer_got_reset = false;
            self.swayward.reset_pointer_inactivity_timer();
        }

        if binds_changed {
            self.swayward.window_mru_ui.update_binds();
        }

        if recent_windows_changed {
            self.swayward.window_mru_ui.update_config();
        }

        if xwls_changed {
            // If xwl-s was previously working and is now off, we don't try to kill it or stop
            // watching the sockets, for simplicity's sake.
            let was_working = self.swayward.satellite.is_some();

            // Try to start, or restart in case the user corrected the path or something.
            xwayland::satellite::setup(self);

            let config = self.swayward.config.borrow();
            let display_name = (!config.xwayland_satellite.off)
                .then_some(self.swayward.satellite.as_ref())
                .flatten()
                .map(|satellite| satellite.display_name().to_owned());

            if let Some(name) = &display_name {
                if !was_working {
                    info!("listening on X11 socket: {name}");
                }
            }

            // This won't change the systemd environment, but oh well.
            *CHILD_DISPLAY.write().unwrap() = display_name;
        }

        self.swayward.queue_redraw_all();
        if let Some(server) = &self.swayward.ipc_server {
            server.send_event(swayward_ipc::legacy::Event::WorkspaceReloaded);
        }
    }

    pub fn reload_output_config(&mut self) {
        let mut resized_outputs = vec![];
        let mut recolored_outputs = vec![];

        for output in self.swayward.global_space.outputs() {
            let name = output.user_data().get::<OutputName>().unwrap();
            let full_config = self.swayward.config.borrow_mut();
            let config = full_config.outputs.find(name);

            let scale = config
                .and_then(|c| c.scale)
                .map(|s| s.0)
                .unwrap_or_else(|| {
                    let size_mm = output.physical_properties().size;
                    let resolution = output.current_mode().unwrap().size;
                    guess_monitor_scale(size_mm, resolution)
                });
            let scale = closest_representable_scale(scale.clamp(0.1, 10.));

            let mut transform = panel_orientation(output)
                + config
                    .map(|c| ipc_transform_to_smithay(c.transform))
                    .unwrap_or(Transform::Normal);
            // FIXME: fix winit damage on other transforms.
            if name.connector == "winit" {
                transform = Transform::Flipped180;
            }

            if output.current_scale().fractional_scale() != scale
                || output.current_transform() != transform
            {
                output.change_current_state(
                    None,
                    Some(transform),
                    Some(output::Scale::Fractional(scale)),
                    None,
                );
                self.swayward.ipc_outputs_changed = true;
                resized_outputs.push(output.clone());
            }

            let mut backdrop_color = config
                .and_then(|c| c.backdrop_color)
                .unwrap_or(full_config.overview.backdrop_color)
                .to_array_unpremul();
            backdrop_color[3] = 1.;
            let backdrop_color = Color32F::from(backdrop_color);

            if let Some(state) = self.swayward.output_state.get_mut(output) {
                if state.backdrop_buffer.color() != backdrop_color {
                    state.backdrop_buffer.set_color(backdrop_color);
                    recolored_outputs.push(output.clone());
                }
            }

            for mon in self.swayward.layout.monitors_mut() {
                if mon.output() != output {
                    continue;
                }

                let mut layout_config = config.and_then(|c| c.layout.clone());
                // Support the deprecated non-layout background-color key.
                if let Some(layout) = &mut layout_config {
                    if layout.background_color.is_none() {
                        layout.background_color = config.and_then(|c| c.background_color);
                    }
                }

                if mon.update_layout_config(layout_config) {
                    // Also redraw these; if anything, the background color could've changed.
                    recolored_outputs.push(output.clone());
                }
                break;
            }
        }

        for output in resized_outputs {
            self.swayward.output_resized(&output);
        }

        for output in recolored_outputs {
            self.swayward.queue_redraw(&output);
        }

        self.backend.on_output_config_changed(&mut self.swayward);

        self.swayward.reposition_outputs(None);

        if let Some(touch) = self.swayward.seat.get_touch() {
            touch.cancel(self);
        }

        let config = self.swayward.config.borrow().outputs.clone();
        self.swayward
            .output_management_state
            .on_config_changed(config);
    }

    pub fn modify_output_config<F>(&mut self, name: &str, fun: F)
    where
        F: FnOnce(&mut swayward_config::Output),
    {
        // Try hard to find the output config section corresponding to the output set by the
        // user. Since if we add a new section and some existing section also matches the
        // output, then our new section won't do anything.
        let temp;
        let match_name = if let Some(output) = self.swayward.output_by_name_match(name) {
            output.user_data().get::<OutputName>().unwrap()
        } else if let Some(output_name) = self
            .backend
            .tty_checked()
            .and_then(|tty| tty.disconnected_connector_name_by_name_match(name))
        {
            temp = output_name;
            &temp
        } else {
            // Even if name is "make model serial", matching will work fine this way.
            temp = OutputName {
                connector: name.to_owned(),
                make: None,
                model: None,
                serial: None,
            };
            &temp
        };

        let mut config = self.swayward.config.borrow_mut();
        let config = if let Some(config) = config.outputs.find_mut(match_name) {
            config
        } else {
            config.outputs.0.push(swayward_config::Output {
                // Save name as set by the user.
                name: String::from(name),
                ..Default::default()
            });
            config.outputs.0.last_mut().unwrap()
        };

        fun(config);
    }

    pub fn apply_transient_output_config(
        &mut self,
        name: &str,
        actions: &[swayward_ipc::OutputAction],
    ) {
        self.modify_output_config(name, move |config| {
            for action in actions.iter().cloned() {
                match action {
                    swayward_ipc::OutputAction::Off => config.off = true,
                    swayward_ipc::OutputAction::On => config.off = false,
                    swayward_ipc::OutputAction::Power { .. } => {}
                    swayward_ipc::OutputAction::Mode { mode } => {
                        config.mode = match mode {
                            swayward_ipc::ModeToSet::Automatic => None,
                            swayward_ipc::ModeToSet::Specific(mode) => {
                                Some(swayward_config::output::Mode {
                                    custom: false,
                                    mode,
                                })
                            }
                        };
                        config.modeline = None;
                    }
                    swayward_ipc::OutputAction::CustomMode { mode } => {
                        config.mode = Some(swayward_config::output::Mode { custom: true, mode });
                        config.modeline = None;
                    }
                    swayward_ipc::OutputAction::Modeline {
                        clock,
                        hdisplay,
                        hsync_start,
                        hsync_end,
                        htotal,
                        vdisplay,
                        vsync_start,
                        vsync_end,
                        vtotal,
                        hsync_polarity,
                        vsync_polarity,
                    } => {
                        // Do not reset config.mode to None since it's used as a fallback.
                        config.modeline = Some(swayward_config::output::Modeline {
                            clock,
                            hdisplay,
                            hsync_start,
                            hsync_end,
                            htotal,
                            vdisplay,
                            vsync_start,
                            vsync_end,
                            vtotal,
                            hsync_polarity,
                            vsync_polarity,
                        })
                    }
                    swayward_ipc::OutputAction::Scale { scale } => {
                        config.scale = match scale {
                            swayward_ipc::ScaleToSet::Automatic => None,
                            swayward_ipc::ScaleToSet::Specific(scale) => Some(FloatOrInt(scale)),
                        }
                    }
                    swayward_ipc::OutputAction::Transform { transform } => {
                        config.transform = transform
                    }
                    swayward_ipc::OutputAction::Position { position } => {
                        config.position = match position {
                            swayward_ipc::PositionToSet::Automatic => None,
                            swayward_ipc::PositionToSet::Specific(position) => {
                                Some(swayward_config::Position {
                                    x: position.x,
                                    y: position.y,
                                })
                            }
                        }
                    }
                    swayward_ipc::OutputAction::Vrr { vrr } => {
                        config.variable_refresh_rate = if vrr.vrr {
                            Some(swayward_config::Vrr {
                                on_demand: vrr.on_demand,
                            })
                        } else {
                            None
                        }
                    }
                    swayward_ipc::OutputAction::MaxBpc { max_bpc } => {
                        config.max_bpc = Some(MaxBpc(max_bpc))
                    }
                }
            }
        });

        self.reload_output_config();
    }

    pub fn refresh_ipc_outputs(&mut self) {
        if !self.swayward.ipc_outputs_changed {
            return;
        }
        self.swayward.ipc_outputs_changed = false;

        let _span = tracy_client::span!("State::refresh_ipc_outputs");

        for ipc_output in self.backend.ipc_outputs().lock().unwrap().values_mut() {
            let logical = self
                .swayward
                .global_space
                .outputs()
                .find(|output| output.name() == ipc_output.name)
                .map(logical_output);
            ipc_output.logical = logical;
        }

        #[cfg(feature = "dbus")]
        self.swayward.on_ipc_outputs_changed();

        let new_config = self.backend.ipc_outputs().lock().unwrap().clone();
        self.swayward
            .output_management_state
            .notify_changes(new_config);
        self.swayward.ipc_output_changed();
    }
}

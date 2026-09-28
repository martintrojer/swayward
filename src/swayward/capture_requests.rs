use super::*;

impl State {
    pub fn open_screenshot_ui(&mut self, show_pointer: bool, path: Option<String>) {
        if self.swayward.is_locked() || self.swayward.screenshot_ui.is_open() {
            return;
        }

        let default_output = self
            .swayward
            .output_under_cursor()
            .or_else(|| self.swayward.layout.active_output().cloned());
        let Some(default_output) = default_output else {
            return;
        };

        self.swayward.update_render_elements(None);

        let Some(screenshots) = self.backend.with_primary_renderer(|renderer| {
            self.swayward.capture_screenshots(renderer).collect()
        }) else {
            return;
        };

        // Now that we captured the screenshots, clear grabs like drag-and-drop, etc.
        let time = InputTime::now();
        self.swayward.seat.get_pointer().unwrap().unset_grab(
            self,
            SERIAL_COUNTER.next_serial(),
            time,
        );
        if let Some(touch) = self.swayward.seat.get_touch() {
            touch.unset_grab(self);
        }

        for tool in self.swayward.seat.tablet_seat().get_tools().into_values() {
            tool.unset_grab(self, SERIAL_COUNTER.next_serial(), time);
        }

        self.backend.with_primary_renderer(|renderer| {
            self.swayward.screenshot_ui.open(
                renderer,
                screenshots,
                default_output,
                show_pointer,
                path,
            )
        });

        self.swayward
            .cursor_manager
            .set_cursor_image(CursorImageStatus::Named(CursorIcon::Crosshair));
        self.swayward.queue_redraw_all();
    }

    pub fn handle_pick_color(
        &mut self,
        tx: async_channel::Sender<Option<swayward_ipc::PickedColor>>,
    ) {
        let pointer = self.swayward.seat.get_pointer().unwrap();
        let start_data = PointerGrabStartData {
            focus: None,
            button: 0,
            location: pointer.current_location(),
        };
        let grab = PickColorGrab::new(start_data);
        pointer.set_grab(self, grab, SERIAL_COUNTER.next_serial(), Focus::Clear);
        self.swayward.pick_color = Some(tx);
        self.swayward
            .cursor_manager
            .set_cursor_image(CursorImageStatus::Named(CursorIcon::Crosshair));
        self.swayward.queue_redraw_all();
    }

    pub fn confirm_screenshot(&mut self, write_to_disk: bool) {
        let ScreenshotUi::Open { path, .. } = &mut self.swayward.screenshot_ui else {
            return;
        };
        let path = path.take();

        self.backend.with_primary_renderer(|renderer| {
            match self.swayward.screenshot_ui.capture(renderer) {
                Ok((size, pixels)) => {
                    if let Err(err) =
                        self.swayward
                            .save_screenshot(size, pixels, write_to_disk, path)
                    {
                        warn!("error saving screenshot: {err:?}");
                    }
                }
                Err(err) => {
                    warn!("error capturing screenshot: {err:?}");
                }
            }
        });

        self.swayward.screenshot_ui.close();
        self.swayward
            .cursor_manager
            .set_cursor_image(CursorImageStatus::default_named());
        self.swayward.queue_redraw_all();
    }

    pub fn store_unmap_snapshot(&mut self, window: &Window, output: Option<&Output>) {
        // The unmapping tile may have an xray background, in which case we will render xray
        // elements, so they need to be updated.
        self.swayward.update_xray_render_elements(output);

        self.backend.with_primary_renderer(|renderer| {
            if let Some(output) = output {
                let mut ctx = RenderCtx {
                    target: RenderTarget::Output,
                    renderer,
                    xray: None,
                };

                self.swayward.fill_xray_elements(ctx.r(), output);

                // If any background layer has block_out_from, also fill the Screencast xray
                // buffer so the unmap snapshot can render a buffer with blocked-out background.
                //
                // This will be used in Tile::render_snapshot().
                let has_blocked_out = self.swayward.has_blocked_out_background_layers(output);
                if has_blocked_out {
                    let screencast_ctx = RenderCtx {
                        target: RenderTarget::Screencast,
                        ..ctx.r()
                    };
                    self.swayward.fill_xray_elements(screencast_ctx, output);
                }

                let state = self.swayward.output_state.get_mut(output).unwrap();
                self.swayward.layout.store_unmap_snapshot(
                    renderer,
                    Some(&mut state.xray),
                    has_blocked_out,
                    window,
                );

                self.swayward.clear_xray_elements(output);
            } else {
                self.swayward
                    .layout
                    .store_unmap_snapshot(renderer, None, false, window);
            }
        });
    }

    #[cfg(not(feature = "xdp-gnome-screencast"))]
    pub fn set_dynamic_cast_target(&mut self, _target: CastTarget) {}

    #[cfg(feature = "dbus")]
    pub fn on_screen_shot_msg(
        &mut self,
        to_screenshot: &async_channel::Sender<NiriToScreenshot>,
        msg: ScreenshotToNiri,
    ) {
        match msg {
            ScreenshotToNiri::TakeScreenshot { include_cursor } => {
                self.handle_take_screenshot(to_screenshot, include_cursor);
            }
            ScreenshotToNiri::PickColor(tx) => {
                self.handle_pick_color(tx);
            }
        }
    }

    #[cfg(feature = "dbus")]
    pub(super) fn handle_take_screenshot(
        &mut self,
        to_screenshot: &async_channel::Sender<NiriToScreenshot>,
        include_cursor: bool,
    ) {
        let _span = tracy_client::span!("TakeScreenshot");

        let rv = self.backend.with_primary_renderer(|renderer| {
            let on_done = {
                let to_screenshot = to_screenshot.clone();
                move |path| {
                    let msg = NiriToScreenshot::ScreenshotResult(Some(path));
                    if let Err(err) = to_screenshot.send_blocking(msg) {
                        warn!("error sending path to screenshot: {err:?}");
                    }
                }
            };

            let res = self
                .swayward
                .screenshot_all_outputs(renderer, include_cursor, on_done);

            if let Err(err) = res {
                warn!("error taking a screenshot: {err:?}");

                let msg = NiriToScreenshot::ScreenshotResult(None);
                if let Err(err) = to_screenshot.send_blocking(msg) {
                    warn!("error sending None to screenshot: {err:?}");
                }
            }
        });

        if rv.is_none() {
            let msg = NiriToScreenshot::ScreenshotResult(None);
            if let Err(err) = to_screenshot.send_blocking(msg) {
                warn!("error sending None to screenshot: {err:?}");
            }
        }
    }

    #[cfg(feature = "dbus")]
    pub fn on_introspect_msg(
        &mut self,
        to_introspect: &async_channel::Sender<NiriToIntrospect>,
        msg: IntrospectToNiri,
    ) {
        use crate::utils::with_toplevel_role;

        let IntrospectToNiri::GetWindows = msg;
        let _span = tracy_client::span!("GetWindows");

        let mut windows = HashMap::new();

        #[cfg(feature = "xdp-gnome-screencast")]
        windows.insert(
            self.swayward.casting.dynamic_cast_id_for_portal.get(),
            gnome_shell_introspect::WindowProperties {
                title: String::from("swayward Dynamic Cast Target"),
                app_id: String::from("rs.bxt.swayward.desktop"),
            },
        );

        self.swayward.layout.with_windows(|mapped, _, _, _| {
            let id = mapped.id().get();
            let props = with_toplevel_role(mapped.toplevel(), |role| {
                gnome_shell_introspect::WindowProperties {
                    title: role.title.clone().unwrap_or_default(),
                    app_id: role
                        .app_id
                        .as_ref()
                        // We don't do proper .desktop file tracking (it's quite involved), and
                        // Wayland windows can set any app id they want. However, this seems to
                        // work well enough in practice.
                        .map(|app_id| format!("{app_id}.desktop"))
                        .unwrap_or_default(),
                }
            });

            windows.insert(id, props);
        });

        let msg = NiriToIntrospect::Windows(windows);
        if let Err(err) = to_introspect.send_blocking(msg) {
            warn!("error sending windows to introspect: {err:?}");
        }
    }

    #[cfg(feature = "dbus")]
    pub fn on_login1_msg(&mut self, msg: Login1ToNiri) {
        let Login1ToNiri::LidClosedChanged(is_closed) = msg;

        trace!("login1 lid {}", if is_closed { "closed" } else { "opened" });
        self.set_lid_closed(is_closed);
    }

    #[cfg(feature = "dbus")]
    pub fn on_locale1_msg(&mut self, msg: Locale1ToNiri) {
        let Locale1ToNiri::XkbChanged(xkb) = msg;

        trace!("locale1 xkb settings changed: {xkb:?}");
        let xkb = self.swayward.xkb_from_locale1.insert(xkb);

        {
            let config = self.swayward.config.borrow();
            if config.input.keyboard.xkb != Xkb::default() {
                trace!("ignoring locale1 xkb change because swayward config has xkb settings");
                return;
            }
        }

        let xkb = xkb.clone();
        self.set_xkb_config(xkb.to_xkb_config());
        self.ipc_keyboard_layouts_changed();
    }
}

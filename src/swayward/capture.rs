use super::*;

impl Swayward {
    pub fn render_for_screencopy_with_damage(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
    ) {
        let _span = tracy_client::span!("Swayward::render_for_screencopy_with_damage");

        let mut screencopy_state = mem::take(&mut self.screencopy_state);

        screencopy_state.with_queues_mut(|queue| {
            let (damage_tracker, screencopy) = queue.split();
            if let Some(screencopy) = screencopy {
                if screencopy.output() == output {
                    let ctx = RenderCtx {
                        renderer,
                        target: RenderTarget::ScreenCapture,
                        xray: None,
                    };
                    let offset = screencopy.region_loc().upscale(-1);
                    let mut elements = Vec::new();
                    self.render(ctx, output, screencopy.overlay_cursor(), &mut |elem| {
                        let elem =
                            RelocateRenderElement::from_element(elem, offset, Relocate::Relative);
                        elements.push(elem);
                    });

                    let (damages, states) = Self::damage_screencopy_internal(
                        output,
                        &elements,
                        damage_tracker,
                        screencopy,
                    );
                    if let Some(damages) = damages {
                        // Convert from Physical coordinates back to Buffer coordinates.
                        let transform = output.current_transform();
                        let physical_size = transform.transform_size(screencopy.buffer_size());
                        let damages = damages.iter().map(|dmg| {
                            dmg.to_logical(1).to_buffer(
                                1,
                                transform.invert(),
                                &physical_size.to_logical(1),
                            )
                        });

                        screencopy.damage(damages);

                        let render_result = Self::render_for_screencopy_internal(
                            renderer,
                            damage_tracker,
                            &elements,
                            states,
                            screencopy,
                        );
                        match render_result {
                            Ok(sync) => {
                                queue.pop().submit_after_sync(false, sync, &self.event_loop);
                            }
                            Err(err) => {
                                // Recreate damage tracker to report full damage next check.
                                *damage_tracker =
                                    OutputDamageTracker::new((0, 0), 1.0, Transform::Normal);
                                queue.pop();
                                warn!("error rendering for screencopy: {err:?}");
                            }
                        }
                    } else {
                        trace!("no damage found, waiting till next redraw");
                    }
                };
            }
        });

        self.screencopy_state = screencopy_state;
    }

    pub fn render_for_screencopy_without_damage(
        &mut self,
        renderer: &mut GlesRenderer,
        manager: &ZwlrScreencopyManagerV1,
        screencopy: Screencopy,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Swayward::render_for_screencopy");

        let output = screencopy.output();
        ensure!(
            self.output_state.contains_key(output),
            "screencopy output missing"
        );

        self.update_render_elements(Some(output));

        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
        };
        let offset = screencopy.region_loc().upscale(-1);
        let mut elements = Vec::new();
        self.render(ctx, output, screencopy.overlay_cursor(), &mut |elem| {
            let elem = RelocateRenderElement::from_element(elem, offset, Relocate::Relative);
            elements.push(elem);
        });

        let Some(damage_tracker) = self.screencopy_state.damage_tracker(manager) else {
            error!("screencopy queue must not be deleted as long as frames exist");
            bail!("screencopy queue missing");
        };

        let (_damages, states) =
            Self::damage_screencopy_internal(output, &elements, damage_tracker, &screencopy);
        let res = Self::render_for_screencopy_internal(
            renderer,
            damage_tracker,
            &elements,
            states,
            &screencopy,
        );
        let res = res.map(|sync| screencopy.submit_after_sync(false, sync, &self.event_loop));

        if res.is_err() {
            // Recreate damage tracker to report full damage next check.
            *damage_tracker = OutputDamageTracker::new((0, 0), 1.0, Transform::Normal);
        }

        res
    }

    pub(super) fn damage_screencopy_internal<'a>(
        output: &Output,
        elements: &[impl Element],
        damage_tracker: &'a mut OutputDamageTracker,
        screencopy: &Screencopy,
    ) -> (
        Option<&'a Vec<Rectangle<i32, Physical>>>,
        RenderElementStates,
    ) {
        let OutputModeSource::Static {
            size: last_size,
            scale: last_scale,
            transform: last_transform,
        } = damage_tracker.mode().clone()
        else {
            unreachable!("damage tracker must have static mode");
        };

        let size = screencopy.buffer_size();
        let scale: Scale<f64> = output.current_scale().fractional_scale().into();
        let transform = output.current_transform();

        if size != last_size || scale != last_scale || transform != last_transform {
            *damage_tracker = OutputDamageTracker::new(size, scale, transform);
        }

        // Just checked damage tracker has static mode
        damage_tracker.damage_output(1, elements).unwrap()
    }

    pub(super) fn render_for_screencopy_internal(
        renderer: &mut GlesRenderer,
        damage_tracker: &mut OutputDamageTracker,
        elements: &[impl RenderElement<GlesRenderer>],
        states: RenderElementStates,
        screencopy: &Screencopy,
    ) -> ScreencopyRenderResult {
        let sync = match screencopy.buffer() {
            ScreencopyBuffer::Dmabuf(dmabuf) => {
                let sync =
                    render_to_dmabuf(renderer, damage_tracker, dmabuf.clone(), elements, states)
                        .context("error rendering to screencopy dmabuf")?;
                Some(sync)
            }
            ScreencopyBuffer::Shm(wl_buffer) => {
                render_to_shm(renderer, damage_tracker, wl_buffer, elements, states)
                    .context("error rendering to screencopy shm buffer")?;
                None
            }
        };

        Ok(sync)
    }

    #[cfg(not(feature = "xdp-gnome-screencast"))]
    pub fn stop_casts_for_target(&mut self, _target: CastTarget) {}

    #[cfg(not(feature = "xdp-gnome-screencast"))]
    pub fn stop_cast(&mut self, _session_id: crate::utils::CastSessionId) {}

    pub fn debug_toggle_damage(&mut self) {
        self.debug_draw_damage = !self.debug_draw_damage;

        if self.debug_draw_damage {
            for (output, state) in &mut self.output_state {
                state.debug_damage_tracker = OutputDamageTracker::from_output(output);
            }
        }

        self.queue_redraw_all();
    }

    pub fn capture_screenshots<'a>(
        &'a self,
        renderer: &'a mut GlesRenderer,
    ) -> impl Iterator<Item = (Output, [OutputScreenshot; 3])> + 'a {
        self.global_space.outputs().cloned().filter_map(|output| {
            let size = output.current_mode().unwrap().size;
            let transform = output.current_transform();
            let size = transform.transform_size(size);

            let scale = Scale::from(output.current_scale().fractional_scale());
            let targets = [
                RenderTarget::Output,
                RenderTarget::Screencast,
                RenderTarget::ScreenCapture,
            ];
            let screenshot = targets.map(|target| {
                let ctx = RenderCtx {
                    renderer,
                    target,
                    xray: None,
                };
                let elements = self.render_to_vec(ctx, &output, false);
                let elements = elements.iter().rev();

                let res = render_to_texture(
                    renderer,
                    size,
                    scale,
                    Transform::Normal,
                    Fourcc::Abgr8888,
                    elements,
                );
                if let Err(err) = &res {
                    warn!("error rendering output {}: {err:?}", output.name());
                }
                let res_output = res.ok();

                let mut pointer = Vec::new();

                // We check the pointer visibility for Disabled (and not .is_visible()) in order to
                // show the pointer even when it's hidden through cursor {} options. The user can
                // then toggle it in the screenshot UI as needed.
                if self.pointer_visibility != PointerVisibility::Disabled {
                    self.render_pointer(renderer, &output, &mut |elem| pointer.push(elem));
                }

                let res_pointer = if pointer.is_empty() {
                    None
                } else {
                    let res = render_to_encompassing_texture(
                        renderer,
                        scale,
                        Transform::Normal,
                        Fourcc::Abgr8888,
                        &pointer,
                    );
                    if let Err(err) = &res {
                        warn!("error rendering pointer for {}: {err:?}", output.name());
                    }
                    res.ok()
                };

                res_output.map(|(texture, _)| {
                    OutputScreenshot::from_textures(
                        renderer,
                        scale,
                        texture,
                        res_pointer.map(|(texture, _, geo)| (texture, geo)),
                    )
                })
            });

            if screenshot.iter().any(|res| res.is_none()) {
                return None;
            }

            let screenshot = screenshot.map(|res| res.unwrap());
            Some((output, screenshot))
        })
    }

    pub fn screenshot(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        write_to_disk: bool,
        include_pointer: bool,
        path: Option<String>,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Swayward::screenshot");

        self.update_render_elements(Some(output));

        let size = output.current_mode().unwrap().size;
        let transform = output.current_transform();
        let size = transform.transform_size(size);

        let scale = Scale::from(output.current_scale().fractional_scale());
        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
        };
        let elements = self.render_to_vec(ctx, output, include_pointer);
        let elements = elements.iter().rev();
        let pixels = render_to_vec(
            renderer,
            size,
            scale,
            Transform::Normal,
            Fourcc::Abgr8888,
            elements,
        )?;

        self.save_screenshot(size, pixels, write_to_disk, path)
            .context("error saving screenshot")
    }

    pub fn screenshot_window(
        &self,
        renderer: &mut GlesRenderer,
        output: &Output,
        mapped: &Mapped,
        write_to_disk: bool,
        show_pointer: bool,
        path: Option<String>,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Swayward::screenshot_window");

        let scale = Scale::from(output.current_scale().fractional_scale());
        let rule_alpha =
            if mapped.sizing_mode().is_fullscreen() || mapped.is_ignoring_opacity_window_rule() {
                1.
            } else {
                mapped.rules().opacity.unwrap_or(1.).clamp(0., 1.)
            };
        let alpha = rule_alpha * mapped.command_opacity();

        let mut elements: Vec<WindowScreenshotRenderElement<GlesRenderer>> = Vec::new();

        // Add pointer if requested and it's over this window.
        if show_pointer {
            if let Some((_, win_pos)) = self.pointer_pos_for_window_cast(mapped) {
                // Pointer elements are at output-local physical coords.
                // Relocate by -win_pos to make them window-relative.
                let pos = win_pos.to_physical_precise_round(scale).upscale(-1);
                self.render_pointer(renderer, output, &mut |elem| {
                    let elem = RelocateRenderElement::from_element(elem, pos, Relocate::Relative);
                    elements.push(elem.into());
                });
            }
        }
        let pointer_count = elements.len();

        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
        };
        mapped.render(
            ctx,
            mapped.window.geometry().loc.to_f64(),
            scale,
            alpha,
            XrayPos::default(),
            &mut |elem| elements.push(elem.into()),
        );

        // The pointer is not included in encompassing_geo because we don't want it to expand the
        // screenshot size.
        let geo = encompassing_geo(scale, elements.iter().skip(pointer_count));
        let elements = elements.iter().rev().map(|elem| {
            RelocateRenderElement::from_element(elem, geo.loc.upscale(-1), Relocate::Relative)
        });
        let pixels = render_to_vec(
            renderer,
            geo.size,
            scale,
            Transform::Normal,
            Fourcc::Abgr8888,
            elements,
        )?;

        self.save_screenshot(geo.size, pixels, write_to_disk, path)
            .context("error saving screenshot")
    }

    pub fn save_screenshot(
        &self,
        size: Size<i32, Physical>,
        pixels: Vec<u8>,
        write_to_disk: bool,
        path_arg: Option<String>,
    ) -> anyhow::Result<()> {
        let path = write_to_disk
            .then(|| {
                // When given an explicit path, don't try to strftime it or create parents.
                path_arg.map(|p| (PathBuf::from(p), false)).or_else(|| {
                    match make_screenshot_path(&self.config.borrow()) {
                        Ok(path) => path.map(|p| (p, true)),
                        Err(err) => {
                            warn!("error making screenshot path: {err:?}");
                            None
                        }
                    }
                })
            })
            .flatten();

        // Prepare to set the encoded image as our clipboard selection. This must be done from the
        // main thread.
        let (tx, rx) = calloop::channel::sync_channel::<Arc<[u8]>>(1);
        self.event_loop
            .insert_source(rx, move |event, _, state| match event {
                calloop::channel::Event::Msg(buf) => {
                    set_data_device_selection(
                        &state.swayward.display_handle,
                        &state.swayward.seat,
                        vec![String::from("image/png")],
                        buf.clone(),
                    );
                }
                calloop::channel::Event::Closed => (),
            })
            .unwrap();

        // Encode and save the image in a thread as it's slow.
        thread::spawn(move || {
            let mut buf = vec![];

            let w = std::io::Cursor::new(&mut buf);
            if let Err(err) = write_png_rgba8(w, size.w as u32, size.h as u32, &pixels) {
                warn!("error encoding screenshot image: {err:?}");
                return;
            }

            let buf: Arc<[u8]> = Arc::from(buf.into_boxed_slice());
            let _ = tx.send(buf.clone());

            let mut image_path = None;

            if let Some((path, create_parent)) = path {
                debug!("saving screenshot to {path:?}");

                if create_parent {
                    if let Some(parent) = path.parent() {
                        // Relative paths with one component, i.e. "test.png", have Some("") parent.
                        if !parent.as_os_str().is_empty() {
                            if let Err(err) = std::fs::create_dir_all(parent) {
                                if err.kind() != std::io::ErrorKind::AlreadyExists {
                                    warn!("error creating screenshot directory: {err:?}");
                                }
                            }
                        }
                    }
                }

                match std::fs::write(&path, buf) {
                    Ok(()) => image_path = Some(path),
                    Err(err) => {
                        warn!("error saving screenshot image: {err:?}");
                    }
                }
            } else {
                debug!("not saving screenshot to disk");
            }

            #[cfg(feature = "dbus")]
            if let Err(err) = crate::utils::show_screenshot_notification(image_path.as_deref()) {
                warn!("error showing screenshot notification: {err:?}");
            }
            #[cfg(not(feature = "dbus"))]
            let _ = image_path;
        });

        Ok(())
    }

    #[cfg(feature = "dbus")]
    pub fn screenshot_all_outputs(
        &mut self,
        renderer: &mut GlesRenderer,
        include_pointer: bool,
        on_done: impl FnOnce(PathBuf) + Send + 'static,
    ) -> anyhow::Result<()> {
        let _span = tracy_client::span!("Swayward::screenshot_all_outputs");

        self.update_render_elements(None);

        let outputs: Vec<_> = self.global_space.outputs().cloned().collect();

        // FIXME: support multiple outputs, needs fixing multi-scale handling and cropping.
        anyhow::ensure!(outputs.len() == 1);

        let output = outputs.into_iter().next().unwrap();
        let geom = self.global_space.output_geometry(&output).unwrap();

        let output_scale = output.current_scale().integer_scale();
        let geom = geom.to_physical(output_scale);

        let size = geom.size;
        let transform = output.current_transform();
        let size = transform.transform_size(size);

        let ctx = RenderCtx {
            renderer,
            target: RenderTarget::ScreenCapture,
            xray: None,
        };
        let elements = self.render_to_vec(ctx, &output, include_pointer);
        let elements = elements.iter().rev();
        let pixels = render_to_vec(
            renderer,
            size,
            Scale::from(f64::from(output_scale)),
            Transform::Normal,
            Fourcc::Abgr8888,
            elements,
        )?;

        let path = make_screenshot_path(&self.config.borrow())
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let mut path = env::temp_dir();
                path.push("screenshot.png");
                path
            });
        debug!("saving screenshot to {path:?}");

        thread::spawn(move || {
            let file = match std::fs::File::create(&path) {
                Ok(file) => file,
                Err(err) => {
                    warn!("error creating file: {err:?}");
                    return;
                }
            };

            let w = std::io::BufWriter::new(file);
            if let Err(err) = write_png_rgba8(w, size.w as u32, size.h as u32, &pixels) {
                warn!("error encoding screenshot image: {err:?}");
                return;
            }

            on_done(path);
        });

        Ok(())
    }
}

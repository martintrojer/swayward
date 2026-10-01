impl State {
    fn popup_positioner(&self) -> XdgPositioner {
        let positioner = self
            .xdg_wm_base
            .as_ref()
            .unwrap()
            .create_positioner(&self.qh, ());
        positioner.set_size(100, 100);
        positioner.set_anchor_rect(0, 0, 1, 1);
        positioner
    }

    pub fn create_popup(
        &mut self,
        parent: Option<&XdgSurface>,
        offset: Option<(i32, i32)>,
    ) -> &mut Popup {
        let surface = self
            .compositor
            .as_ref()
            .unwrap()
            .create_surface(&self.qh, ());
        let xdg_surface =
            self.xdg_wm_base
                .as_ref()
                .unwrap()
                .get_xdg_surface(&surface, &self.qh, ());
        let positioner = self.popup_positioner();
        if let Some((x, y)) = offset {
            positioner.set_offset(x, y);
            positioner.set_constraint_adjustment(
                ConstraintAdjustment::SlideX | ConstraintAdjustment::SlideY,
            );
        }
        let xdg_popup = xdg_surface.get_popup(parent, &positioner, &self.qh, ());
        positioner.destroy();
        self.popups.push(Popup {
            surface,
            xdg_surface,
            xdg_popup,
            configures_received: Vec::new(),
            repositioned: Vec::new(),
        });
        self.popups.last_mut().unwrap()
    }

    pub fn reposition_popup(&self, popup: &XdgPopup, token: u32) {
        self.reposition_popup_at(popup, token, None);
    }

    pub fn reposition_popup_at(&self, popup: &XdgPopup, token: u32, offset: Option<(i32, i32)>) {
        let positioner = self.popup_positioner();
        if let Some((x, y)) = offset {
            positioner.set_offset(x, y);
            positioner.set_constraint_adjustment(
                ConstraintAdjustment::SlideX | ConstraintAdjustment::SlideY,
            );
        }
        popup.reposition(&positioner, token);
        positioner.destroy();
    }

    pub fn create_lock_surface(
        &mut self,
        lock: &ExtSessionLockV1,
        output: &WlOutput,
    ) -> &mut SessionLockSurface {
        let surface = self
            .compositor
            .as_ref()
            .unwrap()
            .create_surface(&self.qh, ());
        let lock_surface = lock.get_lock_surface(&surface, output, &self.qh, ());
        let viewport = self
            .viewporter
            .as_ref()
            .unwrap()
            .get_viewport(&surface, &self.qh, ());
        self.lock_surfaces.push(SessionLockSurface {
            surface,
            lock_surface,
            viewport,
            configure: None,
        });
        self.lock_surfaces.last_mut().unwrap()
    }

    pub fn create_subsurface(&mut self, parent: &WlSurface) -> &mut (WlSurface, WlSubsurface) {
        let surface = self
            .compositor
            .as_ref()
            .unwrap()
            .create_surface(&self.qh, ());
        let subsurface =
            self.subcompositor
                .as_ref()
                .unwrap()
                .get_subsurface(&surface, parent, &self.qh, ());
        self.subsurfaces.push((surface, subsurface));
        self.subsurfaces.last_mut().unwrap()
    }

    pub fn create_window(&mut self) -> &mut Window {
        let compositor = self.compositor.as_ref().unwrap();
        let xdg_wm_base = self.xdg_wm_base.as_ref().unwrap();
        let viewporter = self.viewporter.as_ref().unwrap();

        let surface = compositor.create_surface(&self.qh, ());
        let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, &self.qh, ());
        let xdg_toplevel = xdg_surface.get_toplevel(&self.qh, ());
        let viewport = viewporter.get_viewport(&surface, &self.qh, ());

        let window = Window {
            qh: self.qh.clone(),
            spbm: self.spbm.clone().unwrap(),

            surface,
            xdg_surface,
            xdg_toplevel,
            xdg_decoration: None,
            decoration_modes: Vec::new(),
            viewport,
            pending_configure: Configure::default(),
            configures_received: Vec::new(),
            close_requested: false,

            configures_looked_at: 0,
            last_acked_configure: None,
        };

        self.windows.push(window);
        self.windows.last_mut().unwrap()
    }

    pub fn window(&mut self, surface: &WlSurface) -> &mut Window {
        self.windows
            .iter_mut()
            .find(|w| w.surface == *surface)
            .unwrap()
    }

    pub fn create_layer(
        &mut self,
        output: Option<&WlOutput>,
        layer: zwlr_layer_shell_v1::Layer,
        namespace: String,
    ) -> &mut LayerSurface {
        let compositor = self.compositor.as_ref().unwrap();
        let layer_shell = self.layer_shell.as_ref().unwrap();
        let viewporter = self.viewporter.as_ref().unwrap();

        let surface = compositor.create_surface(&self.qh, ());
        let layer_surface =
            layer_shell.get_layer_surface(&surface, output, layer, namespace, &self.qh, ());
        let viewport = viewporter.get_viewport(&surface, &self.qh, ());

        let layer_surface = LayerSurface {
            qh: self.qh.clone(),
            spbm: self.spbm.clone().unwrap(),

            surface,
            layer_surface,
            viewport,
            configures_received: Vec::new(),
            close_requested: false,

            configures_looked_at: 0,
        };

        self.layers.push(layer_surface);
        self.layers.last_mut().unwrap()
    }

    pub fn layer(&mut self, surface: &WlSurface) -> &mut LayerSurface {
        self.layers
            .iter_mut()
            .find(|w| w.surface == *surface)
            .unwrap()
    }
}

impl SessionLockSurface {
    pub fn ack_and_map(&self, qh: &QueueHandle<State>, spbm: &WpSinglePixelBufferManagerV1) {
        let (serial, width, height) = self.configure.unwrap();
        self.lock_surface.ack_configure(serial);
        self.viewport.set_destination(width as i32, height as i32);
        let buffer = spbm.create_u32_rgba_buffer(0, 0, 0, 0, qh, ());
        self.surface.attach(Some(&buffer), 0, 0);
        self.surface.commit();
    }
}

impl Window {
    pub fn commit(&self) {
        self.surface.commit();
    }

    pub fn ack_last(&mut self) {
        let serial = self.configures_received.last().unwrap().0;
        self.ack_configure(serial);
    }

    pub fn ack_configure(&mut self, serial: u32) {
        self.xdg_surface.ack_configure(serial);
        self.last_acked_configure = Some(serial);
    }

    pub fn ack_last_and_commit(&mut self) {
        self.ack_last();
        self.commit();
    }

    pub fn attach_new_buffer(&self) {
        let buffer = self.spbm.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, ());
        self.surface.attach(Some(&buffer), 0, 0);
    }

    pub fn attach_null(&self) {
        self.surface.attach(None, 0, 0);
    }

    /// Attaches an opaque single-pixel buffer of the given colour.
    pub fn attach_color_buffer(&self, r: u8, g: u8, b: u8) {
        let scale = |c: u8| u32::from(c) * (u32::MAX / 255);
        let buffer =
            self.spbm
                .create_u32_rgba_buffer(scale(r), scale(g), scale(b), u32::MAX, &self.qh, ());
        self.surface.attach(Some(&buffer), 0, 0);
    }

    pub fn set_size(&self, w: u16, h: u16) {
        self.viewport.set_destination(i32::from(w), i32::from(h));
    }

    pub fn set_min_size(&self, width: i32, height: i32) {
        self.xdg_toplevel.set_min_size(width, height);
    }

    pub fn set_max_size(&self, width: i32, height: i32) {
        self.xdg_toplevel.set_max_size(width, height);
    }

    pub fn set_fullscreen(&self, output: Option<&WlOutput>) {
        self.xdg_toplevel.set_fullscreen(output);
    }

    pub fn unset_fullscreen(&self) {
        self.xdg_toplevel.unset_fullscreen();
    }

    pub fn set_maximized(&self) {
        self.xdg_toplevel.set_maximized();
    }

    pub fn unset_maximized(&self) {
        self.xdg_toplevel.unset_maximized();
    }

    pub fn set_parent(&self, parent: Option<&XdgToplevel>) {
        self.xdg_toplevel.set_parent(parent);
    }

    pub fn set_title(&self, title: &str) {
        self.xdg_toplevel.set_title(title.to_owned());
    }

    pub fn destroy_role(&self) {
        self.xdg_toplevel.destroy();
        self.xdg_surface.destroy();
    }

    pub fn recent_configures(&mut self) -> impl Iterator<Item = &Configure> {
        let start = self.configures_looked_at;
        self.configures_looked_at = self.configures_received.len();
        self.configures_received[start..].iter().map(|(_, c)| c)
    }

    pub fn format_recent_configures(&mut self) -> String {
        let mut buf = String::new();
        for configure in self.recent_configures() {
            if !buf.is_empty() {
                buf.push('\n');
            }
            write!(buf, "{configure}").unwrap();
        }
        buf
    }
}

impl LayerSurface {
    pub fn commit(&self) {
        self.surface.commit();
    }

    pub fn ack_last(&self) {
        let serial = self.configures_received.last().unwrap().0;
        self.layer_surface.ack_configure(serial);
    }

    pub fn ack_last_and_commit(&self) {
        self.ack_last();
        self.commit();
    }

    pub fn set_configure_props(&self, props: LayerConfigureProps) {
        let LayerConfigureProps {
            size,
            anchor,
            exclusive_zone,
            margin,
            kb_interactivity,
            layer,
            exclusive_edge,
        } = props;

        if let Some(x) = size {
            self.layer_surface.set_size(x.0, x.1);
        }
        if let Some(x) = anchor {
            self.layer_surface.set_anchor(x);
        }
        if let Some(x) = exclusive_zone {
            self.layer_surface.set_exclusive_zone(x);
        }
        if let Some(x) = margin {
            self.layer_surface
                .set_margin(x.top, x.right, x.bottom, x.left);
        }
        if let Some(x) = kb_interactivity {
            self.layer_surface.set_keyboard_interactivity(x);
        }
        if let Some(x) = layer {
            self.layer_surface.set_layer(x);
        }
        if let Some(x) = exclusive_edge {
            self.layer_surface.set_exclusive_edge(x);
        }
    }

    pub fn attach_new_buffer(&self) {
        let buffer = self.spbm.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, ());
        self.surface.attach(Some(&buffer), 0, 0);
    }

    pub fn attach_null(&self) {
        self.surface.attach(None, 0, 0);
    }

    pub fn set_size(&self, w: u16, h: u16) {
        self.viewport.set_destination(i32::from(w), i32::from(h));
    }

    pub fn recent_configures(&mut self) -> impl Iterator<Item = &LayerConfigure> {
        let start = self.configures_looked_at;
        self.configures_looked_at = self.configures_received.len();
        self.configures_received[start..].iter().map(|(_, c)| c)
    }

    pub fn format_recent_configures(&mut self) -> String {
        let mut buf = String::new();
        for configure in self.recent_configures() {
            if !buf.is_empty() {
                buf.push('\n');
            }
            write!(buf, "{configure}").unwrap();
        }
        buf
    }
}

impl Dispatch<WlCallback, Arc<SyncData>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlCallback,
        event: <WlCallback as wayland_client::Proxy>::Event,
        data: &Arc<SyncData>,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_callback::Event::Done { .. } => data.done.store(true, Ordering::Relaxed),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == WlCompositor::interface().name {
                    let version = min(version, WlCompositor::interface().version);
                    state.compositor = Some(registry.bind(name, version, qh, ()));
                } else if interface == WlSubcompositor::interface().name {
                    let version = min(version, WlSubcompositor::interface().version);
                    state.subcompositor = Some(registry.bind(name, version, qh, ()));
                } else if interface == WlSeat::interface().name {
                    let version = min(version, WlSeat::interface().version);
                    state.seat = Some(registry.bind(name, version, qh, ()));
                } else if interface == XdgWmBase::interface().name {
                    let version = min(version, XdgWmBase::interface().version);
                    state.xdg_wm_base = Some(registry.bind(name, version, qh, ()));
                    state.xdg_wm_base_version = Some(version);
                } else if interface == XdgActivationV1::interface().name {
                    let version = min(version, XdgActivationV1::interface().version);
                    state.xdg_activation = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZxdgDecorationManagerV1::interface().name {
                    let version = min(version, ZxdgDecorationManagerV1::interface().version);
                    state.xdg_decoration_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == XdgToplevelTagManagerV1::interface().name {
                    let version = min(version, XdgToplevelTagManagerV1::interface().version);
                    state.xdg_toplevel_tag_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwpKeyboardShortcutsInhibitManagerV1::interface().name {
                    let version = min(
                        version,
                        ZwpKeyboardShortcutsInhibitManagerV1::interface().version,
                    );
                    state.keyboard_shortcuts_inhibit_manager =
                        Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrLayerShellV1::interface().name {
                    let version = min(version, ZwlrLayerShellV1::interface().version);
                    state.layer_shell = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrForeignToplevelManagerV1::interface().name {
                    let version = min(version, ZwlrForeignToplevelManagerV1::interface().version);
                    state.foreign_toplevel_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ExtWorkspaceManagerV1::interface().name {
                    let version = min(version, ExtWorkspaceManagerV1::interface().version);
                    state.ext_workspace_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ExtSessionLockManagerV1::interface().name {
                    let version = min(version, ExtSessionLockManagerV1::interface().version);
                    state.session_lock_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrOutputManagerV1::interface().name {
                    let version = min(version, ZwlrOutputManagerV1::interface().version);
                    state.output_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrVirtualPointerManagerV1::interface().name {
                    let version = min(version, ZwlrVirtualPointerManagerV1::interface().version);
                    state.virtual_pointer_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == WpSinglePixelBufferManagerV1::interface().name {
                    let version = min(version, WpSinglePixelBufferManagerV1::interface().version);
                    state.spbm = Some(registry.bind(name, version, qh, ()));
                } else if interface == WpViewporter::interface().name {
                    let version = min(version, WpViewporter::interface().version);
                    state.viewporter = Some(registry.bind(name, version, qh, ()));
                } else if interface == WlShm::interface().name {
                    let version = min(version, WlShm::interface().version);
                    state.shm = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrScreencopyManagerV1::interface().name {
                    let version = min(version, ZwlrScreencopyManagerV1::interface().version);
                    state.screencopy = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrGammaControlManagerV1::interface().name {
                    let version = min(version, ZwlrGammaControlManagerV1::interface().version);
                    state.gamma_control_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == MutterX11Interop::interface().name {
                    let version = min(version, MutterX11Interop::interface().version);
                    state.mutter_x11_interop = Some(registry.bind(name, version, qh, ()));
                } else if interface == WlOutput::interface().name {
                    let version = min(version, WlOutput::interface().version);
                    let output = registry.bind(name, version, qh, ());
                    state.outputs.insert(output, String::new());
                }

                let global = Global {
                    name,
                    interface,
                    version,
                };
                state.globals.push(global);
            }
            wl_registry::Event::GlobalRemove { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ExtSessionLockManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ExtSessionLockManagerV1,
        _event: ext_session_lock_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ExtSessionLockV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ExtSessionLockV1,
        event: ext_session_lock_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_session_lock_v1::Event::Locked => state.session_locked = true,
            ext_session_lock_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ExtSessionLockSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ExtSessionLockSurfaceV1,
        event: ext_session_lock_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let surface = state
            .lock_surfaces
            .iter_mut()
            .find(|surface| &surface.lock_surface == proxy)
            .unwrap();
        if let ext_session_lock_surface_v1::Event::Configure {
            serial,
            width,
            height,
        } = event
        {
            surface.configure = Some((serial, width, height));
        }
    }
}

impl Dispatch<ZxdgDecorationManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZxdgDecorationManagerV1,
        _event: <ZxdgDecorationManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZxdgToplevelDecorationV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZxdgToplevelDecorationV1,
        event: <ZxdgToplevelDecorationV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let window = state
            .windows
            .iter_mut()
            .find(|window| window.xdg_decoration.as_ref() == Some(proxy))
            .unwrap();
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode } = event {
            window.decoration_modes.push(mode.into_result().unwrap());
        }
    }
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlSubcompositor,
        _event: <WlSubcompositor as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WlSubsurface, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlSubsurface,
        _event: <WlSubsurface as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

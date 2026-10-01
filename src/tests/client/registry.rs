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

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
                macro_rules! bind {
                    ($proxy:ty, $field:ident) => {
                        if interface == <$proxy>::interface().name {
                            let version = min(version, <$proxy>::interface().version);
                            state.$field = Some(registry.bind(name, version, qh, ()));
                            true
                        } else {
                            false
                        }
                    };
                }

                let bound = bind!(WlCompositor, compositor)
                    || bind!(WlSubcompositor, subcompositor)
                    || bind!(WlSeat, seat)
                    || bind!(XdgActivationV1, xdg_activation)
                    || bind!(ZxdgDecorationManagerV1, xdg_decoration_manager)
                    || bind!(XdgToplevelTagManagerV1, xdg_toplevel_tag_manager)
                    || bind!(
                        ZwpKeyboardShortcutsInhibitManagerV1,
                        keyboard_shortcuts_inhibit_manager
                    )
                    || bind!(ZwlrLayerShellV1, layer_shell)
                    || bind!(ZwlrForeignToplevelManagerV1, foreign_toplevel_manager)
                    || bind!(ExtWorkspaceManagerV1, ext_workspace_manager)
                    || bind!(ExtSessionLockManagerV1, session_lock_manager)
                    || bind!(ZwlrOutputManagerV1, output_manager)
                    || bind!(ZwlrVirtualPointerManagerV1, virtual_pointer_manager)
                    || bind!(WpSinglePixelBufferManagerV1, spbm)
                    || bind!(WpViewporter, viewporter)
                    || bind!(WlShm, shm)
                    || bind!(ZwlrScreencopyManagerV1, screencopy)
                    || bind!(ZwlrGammaControlManagerV1, gamma_control_manager)
                    || bind!(MutterX11Interop, mutter_x11_interop);

                if !bound && interface == XdgWmBase::interface().name {
                    let version = min(version, XdgWmBase::interface().version);
                    state.xdg_wm_base = Some(registry.bind(name, version, qh, ()));
                    state.xdg_wm_base_version = Some(version);
                } else if !bound && interface == WlOutput::interface().name {
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

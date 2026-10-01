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

impl Dispatch<XdgToplevelTagManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgToplevelTagManagerV1,
        _event: <XdgToplevelTagManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgActivationV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgActivationV1,
        _event: xdg_activation_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgActivationTokenV1, Arc<std::sync::Mutex<Option<String>>>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        token: &Arc<std::sync::Mutex<Option<String>>>,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_activation_token_v1::Event::Done { token: value } => {
                *token.lock().unwrap() = Some(value);
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _state: &mut Self,
        xdg_wm_base: &XdgWmBase,
        event: <XdgWmBase as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_wm_base::Event::Ping { serial } => {
                xdg_wm_base.pong(serial);
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgPositioner, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgPositioner,
        _event: <XdgPositioner as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgPopup, ()> for State {
    fn event(
        state: &mut Self,
        popup: &XdgPopup,
        event: xdg_popup::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let popup = state
            .popups
            .iter_mut()
            .find(|candidate| candidate.xdg_popup == *popup)
            .unwrap();
        match event {
            xdg_popup::Event::Configure {
                x,
                y,
                width,
                height,
            } => popup.configures_received.push((x, y, width, height)),
            xdg_popup::Event::PopupDone => (),
            xdg_popup::Event::Repositioned { token } => popup.repositioned.push(token),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgSurface, ()> for State {
    fn event(
        state: &mut Self,
        xdg_surface: &XdgSurface,
        event: <XdgSurface as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_surface::Event::Configure { serial } => {
                if let Some(window) = state
                    .windows
                    .iter_mut()
                    .find(|window| window.xdg_surface == *xdg_surface)
                {
                    let configure = window.pending_configure.clone();
                    window.configures_received.push((serial, configure));
                } else if let Some(popup) = state
                    .popups
                    .iter()
                    .find(|popup| popup.xdg_surface == *xdg_surface)
                {
                    popup.xdg_surface.ack_configure(serial);
                } else {
                    panic!("configure for unknown xdg_surface")
                }
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgToplevel, ()> for State {
    fn event(
        state: &mut Self,
        xdg_toplevel: &XdgToplevel,
        event: <XdgToplevel as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let window = state
            .windows
            .iter_mut()
            .find(|w| w.xdg_toplevel == *xdg_toplevel)
            .unwrap();

        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let configure = &mut window.pending_configure;
                configure.size = (width, height);
                configure.states = states
                    .chunks_exact(4)
                    .flat_map(TryInto::<[u8; 4]>::try_into)
                    .map(u32::from_ne_bytes)
                    .flat_map(xdg_toplevel::State::try_from)
                    .collect();
            }
            xdg_toplevel::Event::Close => {
                window.close_requested = true;
            }
            xdg_toplevel::Event::ConfigureBounds { width, height } => {
                window.pending_configure.bounds = Some((width, height));
            }
            xdg_toplevel::Event::WmCapabilities { .. } => (),
            _ => unreachable!(),
        }
    }
}

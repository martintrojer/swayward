impl Dispatch<MutterX11Interop, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &MutterX11Interop,
        _event: <MutterX11Interop as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WpSinglePixelBufferManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpSinglePixelBufferManagerV1,
        _event: <WpSinglePixelBufferManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WpViewporter, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpViewporter,
        _event: <WpViewporter as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WpViewport, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpViewport,
        _event: <WpViewport as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

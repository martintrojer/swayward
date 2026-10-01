impl Dispatch<ZwlrGammaControlManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrGammaControlManagerV1,
        _event: <ZwlrGammaControlManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrGammaControlV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrGammaControlV1,
        event: <ZwlrGammaControlV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let control = state
            .gamma_controls
            .iter_mut()
            .find(|control| control.proxy == *proxy)
            .unwrap();
        match event {
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                control.gamma_size = Some(size);
            }
            zwlr_gamma_control_v1::Event::Failed => control.failed = true,
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrScreencopyManagerV1,
        _event: <ZwlrScreencopyManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, Arc<std::sync::Mutex<ScreencopyFrameEvents>>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        data: &Arc<std::sync::Mutex<ScreencopyFrameEvents>>,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let mut data = data.lock().unwrap();
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => data.buffer = Some((format.into_result().unwrap(), width, height, stride)),
            zwlr_screencopy_frame_v1::Event::LinuxDmabuf {
                format,
                width,
                height,
            } => data.linux_dmabuf = Some((format, width, height)),
            zwlr_screencopy_frame_v1::Event::BufferDone => data.buffer_done = true,
            zwlr_screencopy_frame_v1::Event::Flags { .. } => (),
            zwlr_screencopy_frame_v1::Event::Ready { .. } => data.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => data.failed = true,
            zwlr_screencopy_frame_v1::Event::Damage {
                x,
                y,
                width,
                height,
            } => data.damage.push((x, y, width, height)),
            _ => unreachable!(),
        }
    }
}

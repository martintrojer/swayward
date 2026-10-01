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

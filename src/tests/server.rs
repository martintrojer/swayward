use std::time::Duration;

use calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use swayward_config::Config;

use crate::swayward::{IpcMode, StartupOptions, State};

pub struct Server {
    pub event_loop: EventLoop<'static, State>,
    pub state: State,
}

impl Server {
    pub fn new(config: Config) -> Self {
        let event_loop = EventLoop::try_new().unwrap();
        let handle = event_loop.handle();
        let display = Display::new().unwrap();
        let state = State::new(
            config,
            handle.clone(),
            event_loop.get_signal(),
            display,
            StartupOptions {
                headless: true,
                create_wayland_socket: false,
                ipc_mode: IpcMode::Off,
                is_session_instance: false,
            },
        )
        .unwrap();

        Self { event_loop, state }
    }

    pub fn dispatch(&mut self) {
        self.event_loop
            .dispatch(Duration::ZERO, &mut self.state)
            .unwrap();
        self.state.refresh_and_flush_clients();
    }
}

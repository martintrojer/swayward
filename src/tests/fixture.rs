use std::os::fd::AsFd as _;
use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::time::Duration;

use calloop::generic::Generic;
use calloop::{EventLoop, Interest, LoopHandle, Mode, PostAction};
use smithay::output::Output;
use swayward_config::Config;

use super::client::{Client, ClientId};
use super::server::Server;
use crate::swayward::{NewClient, Swayward};

pub struct Fixture {
    pub event_loop: EventLoop<'static, State>,
    pub handle: LoopHandle<'static, State>,
    pub state: State,
}

pub struct State {
    pub server: Server,
    pub clients: Vec<Client>,
}

impl Fixture {
    pub fn new() -> Self {
        Self::with_config(Config::default())
    }

    pub fn with_config(config: Config) -> Self {
        let event_loop = EventLoop::try_new().unwrap();
        let handle = event_loop.handle();

        let server = Server::new(config);
        let fd = server.event_loop.as_fd().try_clone_to_owned().unwrap();
        let source = Generic::new(fd, Interest::READ, Mode::Level);
        handle
            .insert_source(source, |_, _, state: &mut State| {
                state.server.dispatch();
                Ok(PostAction::Continue)
            })
            .unwrap();

        let state = State {
            server,
            clients: Vec::new(),
        };

        Self {
            event_loop,
            handle,
            state,
        }
    }

    pub fn dispatch(&mut self) {
        self.event_loop
            .dispatch(Duration::ZERO, &mut self.state)
            .unwrap();
    }

    pub fn niri_state(&mut self) -> &mut crate::swayward::State {
        &mut self.state.server.state
    }

    pub fn swayward(&mut self) -> &mut Swayward {
        &mut self.niri_state().swayward
    }

    pub fn niri_output(&self, n: u8) -> Output {
        let swayward = &self.state.server.state.swayward;
        let idx = usize::from(n - 1);
        let output = swayward.global_space.outputs().nth(idx).unwrap();
        output.clone()
    }

    pub fn niri_focus_output(&mut self, n: u8) {
        let swayward = &mut self.state.server.state.swayward;
        let idx = usize::from(n - 1);
        let output = swayward.global_space.outputs().nth(idx).unwrap();
        swayward.layout.focus_output(output);
    }

    pub fn add_output(&mut self, n: u8, size: (u16, u16)) {
        self.add_output_at(n, size, None);
    }

    pub fn add_output_at(&mut self, n: u8, size: (u16, u16), position: Option<(i32, i32)>) {
        self.add_named_output_at(format!("headless-{n}"), size, position);
    }

    pub fn add_named_output_at(
        &mut self,
        name: String,
        size: (u16, u16),
        position: Option<(i32, i32)>,
    ) {
        let state = self.niri_state();
        let swayward = &mut state.swayward;
        state
            .backend
            .headless()
            .add_named_output_at(swayward, name, size, position);
    }

    pub fn replace_outputs(&mut self, outputs: Vec<((i32, i32), (u16, u16))>) {
        let existing = self
            .swayward()
            .layout
            .outputs()
            .cloned()
            .collect::<Vec<_>>();
        for output in existing {
            self.swayward().remove_output(&output);
        }
        let names = outputs
            .into_iter()
            .enumerate()
            .map(|(index, (position, size))| {
                let name = format!("fake-{index}");
                self.add_named_output_at(name.clone(), size, Some(position));
                name
            })
            .collect::<Vec<_>>();
        self.niri_state()
            .backend
            .headless()
            .retain_ipc_outputs(&names);
        self.niri_state().refresh_ipc_outputs();
        self.niri_state().ipc_refresh_layout();
    }

    pub fn add_client(&mut self) -> ClientId {
        let (sock1, sock2) = UnixStream::pair().unwrap();
        self.swayward().insert_client(NewClient {
            client: sock1,
            restricted: false,
            credentials_unknown: false,
            security_context: None,
        });

        let client = Client::new(sock2);
        let id = client.id;

        let fd = client.event_loop.as_fd().try_clone_to_owned().unwrap();
        let source = Generic::new(fd, Interest::READ, Mode::Level);
        self.handle
            .insert_source(source, move |_, _, state: &mut State| {
                state.client(id).dispatch();
                Ok(PostAction::Continue)
            })
            .unwrap();

        self.state.clients.push(client);
        self.roundtrip(id);
        id
    }

    pub fn client(&mut self, id: ClientId) -> &mut Client {
        self.state.client(id)
    }

    pub fn roundtrip(&mut self, id: ClientId) {
        let client = self.state.client(id);
        let data = client.send_sync();
        while !data.done.load(Ordering::Relaxed) {
            self.dispatch();
        }
    }

    /// Roundtrip twice in a row.
    ///
    /// For some reason, when running tests on many threads at once, a single roundtrip is
    /// sometimes not sufficient to get the configure events to the client.
    ///
    /// I suspect that this is because these configure events are sent from the niri loop callback,
    /// so they arrive after the sync done event and don't get processed in that client dispatch
    /// cycle. I'm not sure why this would be dependent on multithreading. But if this is indeed
    /// the issue, then a double roundtrip fixes it.
    pub fn double_roundtrip(&mut self, id: ClientId) {
        self.roundtrip(id);
        self.roundtrip(id);
    }
}

impl State {
    pub fn client(&mut self, id: ClientId) -> &mut Client {
        self.clients.iter_mut().find(|c| c.id == id).unwrap()
    }
}

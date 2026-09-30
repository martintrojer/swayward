use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
#[cfg(not(test))]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{env, io, process};

use anyhow::Context;
use async_channel::{Receiver, Sender};
use calloop::io::Async;
#[cfg(not(test))]
use directories::BaseDirs;
use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use futures_util::{select_biased, AsyncWrite, FutureExt as _};
use smithay::reexports::calloop::channel::{self, Event as ChannelEvent};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::{Interest, LoopHandle, Mode, PostAction};
use smithay::reexports::rustix::fs::unlink;
use smithay::reexports::rustix::io::Errno;
use swayward_ipc::legacy::{Event, Workspace};
use swayward_ipc::state::{EventStreamState, EventStreamStatePart as _};
use swayward_ipc::wire::{
    decode_header_raw, encode, encode_raw, CLOSE_SENTINEL, HEADER_SIZE, MAX_PAYLOAD_SIZE,
};
use swayward_ipc::{
    CommandOutcome, KeyboardLayouts, MessageType, Timestamp, Version, WindowLayout,
};

use crate::ipc::tree::{describe_tree, describe_workspaces_with_marks};
use crate::layout::workspace::WorkspaceId;
use crate::swayward::State;
use crate::utils::{version, with_toplevel_role, SWAYWARD_IPC_VERSION};
use crate::window::Mapped;

mod event_bridge;
mod query_state;
mod requests;
mod transport;

pub(crate) use event_bridge::ScratchpadEventOrder;
use event_bridge::WorkspaceEventTransaction;
pub(crate) use query_state::{find_node_by_id, ipc_outputs_snapshot, keyboard_layouts};
use query_state::{refresh_all_query_state, QueryState};
#[cfg(not(test))]
use transport::socket_dir;
use transport::{
    bind_listener, default_socket_path, on_new_ipc_client, select_socket_path, ClientCtx,
    CommandRequest, EventStreamSender, RequestKind,
};

const INITIAL_WRITE_BUFFER_SIZE: usize = 128;
const MAX_WRITE_BUFFER_SIZE: usize = 4_000_000;
#[cfg(not(test))]
static IPC_SOCKET_ID: AtomicU64 = AtomicU64::new(0);

pub struct IpcServer {
    pub socket_path: Option<PathBuf>,
    event_streams: Rc<RefCell<Vec<EventStreamSender>>>,
    next_event_stream_id: Rc<Cell<u64>>,
    event_stream_state: Rc<RefCell<EventStreamState>>,
    query_state: Rc<RefCell<QueryState>>,
    workspace_events: RefCell<Option<WorkspaceEventTransaction>>,
    commands: channel::Sender<CommandRequest>,
}

impl IpcServer {
    #[cfg(not(test))]
    pub fn start(
        event_loop: &LoopHandle<'static, State>,
        wayland_socket_name: Option<&OsStr>,
    ) -> anyhow::Result<Self> {
        let socket_path = wayland_socket_name.map(|wayland_socket_name| {
            let default = default_socket_path(
                socket_dir(),
                wayland_socket_name,
                process::id(),
                IPC_SOCKET_ID.fetch_add(1, Ordering::Relaxed),
            );
            select_socket_path(default, env::var_os("SWAYSOCK").map(Into::into))
        });
        Self::start_at(event_loop, socket_path)
    }

    pub(crate) fn start_at(
        event_loop: &LoopHandle<'static, State>,
        socket_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let _span = tracy_client::span!("Ipc::start");

        let (commands, command_rx) = channel::channel::<CommandRequest>();
        event_loop
            .insert_source(command_rx, |event, _, state| {
                if let ChannelEvent::Msg(request) = event {
                    let outcome = match request.kind {
                        RequestKind::Command(input) => crate::command::execute(state, &input),
                        RequestKind::RefreshQueryState => {
                            refresh_all_query_state(state);
                            Vec::new()
                        }
                        RequestKind::RefreshEventState => {
                            state.ipc_initialize_event_state();
                            Vec::new()
                        }
                    };
                    let _ = request.reply.send_blocking(outcome);
                }
            })
            .map_err(|error| anyhow::anyhow!(error.error))?;

        let socket_path = if let Some(socket_path) = socket_path {
            let listener = bind_listener(&socket_path)?;

            let source = Generic::new(listener, Interest::READ, Mode::Level);
            event_loop.insert_source(source, |_, socket, state| {
                match socket.accept() {
                    Ok((stream, _)) => on_new_ipc_client(state, stream),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                    Err(e) => return Err(e),
                }
                Ok(PostAction::Continue)
            })?;

            Some(socket_path)
        } else {
            None
        };

        Ok(Self {
            socket_path,
            event_streams: Rc::new(RefCell::new(Vec::new())),
            next_event_stream_id: Rc::new(Cell::new(0)),
            event_stream_state: Rc::new(RefCell::new(EventStreamState::default())),
            query_state: Rc::new(RefCell::new(QueryState::default())),
            workspace_events: RefCell::new(None),
            commands,
        })
    }

    pub(crate) fn has_event_streams(&self) -> bool {
        !self.event_streams.borrow().is_empty()
    }

    #[cfg(test)]
    pub(crate) fn event_stream_count(&self) -> usize {
        self.event_streams.borrow().len()
    }

    /// Record the config path that GET_VERSION reports.
    ///
    /// Called at startup and on reload. Without it the field stays empty in a
    /// real session, which is how it shipped until a hardware boot showed it.
    pub fn set_loaded_config_file_name(&self, path: String) {
        self.query_state.borrow_mut().loaded_config_file_name = path;
    }

    pub(crate) fn send_event(&self, event: Event) {
        if let Some(transaction) = self.workspace_events.borrow_mut().as_mut() {
            transaction.events.push(event);
            return;
        }
        self.send_event_now(event);
    }

    fn send_event_now(&self, event: Event) {
        let event_type = match &event {
            Event::WorkspacesChanged { .. } => "workspaces_changed",
            Event::WorkspaceEmptied { .. } => "workspace_empty",
            Event::WorkspaceReloaded => "workspace_reload",
            Event::WorkspaceInitialized { .. } => "workspace_init",
            Event::WorkspaceRenamed { .. } => "workspace_rename",
            Event::WorkspaceFocusChanged { .. } => "workspace_focus",
            Event::WorkspaceMoved { .. } => "workspace_move",
            Event::WorkspaceUrgencyChanged { .. } => "workspace_urgent",
            Event::WorkspaceActivated { .. } => "workspace_activated",
            Event::WorkspaceActiveWindowChanged { .. } => "workspace_active_window",
            Event::WindowsChanged { .. } => "windows_changed",
            Event::WindowOpenedOrChanged { .. } => "window_opened_or_changed",
            Event::SwayWindowChanged { .. } => "sway_window",
            Event::WindowMoved { .. } => "window_move",
            Event::WindowClosed { .. } => "window_close",
            Event::WindowFocusChanged { .. } => "window_focus",
            Event::WindowFocusTimestampChanged { .. } => "window_focus_timestamp",
            Event::WindowUrgencyChanged { .. } => "window_urgent",
            Event::WindowLayoutsChanged { .. } => "window_layout",
            Event::KeyboardLayoutsChanged { .. } => "keyboard_layouts",
            Event::KeyboardLayoutSwitched { .. } => "keyboard_layout_switch",
            Event::SwayInputChanged { .. } => "sway_input",
            Event::OutputChanged => "output",
            Event::Shutdown { .. } => "shutdown",
            Event::Tick { .. } => "tick",
            Event::SwayBinding { .. } => "binding",
            Event::BindingModeChanged { .. } => "binding_mode",
            Event::OverviewOpenedOrClosed { .. } => "overview",
            Event::ConfigLoaded { .. } => "config",
            Event::ScreenshotCaptured { .. } => "screenshot",
            Event::CastsChanged { .. } => "casts_changed",
            Event::CastStartedOrChanged { .. } => "cast_started_or_changed",
            Event::CastStopped { .. } => "cast_stopped",
        };
        trace!(event_type, "emitting IPC event");
        let mut streams = self.event_streams.borrow_mut();
        let mut to_remove = Vec::new();
        for (idx, stream) in streams.iter_mut().enumerate() {
            if stream.events.try_send(event.clone()).is_err() {
                to_remove.push(idx);
            }
        }
        for idx in to_remove.into_iter().rev() {
            let stream = streams.swap_remove(idx);
            let _ = stream.disconnect.send_blocking(());
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        if let Some(socket_path) = &self.socket_path {
            let _ = unlink(socket_path);
        }
    }
}

#[cfg(test)]
mod tests;

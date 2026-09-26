use std::cell::RefCell;
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
use swayward_ipc::wire::{decode_header_raw, encode, encode_raw, CLOSE_SENTINEL, HEADER_SIZE};
use swayward_ipc::{
    CommandOutcome, KeyboardLayouts, MessageType, Timestamp, Version, WindowLayout,
};

use crate::ipc::tree::{describe_tree, describe_workspaces_with_marks};
use crate::layout::workspace::WorkspaceId;
use crate::swayward::State;
use crate::utils::{version, with_toplevel_role};
use crate::window::Mapped;

const INITIAL_WRITE_BUFFER_SIZE: usize = 128;
const MAX_WRITE_BUFFER_SIZE: usize = 4_000_000;
const MAX_PAYLOAD_SIZE: u32 = 16 * 1024 * 1024;
#[cfg(not(test))]
static IPC_SOCKET_ID: AtomicU64 = AtomicU64::new(0);

pub struct IpcServer {
    pub socket_path: Option<PathBuf>,
    event_streams: Rc<RefCell<Vec<EventStreamSender>>>,
    event_stream_state: Rc<RefCell<EventStreamState>>,
    query_state: Rc<RefCell<QueryState>>,
    commands: channel::Sender<CommandRequest>,
}

#[derive(Default)]
struct QueryState {
    loaded_config_file_name: String,
    tree: String,
    event_tree: String,
    workspaces: String,
    outputs: String,
    marks: String,
    binding_modes: String,
    binding_state: String,
    inputs: String,
    seats: String,
}

struct EventStreamClient {
    events: Receiver<Event>,
    disconnect: Receiver<()>,
    read: Box<dyn AsyncRead + Unpin>,
    write: Box<dyn AsyncWrite + Unpin>,
    subscriptions: HashSet<String>,
    ctx: ClientCtx,
}

#[derive(Clone)]
struct ClientCtx {
    query_state: Rc<RefCell<QueryState>>,
    event_streams: Rc<RefCell<Vec<EventStreamSender>>>,
    commands: channel::Sender<CommandRequest>,
}

struct EventStreamSender {
    events: Sender<Event>,
    disconnect: Sender<()>,
}

struct CommandRequest {
    kind: RequestKind,
    reply: Sender<Vec<CommandOutcome>>,
}

enum RequestKind {
    Command(String),
    /// Recompute the cached query replies from the live compositor state, so a
    /// long-lived connection does not answer from its connect-time snapshot.
    /// Sway builds every query reply at request time
    /// (`sway/sway/ipc-server.c:815-823`).
    RefreshQueryState,
    /// Establish an event diff baseline immediately before subscribing.
    RefreshEventState,
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
            event_stream_state: Rc::new(RefCell::new(EventStreamState::default())),
            query_state: Rc::new(RefCell::new(QueryState::default())),
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
        debug!(event_type, "emitting IPC event");
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

#[cfg(not(test))]
fn socket_dir() -> PathBuf {
    socket_dir_from(BaseDirs::new().and_then(|dirs| dirs.runtime_dir().map(PathBuf::from)))
}

fn socket_dir_from(runtime_dir: Option<PathBuf>) -> PathBuf {
    runtime_dir.unwrap_or_else(env::temp_dir)
}

fn default_socket_path(dir: PathBuf, wayland_socket_name: &OsStr, pid: u32, id: u64) -> PathBuf {
    dir.join(format!(
        "swayward-ipc.{}.{pid}.{id}.sock",
        wayland_socket_name.to_string_lossy()
    ))
}

fn select_socket_path(default: PathBuf, requested: Option<PathBuf>) -> PathBuf {
    requested.filter(|path| !path.exists()).unwrap_or(default)
}

fn bind_listener(path: &std::path::Path) -> anyhow::Result<UnixListener> {
    match unlink(path) {
        Ok(()) => (),
        Err(Errno::NOENT) => (),
        Err(error) => {
            return Err(io::Error::from_raw_os_error(error.raw_os_error()))
                .context("error removing stale IPC socket")
        }
    }
    let listener = UnixListener::bind(path).context("error binding socket")?;
    listener
        .set_nonblocking(true)
        .context("error setting socket to non-blocking")?;
    Ok(listener)
}

fn on_new_ipc_client(state: &mut State, stream: UnixStream) {
    let stream = match state.swayward.event_loop.adapt_io(stream) {
        Ok(stream) => stream,
        Err(err) => {
            warn!("error making IPC stream async: {err:?}");
            return;
        }
    };

    if state.swayward.ipc_server.is_none() {
        return;
    }
    let Some(server) = &state.swayward.ipc_server else {
        return;
    };
    let ctx = ClientCtx {
        query_state: server.query_state.clone(),
        event_streams: server.event_streams.clone(),
        commands: server.commands.clone(),
    };
    let future = async move {
        if let Err(err) = handle_client(ctx, stream).await {
            warn!("error handling IPC client: {err:?}");
        }
    };
    if let Err(err) = state.swayward.scheduler.schedule(future) {
        warn!("error scheduling IPC stream future: {err:?}");
    }
}

async fn handle_client(ctx: ClientCtx, stream: Async<'static, UnixStream>) -> anyhow::Result<()> {
    let (mut read, mut write) = stream.split();
    loop {
        let mut header = [0; HEADER_SIZE];
        match read.read_exact(&mut header).await {
            Ok(_) => (),
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(err) => return Err(err).context("error reading IPC header"),
        }

        if &header == CLOSE_SENTINEL {
            return Ok(());
        }
        // Decode the frame before interpreting the type. A request type this
        // build does not model must still get a reply: sway answers IPC_SYNC
        // with {"success": false} (`sway/sway/ipc-server.c:919-924`) and keeps
        // the connection open for anything else (`ipc-server.c:927-929`).
        let (raw_type, payload_len) = decode_header_raw(&header).inspect_err(|_| {
            warn!(
                bytes = %header.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" "),
                ascii = %String::from_utf8_lossy(&header),
                "invalid IPC header"
            );
        })?;
        let msg_type = MessageType::try_from(raw_type).ok();
        debug!(?msg_type, raw_type, payload_len, "received IPC request");
        if payload_len > MAX_PAYLOAD_SIZE {
            anyhow::bail!("IPC payload exceeds {MAX_PAYLOAD_SIZE} bytes");
        }
        let mut payload = vec![0; payload_len as usize];
        read.read_exact(&mut payload)
            .await
            .context("error reading IPC payload")?;

        if msg_type == Some(MessageType::Subscribe) {
            let msg_type = MessageType::Subscribe;
            let Some(subscriptions) = parse_subscriptions(&payload) else {
                write
                    .write_all(&encode(msg_type, r#"{"success": false}"#))
                    .await
                    .context("error writing IPC reply")?;
                continue;
            };
            refresh_event_state(&ctx).await;
            let (events_tx, events_rx) = async_channel::bounded(4096);
            let (disconnect_tx, disconnect_rx) = async_channel::bounded(1);
            ctx.event_streams.borrow_mut().push(EventStreamSender {
                events: events_tx,
                disconnect: disconnect_tx,
            });
            write
                .write_all(&encode(msg_type, r#"{"success": true}"#))
                .await
                .context("error writing IPC reply")?;

            if subscriptions.iter().any(|event| event == "tick") {
                write
                    .write_all(&swayward_ipc::wire::encode_raw(
                        (1 << 31) | 7,
                        r#"{"first":true,"payload":""}"#,
                    ))
                    .await
                    .context("error writing initial tick event")?;
            }
            return handle_event_stream_client(EventStreamClient {
                events: events_rx,
                disconnect: disconnect_rx,
                read: Box::new(read),
                write: Box::new(write),
                subscriptions: subscriptions.into_iter().collect(),
                ctx: ctx.clone(),
            })
            .await;
        }

        let reply = match msg_type {
            Some(msg_type) => dispatch(&ctx, msg_type, &payload).await,
            // IPC_SYNC, sway/include/ipc.h:20: sway decided not to support it
            // and replies success:false rather than closing the socket.
            None if raw_type == IPC_SYNC => r#"{"success": false}"#.to_owned(),
            None => r#"{"success":false,"error":"not implemented"}"#.to_owned(),
        };
        write
            .write_all(&encode_raw(raw_type, &reply))
            .await
            .context("error writing IPC reply")?;
    }
}

/// `IPC_SYNC`, `sway/include/ipc.h:20`. Not a `MessageType`: sway never
/// implemented it and only keeps the code reserved.
const IPC_SYNC: u32 = 11;

async fn refresh_event_state(ctx: &ClientCtx) {
    let (reply, receiver) = async_channel::bounded(1);
    if ctx
        .commands
        .send(CommandRequest {
            kind: RequestKind::RefreshEventState,
            reply,
        })
        .is_ok()
    {
        let _ = receiver.recv().await;
    }
}

fn parse_subscriptions(payload: &[u8]) -> Option<Vec<String>> {
    let subscriptions: Vec<String> = serde_json::from_slice(payload).ok()?;
    subscriptions
        .iter()
        .all(|event| {
            matches!(
                event.as_str(),
                "workspace"
                    | "output"
                    | "mode"
                    | "shutdown"
                    | "window"
                    | "binding"
                    | "tick"
                    | "input"
            )
        })
        .then_some(subscriptions)
}

/// Recompute every cached query reply from live compositor state.
fn refresh_all_query_state(state: &mut State) {
    let Some(server) = &state.swayward.ipc_server else {
        return;
    };
    let mut query_state = server.query_state.borrow_mut();
    query_state.binding_modes = binding_modes(&state.swayward.config.borrow());
    query_state.binding_state = binding_state(&state.swayward.binding_mode);
    refresh_input_query_state(&state.swayward, &mut query_state);
    let ipc_outputs = state.backend.ipc_outputs().lock().unwrap().clone();
    refresh_query_state(
        &state.swayward.layout,
        &state.swayward.global_space,
        &state.swayward.output_power,
        &ipc_outputs,
        &state.swayward.marks_by_window,
        &state.swayward.marks_by_container,
        &mut query_state,
    );
}

async fn dispatch(ctx: &ClientCtx, msg_type: MessageType, payload: &[u8]) -> String {
    // Sway serialises each query from the live tree when the request arrives
    // (`sway/sway/ipc-server.c:815-823`). We cache, so refresh first: a
    // connection that stays open (any subscriber) would otherwise answer from
    // the snapshot taken when it connected.
    if matches!(
        msg_type,
        MessageType::GetTree
            | MessageType::GetWorkspaces
            | MessageType::GetOutputs
            | MessageType::GetMarks
            | MessageType::GetInputs
            | MessageType::GetSeats
            | MessageType::GetConfig
            | MessageType::GetBindingModes
            | MessageType::GetBindingState
            | MessageType::GetVersion
    ) {
        let (reply, receiver) = async_channel::bounded(1);
        if ctx
            .commands
            .send(CommandRequest {
                kind: RequestKind::RefreshQueryState,
                reply,
            })
            .is_ok()
        {
            let _ = receiver.recv().await;
        }
    }
    match msg_type {
        MessageType::GetVersion => serde_json::to_string(&Version {
            human_readable: version(),
            variant: "swayward".into(),
            major: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0),
            minor: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0),
            patch: env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0),
            loaded_config_file_name: ctx.query_state.borrow().loaded_config_file_name.clone(),
        })
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into()),
        MessageType::GetTree => ctx.query_state.borrow().tree.clone(),
        MessageType::GetWorkspaces => ctx.query_state.borrow().workspaces.clone(),
        MessageType::GetOutputs => ctx.query_state.borrow().outputs.clone(),
        MessageType::GetMarks => ctx.query_state.borrow().marks.clone(),
        MessageType::GetBindingModes => ctx.query_state.borrow().binding_modes.clone(),
        MessageType::GetBindingState => ctx.query_state.borrow().binding_state.clone(),
        // GET_CONFIG is not implemented, and must not be faked.
        //
        // Sway's contract is the verbatim text of the sway config file:
        // `config->current_config` is the file read byte for byte into a
        // buffer (`sway/sway/config.c:734-773`) and returned unaltered
        // (`sway/sway/ipc-server.c:908-917`). A client receiving it expects
        // sway syntax it can parse, diff or re-serve.
        //
        // swayward's config is KDL. Returning it in sway's single-field
        // envelope would be well-formed and wrong: the shape says "sway
        // config" and the bytes are not one, so a client that parses the
        // reply breaks in a way no error surfaces. A wire deviation is 100%
        // compliant or not implemented; there is no third option.
        //
        // Sway itself sets the precedent for the honest answer: IPC_SYNC
        // returns `{"success": false}` rather than inventing a reply
        // (`sway/sway/ipc-server.c:919-925`).
        MessageType::GetConfig => String::from(r#"{"success": false}"#),
        MessageType::GetInputs => ctx.query_state.borrow().inputs.clone(),
        MessageType::GetSeats => ctx.query_state.borrow().seats.clone(),
        MessageType::RunCommand => {
            let input = match String::from_utf8(payload.to_vec()) {
                Ok(input) => input,
                Err(_) => {
                    return serialize_outcomes(&[CommandOutcome {
                        success: false,
                        error: Some("command is not valid UTF-8".into()),
                        parse_error: Some(true),
                    }]);
                }
            };
            let (reply, receiver) = async_channel::bounded(1);
            if ctx
                .commands
                .send(CommandRequest {
                    kind: RequestKind::Command(input),
                    reply,
                })
                .is_err()
            {
                return serialize_outcomes(&[CommandOutcome {
                    success: false,
                    error: Some("command dispatcher is unavailable".into()),
                    parse_error: None,
                }]);
            }
            match receiver.recv().await {
                Ok(outcomes) => serialize_outcomes(&outcomes),
                Err(_) => serialize_outcomes(&[CommandOutcome {
                    success: false,
                    error: Some("command dispatcher stopped without replying".into()),
                    parse_error: None,
                }]),
            }
        }
        MessageType::GetBarConfig if payload.is_empty() => "[]".into(),
        // Byte-identical to sway, spaces included: it writes this as a C
        // string literal rather than serialising it
        // (`sway/ipc-server.c:870`).
        MessageType::GetBarConfig => {
            r#"{ "success": false, "error": "No bar with that ID" }"#.into()
        }
        MessageType::SendTick => {
            let payload = String::from_utf8_lossy(payload).into_owned();
            for stream in ctx.event_streams.borrow_mut().iter_mut() {
                let _ = stream.events.try_send(Event::Tick {
                    payload: payload.clone(),
                    first: false,
                });
            }
            // Sway writes this literal with a space, and the two other
            // success replies in this file already match it
            // (`sway/ipc-server.c`, IPC_SEND_TICK).
            r#"{"success": true}"#.into()
        }
        _ => r#"{"success":false,"error":"not implemented"}"#.into(),
    }
}

fn binding_modes(config: &swayward_config::Config) -> String {
    let modes = std::iter::once("default")
        .chain(config.binding_modes.iter().map(|mode| mode.name.as_str()))
        .collect::<Vec<_>>();
    serde_json::to_string(&modes).unwrap_or_else(|_| "[]".into())
}

fn binding_state(mode: &str) -> String {
    serde_json::json!({"name": mode}).to_string()
}

pub(crate) fn keyboard_layouts(state: &mut State) -> Option<KeyboardLayouts> {
    let keyboard = state.swayward.seat.get_keyboard()?;
    keyboard.with_xkb_state(state, |context| {
        let Ok(xkb) = context.xkb().lock() else {
            error!("cannot refresh IPC keyboard layouts: XKB state lock is poisoned");
            return None;
        };
        Some(KeyboardLayouts {
            names: xkb
                .layouts()
                .map(|layout| xkb.layout_name(layout).to_owned())
                .collect(),
            current_idx: xkb.active_layout().0,
        })
    })
}

fn serialize_outcomes(outcomes: &[CommandOutcome]) -> String {
    serde_json::to_string(outcomes)
        .unwrap_or_else(|_| r#"[{"success":false,"error":"serialization failed"}]"#.into())
}

#[derive(serde::Serialize)]
struct IpcSeat<'a> {
    name: &'a str,
    capabilities: u32,
    focus: i64,
    devices: &'a [serde_json::Value],
}

fn describe_input(
    swayward: &crate::swayward::Swayward,
    device: &crate::input::IpcInputDevice,
) -> serde_json::Value {
    let mut value = serde_json::to_value(device).unwrap_or_default();
    if device.device_type == "pointer" {
        if let Some(object) = value.as_object_mut() {
            let config = swayward.config.borrow();
            let factor = config
                .input
                .mouse
                .scroll_factor
                .and_then(|factor| {
                    let (horizontal, vertical) = factor.h_v_factors();
                    (horizontal == vertical).then_some(horizontal)
                })
                .unwrap_or(1.);
            object.insert("scroll_factor".into(), factor.into());
        }
    } else if device.device_type == "keyboard" {
        if let Some(object) = value.as_object_mut() {
            let config = swayward.config.borrow();
            object.insert(
                "repeat_delay".into(),
                serde_json::json!(config.input.keyboard.repeat_delay),
            );
            object.insert(
                "repeat_rate".into(),
                serde_json::json!(config.input.keyboard.repeat_rate),
            );
        }
        let layouts = swayward.ipc_server.as_ref().and_then(|server| {
            server
                .event_stream_state
                .borrow()
                .keyboard_layouts
                .keyboard_layouts
                .clone()
        });
        if let (Some(layouts), Some(object)) = (layouts, value.as_object_mut()) {
            object.insert("xkb_layout_names".into(), serde_json::json!(layouts.names));
            object.insert(
                "xkb_active_layout_index".into(),
                serde_json::json!(layouts.current_idx),
            );
            object.insert(
                "xkb_active_layout_name".into(),
                layouts
                    .names
                    .get(layouts.current_idx as usize)
                    .map_or(serde_json::Value::Null, |name| serde_json::json!(name)),
            );
        }
    }
    value
}

fn describe_inputs(swayward: &crate::swayward::Swayward) -> Vec<serde_json::Value> {
    let mut input_devices = swayward.ipc_input_devices.values().collect::<Vec<_>>();
    input_devices.sort_by(|left, right| left.identifier.cmp(&right.identifier));
    input_devices
        .into_iter()
        .map(|device| describe_input(swayward, device))
        .collect()
}

fn refresh_input_query_state(swayward: &crate::swayward::Swayward, state: &mut QueryState) {
    let devices = describe_inputs(swayward);
    state.inputs = serde_json::to_string(&devices).unwrap_or_else(|_| "[]".into());

    let capabilities = u32::from(swayward.seat.get_pointer().is_some())
        | (u32::from(swayward.seat.get_keyboard().is_some()) << 1)
        | (u32::from(swayward.seat.get_touch().is_some()) << 2);
    let focus = swayward
        .layout
        .focus()
        .map(|window| crate::ipc::tree::window_id(window.id()))
        .unwrap_or(0);
    state.seats = serde_json::to_string(&[IpcSeat {
        name: &swayward.seat_name,
        capabilities,
        focus,
        devices: &devices,
    }])
    .unwrap_or_else(|_| "[]".into());
}

pub(crate) fn find_node_by_id(value: &serde_json::Value, id: i64) -> Option<&serde_json::Value> {
    if value.get("id").and_then(serde_json::Value::as_i64) == Some(id) {
        return Some(value);
    }
    ["nodes", "floating_nodes"].into_iter().find_map(|key| {
        value
            .get(key)?
            .as_array()?
            .iter()
            .find_map(|child| find_node_by_id(child, id))
    })
}

fn find_workspace_by_id(node: &swayward_ipc::Node, id: u64) -> Option<&swayward_ipc::Node> {
    if node.node_type == swayward_ipc::NodeType::Workspace
        && node.id == crate::ipc::tree::workspace_id(id)
    {
        return Some(node);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(|child| find_workspace_by_id(child, id))
}

/// Append every mark in the tree, parent before child, matching sway's
/// `root_for_each_container` walk.
fn collect_marks(node: &swayward_ipc::Node, out: &mut Vec<String>) {
    out.extend(node.marks.iter().cloned());
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect_marks(child, out);
    }
}

fn refresh_query_state(
    layout: &crate::layout::Layout<Mapped>,
    global_space: &smithay::desktop::Space<smithay::desktop::Window>,
    output_power: &std::collections::HashMap<String, bool>,
    ipc_outputs: &crate::backend::IpcOutputMap,
    marks: &std::collections::HashMap<crate::window::mapped::MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<
        (
            crate::layout::workspace::WorkspaceId,
            crate::layout::tiling_tree::NodeId,
        ),
        Vec<String>,
    >,
    state: &mut QueryState,
) {
    let tree = describe_tree(layout, global_space, marks, container_marks);
    state.tree = serde_json::to_string(&tree)
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    state.workspaces = serde_json::to_string(&describe_workspaces_with_marks(
        layout,
        global_space,
        marks,
        container_marks,
    ))
    .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    let mut outputs = serde_json::to_value(crate::ipc::tree::describe_outputs_with_power(
        layout,
        global_space,
        output_power,
    ))
    .unwrap_or_default();
    if let Some(outputs) = outputs.as_array_mut() {
        outputs.extend(
            ipc_outputs
                .values()
                .filter(|output| output.logical.is_none())
                .map(|output| {
                    serde_json::json!({
                        "active": false,
                        "current_workspace": null,
                        "dpms": false,
                        "features": { "adaptive_sync": false, "hdr": false },
                        "make": output.make,
                        "model": output.model,
                        "modes": output.modes.iter().map(|mode| serde_json::json!({
                            "width": mode.width,
                            "height": mode.height,
                            "refresh": mode.refresh_rate,
                        })).collect::<Vec<_>>(),
                        "name": output.name,
                        "non_desktop": false,
                        "percent": null,
                        "power": false,
                        "primary": false,
                        "rect": swayward_ipc::Rect::default(),
                        "serial": output.serial,
                        "type": "output",
                    })
                }),
        );
    }
    state.outputs = serde_json::to_string(&outputs)
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    // Sway walks the container tree and appends each container's marks in the
    // order it meets them (`sway/tree/root.c:246-260`,
    // `sway/ipc-server.c:604-610,825-834`). Collecting from the tree we just
    // built gives that order for free, and reaches marks on split containers
    // as well as on views. Sorting the per-window map did neither: it imposed
    // an order sway never uses, and omitted container marks that GET_TREE was
    // already reporting.
    let mut all_marks = Vec::new();
    collect_marks(&tree, &mut all_marks);
    state.marks = serde_json::to_string(&all_marks)
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
}

async fn handle_event_stream_client(client: EventStreamClient) -> anyhow::Result<()> {
    let EventStreamClient {
        events,
        disconnect,
        mut read,
        mut write,
        mut subscriptions,
        ctx,
    } = client;
    let query_state = ctx.query_state.clone();

    enum StreamInput {
        Read(io::Result<usize>),
        Event(Event),
        Written(io::Result<usize>),
    }

    let mut write_buffer = Vec::new();
    let mut write_buffer_size = INITIAL_WRITE_BUFFER_SIZE;
    let mut header = [0; HEADER_SIZE];
    let mut header_filled = 0;
    loop {
        let write_ready = if write_buffer.is_empty() {
            futures_util::future::Either::Left(futures_util::future::pending())
        } else {
            futures_util::future::Either::Right(write.write(&write_buffer))
        };
        let input = select_biased! {
            _ = disconnect.recv().fuse() => return Ok(()),
            result = read.read(&mut header[header_filled..]).fuse() => StreamInput::Read(result),
            result = write_ready.fuse() => StreamInput::Written(result),
            result = events.recv().fuse() => match result {
                Ok(event) => StreamInput::Event(event),
                Err(_) => return Ok(()),
            },
        };
        let event = match input {
            StreamInput::Written(Ok(written)) => {
                write_buffer.drain(..written);
                continue;
            }
            StreamInput::Written(Err(error)) if error.kind() == io::ErrorKind::BrokenPipe => {
                return Ok(());
            }
            StreamInput::Written(Err(error)) => {
                return Err(error).context("error writing IPC event");
            }
            StreamInput::Read(Ok(0)) => return Ok(()),
            StreamInput::Read(Ok(bytes_read)) => {
                header_filled += bytes_read;
                if header_filled < HEADER_SIZE {
                    continue;
                }
                header_filled = 0;
                if &header == CLOSE_SENTINEL {
                    return Ok(());
                }
                let (raw_type, payload_len) = decode_header_raw(&header)?;
                if payload_len > MAX_PAYLOAD_SIZE {
                    anyhow::bail!("IPC payload exceeds {MAX_PAYLOAD_SIZE} bytes");
                }
                let mut payload = vec![0; payload_len as usize];
                read.read_exact(&mut payload)
                    .await
                    .context("error reading IPC payload")?;
                let msg_type = MessageType::try_from(raw_type).ok();
                // A subscribed connection stays a normal IPC connection in
                // sway: `IPC_SUBSCRIBE` only sets `subscribed_events` and the
                // client keeps being served by `ipc_client_handle_command`
                // (`sway/sway/ipc-server.c:730-784`). Answer queries and
                // commands here rather than dropping the client.
                let Some(MessageType::Subscribe) = msg_type else {
                    let reply = match msg_type {
                        Some(msg_type) => dispatch(&ctx, msg_type, &payload).await,
                        None if raw_type == IPC_SYNC => r#"{"success": false}"#.to_owned(),
                        None => r#"{"success":false,"error":"not implemented"}"#.to_owned(),
                    };
                    queue_ipc_message(
                        &mut write_buffer,
                        &mut write_buffer_size,
                        &encode_raw(raw_type, &reply),
                    )?;
                    continue;
                };
                let msg_type = MessageType::Subscribe;
                let Some(requested) = parse_subscriptions(&payload) else {
                    // sway replies `{"success": false}` to an unparseable or
                    // unsupported subscribe and keeps the client
                    // (`sway/sway/ipc-server.c:733-765`).
                    queue_ipc_message(
                        &mut write_buffer,
                        &mut write_buffer_size,
                        &encode(msg_type, r#"{"success": false}"#),
                    )?;
                    continue;
                };
                refresh_event_state(&ctx).await;
                let send_first_tick = requested.iter().any(|event| event == "tick");
                subscriptions.extend(requested);
                queue_ipc_message(
                    &mut write_buffer,
                    &mut write_buffer_size,
                    &encode(msg_type, r#"{"success": true}"#),
                )?;
                if send_first_tick {
                    queue_ipc_message(
                        &mut write_buffer,
                        &mut write_buffer_size,
                        &encode_raw((1 << 31) | 7, r#"{"first":true,"payload":""}"#),
                    )?;
                }
                continue;
            }
            StreamInput::Read(Err(error)) => {
                return Err(error).context("error reading IPC event stream");
            }
            StreamInput::Event(event) => event,
        };
        let (msg_type, payload) = match event {
            Event::OutputChanged if subscriptions.contains("output") => {
                ((1 << 31) | 1, serde_json::json!({"change":"unspecified"}))
            }
            Event::SwayInputChanged { change, input } if subscriptions.contains("input") => (
                (1 << 31) | 21,
                serde_json::json!({"change":change,"input":input}),
            ),
            Event::WorkspaceEmptied { current } if subscriptions.contains("workspace") => (
                1 << 31,
                serde_json::json!({"change":"empty","old":null,"current":current}),
            ),
            Event::WorkspaceReloaded if subscriptions.contains("workspace") => (
                1 << 31,
                serde_json::json!({"change":"reload","old":null,"current":null}),
            ),
            Event::WorkspaceInitialized { current } if subscriptions.contains("workspace") => (
                1 << 31,
                serde_json::json!({"change":"init","old":null,"current":current}),
            ),
            Event::WorkspaceRenamed { current } if subscriptions.contains("workspace") => (
                1 << 31,
                serde_json::json!({"change":"rename","old":null,"current":current}),
            ),
            Event::WorkspaceMoved { current } if subscriptions.contains("workspace") => (
                1 << 31,
                serde_json::json!({"change":"move","old":null,"current":current}),
            ),
            Event::WorkspaceUrgencyChanged { current, .. }
                if subscriptions.contains("workspace") =>
            {
                (
                    1 << 31,
                    serde_json::json!({"change":"urgent","old":null,"current":current}),
                )
            }
            Event::WorkspaceFocusChanged { old, current }
                if subscriptions.contains("workspace") =>
            {
                (
                    1 << 31,
                    serde_json::json!({"change":"focus","old":old,"current":current}),
                )
            }
            Event::WorkspaceActiveWindowChanged { .. }
            | Event::WorkspacesChanged { .. }
            | Event::WorkspaceActivated { .. } => continue,
            Event::Shutdown { reason } if subscriptions.contains("shutdown") => {
                ((1 << 31) | 6, serde_json::json!({"change":reason}))
            }
            Event::Tick { payload, first } if subscriptions.contains("tick") => (
                (1 << 31) | 7,
                serde_json::json!({"first":first,"payload":payload}),
            ),
            Event::BindingModeChanged { mode, pango_markup } if subscriptions.contains("mode") => (
                (1 << 31) | 2,
                serde_json::json!({"change":mode,"pango_markup":pango_markup}),
            ),
            Event::SwayBinding {
                command,
                event_state_mask,
                input_codes,
                input_code,
                symbols,
                symbol,
                input_type,
            } if subscriptions.contains("binding") => (
                (1 << 31) | 5,
                serde_json::json!({
                    "change":"run",
                    "binding": {
                        "command": command,
                        "event_state_mask": event_state_mask,
                        "input_codes": input_codes,
                        "input_code": input_code,
                        "symbols": symbols,
                        "symbol": symbol,
                        "input_type": input_type,
                    }
                }),
            ),
            Event::SwayWindowChanged { change, container } if subscriptions.contains("window") => (
                (1 << 31) | 3,
                serde_json::json!({"change":change,"container":container}),
            ),
            Event::WindowMoved { id } if subscriptions.contains("window") => {
                let container =
                    serde_json::from_str::<serde_json::Value>(&query_state.borrow().tree)
                        .ok()
                        .and_then(|tree| find_node_by_id(&tree, id).cloned());
                (
                    (1 << 31) | 3,
                    serde_json::json!({"change":"move","container":container}),
                )
            }
            Event::WindowsChanged { .. }
            | Event::WindowOpenedOrChanged { .. }
            | Event::WindowClosed { .. }
            | Event::WindowFocusTimestampChanged { .. }
            | Event::WindowUrgencyChanged { .. }
            | Event::WindowLayoutsChanged { .. }
            | Event::WindowFocusChanged { .. }
                if subscriptions.contains("window") =>
            {
                continue
            }
            _ => continue,
        };
        let payload = serde_json::to_string(&payload).context("error formatting event")?;
        let buf = swayward_ipc::wire::encode_raw(msg_type, &payload);
        queue_ipc_message(&mut write_buffer, &mut write_buffer_size, &buf)?;
    }
}

fn queue_ipc_message(
    buffer: &mut Vec<u8>,
    buffer_size: &mut usize,
    message: &[u8],
) -> anyhow::Result<()> {
    while buffer.len() + message.len() >= *buffer_size {
        *buffer_size *= 2;
    }
    if *buffer_size > MAX_WRITE_BUFFER_SIZE {
        anyhow::bail!("IPC client write buffer too big ({buffer_size}), disconnecting client");
    }
    buffer.extend_from_slice(message);
    Ok(())
}

fn make_ipc_window(
    mapped: &Mapped,
    workspace_id: Option<WorkspaceId>,
    layout: WindowLayout,
) -> swayward_ipc::Window {
    with_toplevel_role(mapped.toplevel(), |role| swayward_ipc::Window {
        id: mapped.id().get(),
        title: role.title.clone(),
        app_id: role.app_id.clone(),
        pid: mapped.credentials().map(|c| c.pid),
        workspace_id: workspace_id.map(|id| id.get()),
        is_focused: mapped.is_focused(),
        is_floating: mapped.is_floating(),
        is_urgent: mapped.is_urgent(),
        layout,
        focus_timestamp: mapped.get_focus_timestamp().map(Timestamp::from),
    })
}

impl State {
    pub(crate) fn ipc_input_changed(
        &mut self,
        change: &'static str,
        device: crate::input::IpcInputDevice,
    ) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let input = describe_input(&self.swayward, &device);
        refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
        server.send_event(Event::SwayInputChanged {
            change: change.into(),
            input,
        });
    }

    fn ipc_keyboard_input_changed(&mut self, change: &'static str) {
        let devices = self
            .swayward
            .ipc_input_devices
            .values()
            .filter(|device| device.device_type == "keyboard")
            .cloned()
            .collect::<Vec<_>>();
        for device in devices {
            self.ipc_input_changed(change, device);
        }
    }

    pub fn ipc_keyboard_layouts_changed(&mut self) {
        let Some(keyboard_layouts) = keyboard_layouts(self) else {
            if let Some(server) = &self.swayward.ipc_server {
                server
                    .event_stream_state
                    .borrow_mut()
                    .keyboard_layouts
                    .keyboard_layouts = None;
                refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
            }
            return;
        };

        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        {
            let mut event_state = server.event_stream_state.borrow_mut();
            let state = &mut event_state.keyboard_layouts;
            let event = Event::KeyboardLayoutsChanged { keyboard_layouts };
            state.apply(event.clone());
            server.send_event(event);
        }
        refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
        self.ipc_keyboard_input_changed("xkb_keymap");
    }

    pub fn ipc_refresh_keyboard_layout_index(&mut self) {
        let Some(keyboard_layouts) = keyboard_layouts(self) else {
            return;
        };
        let idx = keyboard_layouts.current_idx;

        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        {
            let mut event_state = server.event_stream_state.borrow_mut();
            let state = &mut event_state.keyboard_layouts;
            if state
                .keyboard_layouts
                .as_ref()
                .is_none_or(|layouts| layouts.current_idx == idx)
            {
                return;
            }
            let event = Event::KeyboardLayoutSwitched { idx };
            state.apply(event.clone());
            server.send_event(event);
        }
        refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
        self.ipc_keyboard_input_changed("xkb_layout");
    }

    pub(crate) fn ipc_refresh_config(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let mut query_state = server.query_state.borrow_mut();
        query_state.binding_modes = binding_modes(&self.swayward.config.borrow());
        query_state.binding_state = binding_state(&self.swayward.binding_mode);
        refresh_input_query_state(&self.swayward, &mut query_state);
    }

    pub fn ipc_refresh_layout(&mut self) {
        if self
            .swayward
            .ipc_server
            .as_ref()
            .is_none_or(|server| !server.has_event_streams())
        {
            return;
        }
        self.ipc_initialize_event_state();
    }

    fn ipc_initialize_event_state(&mut self) {
        let previous_tree =
            self.swayward.ipc_server.as_ref().and_then(|server| {
                serde_json::from_str(&server.query_state.borrow().event_tree).ok()
            });
        self.ipc_refresh_workspaces();
        if let Some(server) = &self.swayward.ipc_server {
            let mut query_state = server.query_state.borrow_mut();
            query_state.binding_state = binding_state(&self.swayward.binding_mode);
            refresh_input_query_state(&self.swayward, &mut query_state);
            let ipc_outputs = self.backend.ipc_outputs().lock().unwrap().clone();
            refresh_query_state(
                &self.swayward.layout,
                &self.swayward.global_space,
                &self.swayward.output_power,
                &ipc_outputs,
                &self.swayward.marks_by_window,
                &self.swayward.marks_by_container,
                &mut query_state,
            );
            query_state.event_tree = query_state.tree.clone();
        }
        self.ipc_refresh_windows(previous_tree.as_ref());
        self.ipc_refresh_overview();
    }

    fn ipc_refresh_workspaces(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let _span = tracy_client::span!("State::ipc_refresh_workspaces");

        let previous_tree =
            serde_json::from_str::<swayward_ipc::Node>(&server.query_state.borrow().event_tree)
                .ok();
        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.workspaces;

        let mut events = Vec::new();
        let layout = &self.swayward.layout;
        let focused_ws_id = layout.active_workspace().map(|ws| ws.id().get());

        let current_tree = crate::ipc::tree::describe_tree(
            layout,
            &self.swayward.global_space,
            &self.swayward.marks_by_window,
            &self.swayward.marks_by_container,
        );
        let old_focused = state
            .workspaces
            .values()
            .find(|workspace| workspace.is_focused)
            .cloned();
        let old_focused_node = old_focused
            .as_ref()
            .and_then(|workspace| {
                previous_tree
                    .as_ref()
                    .and_then(|tree| find_workspace_by_id(tree, workspace.id))
            })
            .cloned()
            .map(|mut workspace| {
                workspace.focused = false;
                Box::new(workspace)
            });

        // Check for workspace changes.
        let mut seen = HashSet::new();
        let mut need_workspaces_changed = false;
        for (mon, ws_idx, ws) in layout.workspaces() {
            let id = ws.id().get();
            let Some(current_node) = find_workspace_by_id(&current_tree, id) else {
                continue;
            };
            seen.insert(id);

            let Some(ipc_ws) = state.workspaces.get(&id) else {
                let mut current = current_node.clone();
                let focused = Some(id) == focused_ws_id;
                current.focused = false;
                events.push(Event::WorkspaceInitialized {
                    current: Box::new(current),
                });
                if focused {
                    let mut current = current_node.clone();
                    current.focused = true;
                    events.push(Event::WorkspaceFocusChanged {
                        old: old_focused_node.clone(),
                        current: Box::new(current),
                    });
                }
                need_workspaces_changed = true;
                continue;
            };

            let output_name = mon.map(|mon| mon.output_name());
            if ipc_ws.name != ws.sway_name() {
                if let Some(current) = find_workspace_by_id(&current_tree, id).cloned() {
                    events.push(Event::WorkspaceRenamed {
                        current: Box::new(current),
                    });
                }
                need_workspaces_changed = true;
            } else if ipc_ws.output.as_ref() != output_name {
                events.push(Event::WorkspaceMoved {
                    current: Box::new(current_node.clone()),
                });
                need_workspaces_changed = true;
            }

            let active_window_id = ws.active_window().map(|win| win.id().get());
            if ipc_ws.active_window_id != active_window_id {
                events.push(Event::WorkspaceActiveWindowChanged {
                    workspace_id: id,
                    active_window_id,
                });
            }

            // Check if this workspace urgent state changed.
            let urgent = ws.is_urgent();
            if urgent != ipc_ws.is_urgent {
                let mut current = current_node.clone();
                current.urgent = urgent;
                events.push(Event::WorkspaceUrgencyChanged {
                    id,
                    current: Box::new(current),
                });
            }

            // Check if this workspace became focused.
            let is_focused = Some(id) == focused_ws_id;
            if is_focused && !ipc_ws.is_focused {
                if let Some(mut current) = find_workspace_by_id(&current_tree, id).cloned() {
                    current.focused = true;
                    events.push(Event::WorkspaceFocusChanged {
                        old: old_focused_node.clone(),
                        current: Box::new(current),
                    });
                }
                state.apply(Event::WorkspaceActivated { id, focused: true });
                continue;
            }

            // Check if this workspace became active.
            let is_active = mon.is_some_and(|mon| mon.active_workspace_idx() == ws_idx);
            if is_active && !ipc_ws.is_active {
                events.push(Event::WorkspaceActivated { id, focused: false });
            }
        }

        if old_focused.is_some_and(|workspace| !seen.contains(&workspace.id)) {
            events.retain(|event| !matches!(event, Event::WorkspaceFocusChanged { .. }));
            if let Some(id) = focused_ws_id {
                if let Some(mut current) = find_workspace_by_id(&current_tree, id).cloned() {
                    current.focused = true;
                    events.push(Event::WorkspaceFocusChanged {
                        old: old_focused_node.clone(),
                        current: Box::new(current),
                    });
                }
            }
        }

        // Check if any workspaces were removed.
        for workspace in state
            .workspaces
            .values()
            .filter(|workspace| !seen.contains(&workspace.id))
        {
            if let Some(mut current) = previous_tree
                .as_ref()
                .and_then(|tree| find_workspace_by_id(tree, workspace.id))
                .cloned()
            {
                current.nodes.clear();
                current.floating_nodes.clear();
                current.focus.clear();
                if let swayward_ipc::NodeProperties::Workspace(properties) = &mut current.properties
                {
                    properties.representation = None;
                }
                current.focused = false;
                events.push(Event::WorkspaceEmptied {
                    current: Box::new(current),
                });
            }
            need_workspaces_changed = true;
        }

        if need_workspaces_changed {
            let sway_events = events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        Event::WorkspaceInitialized { .. }
                            | Event::WorkspaceRenamed { .. }
                            | Event::WorkspaceMoved { .. }
                            | Event::WorkspaceFocusChanged { .. }
                            | Event::WorkspaceUrgencyChanged { .. }
                            | Event::WorkspaceEmptied { .. }
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            events.clear();

            let workspaces = layout
                .workspaces()
                .filter_map(|(mon, ws_idx, ws)| {
                    let id = ws.id().get();
                    find_workspace_by_id(&current_tree, id).map(|_| Workspace {
                        id,
                        idx: u8::try_from(ws_idx + 1).unwrap_or(u8::MAX),
                        name: ws.sway_name(),
                        output: mon.map(|mon| mon.output_name().clone()),
                        is_urgent: ws.is_urgent(),
                        is_active: mon.is_some_and(|mon| mon.active_workspace_idx() == ws_idx),
                        is_focused: Some(id) == focused_ws_id,
                        active_window_id: ws.active_window().map(|win| win.id().get()),
                    })
                })
                .collect();

            state.apply(Event::WorkspacesChanged { workspaces });
            events.extend(sway_events);
        }

        for event in events {
            state.apply(event.clone());
            server.send_event(event);
        }
    }

    fn ipc_refresh_windows(&mut self, previous_tree: Option<&serde_json::Value>) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let _span = tracy_client::span!("State::ipc_refresh_windows");

        let current_tree = serde_json::to_value(crate::ipc::tree::describe_tree(
            &self.swayward.layout,
            &self.swayward.global_space,
            &self.swayward.marks_by_window,
            &self.swayward.marks_by_container,
        ))
        .unwrap_or_default();
        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.windows;

        let mut events = Vec::new();
        let layout = &self.swayward.layout;

        let mut batch_change_layouts: Vec<(u64, WindowLayout)> = Vec::new();

        // Check for window changes.
        let mut seen = HashSet::new();
        let mut focused_id = None;
        layout.with_windows(|mapped, _, ws_id, window_layout| {
            let id = mapped.id().get();
            seen.insert(id);

            let node_id = crate::ipc::tree::window_id(mapped.id());
            let current_node = find_node_by_id(&current_tree, node_id).cloned();
            let is_focused = mapped.is_focused();
            if is_focused {
                focused_id = Some(id);
            }

            let previous_node = previous_tree.and_then(|tree| find_node_by_id(tree, node_id));
            let Some(ipc_win) = state.windows.get(&id) else {
                if let Some(mut container) = current_node.clone() {
                    // Sway emits the map event before the independent seat-focus transition.
                    container["focused"] = false.into();
                    events.push(Event::SwayWindowChanged {
                        change: "new".into(),
                        container,
                    });
                }
                let window = make_ipc_window(mapped, ws_id, window_layout);
                events.push(Event::WindowOpenedOrChanged {
                    window: window.clone(),
                });
                if window.is_focused {
                    if let Some(container) = current_node {
                        events.push(Event::SwayWindowChanged {
                            change: "focus".into(),
                            container,
                        });
                    }
                    events.push(Event::WindowFocusChanged { id: Some(id) });
                }
                return;
            };

            let workspace_id = ws_id.map(|id| id.get());
            let moved = ipc_win.workspace_id != workspace_id;
            let shown_from_scratchpad =
                moved && previous_node.is_some_and(|node| node["scratchpad_state"] == "fresh");
            // root_scratchpad_remove_container emits `move` (sway/tree/root.c:150-154).
            let left_scratchpad =
                previous_node
                    .zip(current_node.as_ref())
                    .is_some_and(|(old, current)| {
                        old["scratchpad_state"] == "fresh" && current["scratchpad_state"] == "none"
                    });
            let floating_changed = ipc_win.is_floating != mapped.is_floating();
            let sway_floating_changed = previous_node
                .zip(current_node.as_ref())
                .is_some_and(|(old, current)| old["type"] != current["type"]);
            let title_changed =
                with_toplevel_role(mapped.toplevel(), |role| ipc_win.title != role.title);
            let fullscreen_changed = previous_node
                .zip(current_node.as_ref())
                .is_some_and(|(old, current)| old["fullscreen_mode"] != current["fullscreen_mode"]);
            let marks_changed = previous_node
                .zip(current_node.as_ref())
                .is_some_and(|(old, current)| old["marks"] != current["marks"]);

            if let Some(container) = current_node.clone() {
                for change in [
                    (moved || left_scratchpad).then_some("move"),
                    sway_floating_changed.then_some("floating"),
                    title_changed.then_some("title"),
                    fullscreen_changed.then_some("fullscreen_mode"),
                    marks_changed.then_some("mark"),
                ]
                .into_iter()
                .flatten()
                {
                    events.push(Event::SwayWindowChanged {
                        change: change.into(),
                        container: container.clone(),
                    });
                }
            }
            if moved || floating_changed || title_changed {
                events.push(Event::WindowOpenedOrChanged {
                    window: make_ipc_window(mapped, ws_id, window_layout.clone()),
                });
                if !shown_from_scratchpad {
                    return;
                }
                if let Some(container) = current_node.clone() {
                    events.push(Event::SwayWindowChanged {
                        change: "focus".into(),
                        container,
                    });
                }
            }

            if ipc_win.layout != window_layout {
                batch_change_layouts.push((id, window_layout));
            }

            if mapped.is_focused() && !ipc_win.is_focused {
                if let Some(container) =
                    find_node_by_id(&current_tree, crate::ipc::tree::window_id(mapped.id()))
                        .cloned()
                {
                    events.push(Event::SwayWindowChanged {
                        change: "focus".into(),
                        container,
                    });
                }
                events.push(Event::WindowFocusChanged { id: Some(id) });
            }

            let focus_timestamp = mapped.get_focus_timestamp().map(Timestamp::from);
            if focus_timestamp != ipc_win.focus_timestamp {
                events.push(Event::WindowFocusTimestampChanged {
                    id,
                    focus_timestamp,
                });
            }

            let urgent = mapped.is_urgent();
            if urgent != ipc_win.is_urgent {
                if let Some(container) =
                    find_node_by_id(&current_tree, crate::ipc::tree::window_id(mapped.id()))
                        .cloned()
                {
                    events.push(Event::SwayWindowChanged {
                        change: "urgent".into(),
                        container,
                    });
                }
                events.push(Event::WindowUrgencyChanged { id, urgent })
            }
        });

        // It might make sense to push layout changes after closed windows (since windows about to
        // be closed will occupy the same column/tile positions as the window that moved into this
        // vacated space), but also we are already pushing some layout changes in
        // WindowOpenedOrChanged above, meaning that the receiving end has to handle this case
        // anyway.
        if !batch_change_layouts.is_empty() {
            events.push(Event::WindowLayoutsChanged {
                changes: batch_change_layouts,
            });
        }

        // Check for closed windows.
        let mut ipc_focused_id = None;
        for (id, ipc_win) in &state.windows {
            if !seen.contains(id) {
                if let Some(mut container) = previous_tree
                    .and_then(|tree| {
                        find_node_by_id(tree, crate::ipc::tree::window_id_from_raw(*id))
                    })
                    .cloned()
                {
                    container["foreign_toplevel_identifier"] = serde_json::Value::Null;
                    events.push(Event::SwayWindowChanged {
                        change: "close".into(),
                        container,
                    });
                }
                events.push(Event::WindowClosed { id: *id });
            }

            if ipc_win.is_focused {
                ipc_focused_id = Some(id);
            }
        }

        // Extra check for focus becoming None, since the checks above only work for focus becoming
        // a different window.
        // Session lock temporarily moves keyboard focus away from the layout,
        // but sway's container focus does not change. Keep the IPC baseline
        // intact so restoring keyboard focus on unlock does not look like a
        // new window focus transition.
        if focused_id.is_none() && ipc_focused_id.is_some() && !self.swayward.is_locked() {
            events.push(Event::WindowFocusChanged { id: None });
        }

        for event in events {
            state.apply(event.clone());
            server.send_event(event);
        }
    }

    pub fn ipc_refresh_overview(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.overview;
        let is_open = self.swayward.layout.is_overview_open();

        if state.is_open == is_open {
            return;
        }

        let event = Event::OverviewOpenedOrClosed { is_open };
        state.apply(event.clone());
        server.send_event(event);
    }

    pub fn ipc_refresh_casts(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let _span = tracy_client::span!("State::ipc_refresh_casts");

        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.casts;

        let mut events = Vec::new();
        let mut seen = HashSet::new();

        // Check PipeWire screencasts.
        #[cfg(feature = "xdp-gnome-screencast")]
        {
            // Check pending dynamic casts.
            for pending in &self.swayward.casting.pending_dynamic_casts {
                let stream_id = pending.stream_id.get();
                seen.insert(stream_id);

                // Pending dynamic casts don't change any properties, so we only need to check if
                // it's missing from the state.
                if !state.casts.contains_key(&stream_id) {
                    let cast = swayward_ipc::Cast {
                        session_id: pending.session_id.get(),
                        stream_id,
                        kind: swayward_ipc::CastKind::PipeWire,
                        target: swayward_ipc::CastTarget::Nothing {},
                        is_dynamic_target: true,
                        is_active: false,
                        pid: None,
                        pw_node_id: None,
                    };
                    events.push(Event::CastStartedOrChanged { cast });
                }
            }

            // Check active casts.
            for cast in &self.swayward.casting.casts {
                let stream_id = cast.stream_id.get();
                seen.insert(stream_id);

                let pw_node_id = cast.node_id();
                if state.casts.get(&stream_id).is_none_or(|existing| {
                    // Only these properties can change.
                    existing.is_active != cast.is_active()
                        || !cast.target.matches(&existing.target)
                        || existing.pw_node_id != pw_node_id
                }) {
                    let cast = swayward_ipc::Cast {
                        session_id: cast.session_id.get(),
                        stream_id,
                        kind: swayward_ipc::CastKind::PipeWire,
                        target: cast.target.make_ipc(),
                        is_dynamic_target: cast.dynamic_target,
                        is_active: cast.is_active(),
                        pid: None,
                        pw_node_id,
                    };
                    events.push(Event::CastStartedOrChanged { cast });
                }
            }
        }

        // Check screencopy casts.
        //
        // First, clear expired casts. Ideally we'd have a deadline timer, but our 1 second frame
        // callback timer calls refresh regularly, so that's fine as is.
        self.swayward.screencopy_state.clear_expired_casts();

        for queue in self.swayward.screencopy_state.queues() {
            if let Some(cast_info) = queue.cast() {
                let stream_id = cast_info.stream_id.get();
                seen.insert(stream_id);

                if state.casts.get(&stream_id).is_none_or(|existing| {
                    // Only this property can change.
                    match &existing.target {
                        swayward_ipc::CastTarget::Output { name } => *name != cast_info.output_name,
                        _ => true,
                    }
                }) {
                    let cast = swayward_ipc::Cast {
                        session_id: cast_info.session_id.get(),
                        stream_id,
                        kind: swayward_ipc::CastKind::WlrScreencopy,
                        target: swayward_ipc::CastTarget::Output {
                            name: cast_info.output_name.clone(),
                        },
                        is_dynamic_target: false,
                        is_active: true,
                        pid: queue.credentials().map(|creds| creds.pid),
                        pw_node_id: None,
                    };
                    events.push(Event::CastStartedOrChanged { cast });
                }
            }
        }

        // Check for stopped casts.
        for stream_id in state.casts.keys() {
            if !seen.contains(stream_id) {
                events.push(Event::CastStopped {
                    stream_id: *stream_id,
                });
            }
        }

        for event in events {
            state.apply(event.clone());
            server.send_event(event);
        }
    }

    pub fn ipc_config_loaded(&mut self, failed: bool) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let mut state = server.event_stream_state.borrow_mut();

        let event = Event::ConfigLoaded { failed };
        state.apply(event.clone());
        server.send_event(event);
        if !failed {
            server.send_event(Event::WorkspacesChanged {
                workspaces: state.workspaces.workspaces.values().cloned().collect(),
            });
        }
    }

    pub fn ipc_screenshot_taken(&mut self, path: Option<String>) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let mut state = server.event_stream_state.borrow_mut();

        let event = Event::ScreenshotCaptured { path };
        state.apply(event.clone());
        server.send_event(event);
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::FileTypeExt as _;

    use super::*;

    #[test]
    fn default_socket_path_uses_runtime_dir_and_falls_back_to_tmp() {
        let runtime = PathBuf::from("/run/user/1234");
        assert_eq!(socket_dir_from(Some(runtime.clone())), runtime);
        assert_eq!(socket_dir_from(None), env::temp_dir());
        assert_eq!(
            default_socket_path(runtime, OsStr::new("wayland-7"), 42, 3),
            PathBuf::from("/run/user/1234/swayward-ipc.wayland-7.42.3.sock")
        );
    }

    #[test]
    fn socket_path_honors_only_a_nonexistent_swaysock() {
        let root = std::env::temp_dir().join(format!("swayward-socket-path-{}", process::id()));
        let requested = root.join("requested.sock");
        let fallback = root.join("fallback.sock");
        std::fs::create_dir_all(&root).unwrap();

        assert_eq!(
            select_socket_path(fallback.clone(), Some(requested.clone())),
            requested
        );
        let occupied = UnixListener::bind(&requested).unwrap();
        assert_eq!(
            select_socket_path(fallback.clone(), Some(requested.clone())),
            fallback
        );
        assert!(UnixStream::connect(&requested).is_ok());

        drop(occupied);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn binding_removes_a_stale_socket_and_drop_cleans_up() {
        let root = std::env::temp_dir().join(format!("swayward-stale-socket-{}", process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("ipc.sock");
        drop(UnixListener::bind(&path).unwrap());

        let event_loop = calloop::EventLoop::<State>::try_new().unwrap();
        let server = IpcServer::start_at(&event_loop.handle(), Some(path.clone())).unwrap();
        assert!(path.metadata().unwrap().file_type().is_socket());
        drop(server);
        assert!(!path.exists());

        let server = IpcServer::start_at(&event_loop.handle(), Some(path.clone())).unwrap();
        assert!(path.metadata().unwrap().file_type().is_socket());
        drop(server);
        assert!(!path.exists());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn overlong_socket_path_fails_instead_of_truncating() {
        let mut root = std::env::temp_dir();
        for _ in 0..4 {
            root.push("x".repeat(30));
        }
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("ipc.sock");
        assert!(bind_listener(&path).is_err());
        assert!(!path.exists());
        std::fs::remove_dir_all(std::env::temp_dir().join("x".repeat(30))).unwrap();
    }

    #[test]
    fn write_buffer_doubles_and_rejects_the_first_size_above_four_mb() {
        let mut buffer = Vec::new();
        let mut size = INITIAL_WRITE_BUFFER_SIZE;
        queue_ipc_message(&mut buffer, &mut size, &[0; 100]).unwrap();
        assert_eq!(size, 128);
        queue_ipc_message(&mut buffer, &mut size, &[0; 28]).unwrap();
        assert_eq!(size, 256);

        buffer.resize(2_097_151, 0);
        size = 2_097_152;
        assert!(queue_ipc_message(&mut buffer, &mut size, &[0]).is_err());
        assert_eq!(size, 4_194_304);
    }
}

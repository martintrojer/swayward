use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{env, io, process};

use anyhow::Context;
use async_channel::{Receiver, Sender};
use calloop::io::Async;
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
use swayward_ipc::wire::{decode_header, encode, CLOSE_SENTINEL, HEADER_SIZE};
use swayward_ipc::{
    CommandOutcome, KeyboardLayouts, MessageType, Timestamp, Version, WindowLayout,
};

use crate::ipc::tree::{describe_outputs, describe_tree, describe_workspaces};
use crate::layout::workspace::WorkspaceId;
use crate::swayward::State;
use crate::utils::{version, with_toplevel_role};
use crate::window::Mapped;

const INITIAL_WRITE_BUFFER_SIZE: usize = 128;
const MAX_WRITE_BUFFER_SIZE: usize = 4_000_000;
const MAX_PAYLOAD_SIZE: u32 = 16 * 1024 * 1024;
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
    config: String,
    loaded_config_file_name: String,
    tree: String,
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
    query_state: Rc<RefCell<QueryState>>,
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
    input: String,
    reply: Sender<Vec<CommandOutcome>>,
}

impl IpcServer {
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
                    let outcome = crate::command::execute(state, &request.input);
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

    #[cfg(test)]
    pub(crate) fn event_stream_count(&self) -> usize {
        self.event_streams.borrow().len()
    }

    #[cfg(test)]
    pub(crate) fn set_loaded_config_file_name(&self, path: String) {
        self.query_state.borrow_mut().loaded_config_file_name = path;
    }

    pub(crate) fn send_event(&self, event: Event) {
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

    let Some(server) = &state.swayward.ipc_server else {
        return;
    };
    let mut query_state = server.query_state.borrow_mut();
    query_state.config = config_reply(&state.swayward.config.borrow());
    query_state.binding_modes = binding_modes(&state.swayward.config.borrow());
    query_state.binding_state = binding_state(&state.swayward.binding_mode);
    refresh_input_query_state(&state.swayward, &mut query_state);
    refresh_query_state(
        &state.swayward.layout,
        &state.swayward.global_space,
        &state.swayward.marks_by_window,
        &state.swayward.marks_by_container,
        &mut query_state,
    );
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
        let (msg_type, payload_len) = decode_header(&header).inspect_err(|_| {
            warn!(
                bytes = %header.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" "),
                ascii = %String::from_utf8_lossy(&header),
                "invalid IPC header"
            );
        })?;
        debug!(?msg_type, payload_len, "received IPC request");
        if payload_len > MAX_PAYLOAD_SIZE {
            anyhow::bail!("IPC payload exceeds {MAX_PAYLOAD_SIZE} bytes");
        }
        let mut payload = vec![0; payload_len as usize];
        read.read_exact(&mut payload)
            .await
            .context("error reading IPC payload")?;

        if msg_type == MessageType::Subscribe {
            let subscriptions: Vec<String> = match serde_json::from_slice(&payload) {
                Ok(subscriptions) => subscriptions,
                Err(_) => {
                    write
                        .write_all(&encode(msg_type, r#"{"success": false}"#))
                        .await
                        .context("error writing IPC reply")?;
                    continue;
                }
            };
            if !subscriptions.iter().all(|event| {
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
            }) {
                write
                    .write_all(&encode(msg_type, r#"{"success": false}"#))
                    .await
                    .context("error writing IPC reply")?;
                continue;
            }
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
            let (events_tx, events_rx) = async_channel::bounded(4096);
            let (disconnect_tx, disconnect_rx) = async_channel::bounded(1);
            ctx.event_streams.borrow_mut().push(EventStreamSender {
                events: events_tx,
                disconnect: disconnect_tx,
            });
            return handle_event_stream_client(EventStreamClient {
                events: events_rx,
                disconnect: disconnect_rx,
                read: Box::new(read),
                write: Box::new(write),
                subscriptions: subscriptions.into_iter().collect(),
                query_state: ctx.query_state.clone(),
            })
            .await;
        }

        let reply = dispatch(&ctx, msg_type, &payload).await;
        write
            .write_all(&encode(msg_type, &reply))
            .await
            .context("error writing IPC reply")?;
    }
}

async fn dispatch(ctx: &ClientCtx, msg_type: MessageType, payload: &[u8]) -> String {
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
        MessageType::GetConfig => ctx.query_state.borrow().config.clone(),
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
            if ctx.commands.send(CommandRequest { input, reply }).is_err() {
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
        MessageType::GetBarConfig => r#"{"success":false,"error":"No bar with that ID"}"#.into(),
        MessageType::SendTick => {
            let payload = String::from_utf8_lossy(payload).into_owned();
            for stream in ctx.event_streams.borrow_mut().iter_mut() {
                let _ = stream.events.try_send(Event::Tick {
                    payload: payload.clone(),
                    first: false,
                });
            }
            r#"{"success":true}"#.into()
        }
        _ => r#"{"success":false,"error":"not implemented"}"#.into(),
    }
}

fn config_reply(config: &swayward_config::Config) -> String {
    serde_json::json!({"config": config.raw_config}).to_string()
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

pub(crate) fn keyboard_layouts(state: &mut State) -> KeyboardLayouts {
    let keyboard = state.swayward.seat.get_keyboard().unwrap();
    keyboard.with_xkb_state(state, |context| {
        let xkb = context.xkb().lock().unwrap();
        KeyboardLayouts {
            names: xkb
                .layouts()
                .map(|layout| xkb.layout_name(layout).to_owned())
                .collect(),
            current_idx: xkb.active_layout().0,
        }
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
    if device.device_type == "keyboard" {
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

fn refresh_query_state(
    layout: &crate::layout::Layout<Mapped>,
    global_space: &smithay::desktop::Space<smithay::desktop::Window>,
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
    state.tree =
        serde_json::to_string(&describe_tree(layout, global_space, marks, container_marks))
            .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    state.workspaces = serde_json::to_string(&describe_workspaces(layout, global_space))
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    state.outputs = serde_json::to_string(&describe_outputs(layout, global_space))
        .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
    let mut all_marks = marks.values().flatten().collect::<Vec<_>>();
    all_marks.sort();
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
        query_state,
    } = client;

    enum StreamInput {
        Header([u8; HEADER_SIZE]),
        Event(Event),
        Written(io::Result<usize>),
    }

    let mut write_buffer = Vec::new();
    let mut write_buffer_size = INITIAL_WRITE_BUFFER_SIZE;
    loop {
        let mut header = [0; HEADER_SIZE];
        let write_ready = if write_buffer.is_empty() {
            futures_util::future::Either::Left(futures_util::future::pending())
        } else {
            futures_util::future::Either::Right(write.write(&write_buffer))
        };
        let input = select_biased! {
            _ = disconnect.recv().fuse() => return Ok(()),
            result = read.read_exact(&mut header).fuse() => match result {
                Ok(_) if &header == CLOSE_SENTINEL => return Ok(()),
                Ok(_) => StreamInput::Header(header),
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(error) => return Err(error).context("error reading IPC event stream"),
            },
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
            StreamInput::Header(header) => {
                let (msg_type, payload_len) = decode_header(&header)?;
                if msg_type != MessageType::Subscribe || payload_len > MAX_PAYLOAD_SIZE {
                    anyhow::bail!("unexpected request on IPC event stream");
                }
                let mut payload = vec![0; payload_len as usize];
                read.read_exact(&mut payload)
                    .await
                    .context("error reading IPC subscription payload")?;
                let requested: Vec<String> = serde_json::from_slice(&payload)
                    .context("error parsing IPC subscription payload")?;
                if !requested.iter().all(|event| {
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
                }) {
                    queue_ipc_message(
                        &mut write_buffer,
                        &mut write_buffer_size,
                        &encode(msg_type, r#"{"success": false}"#),
                    )?;
                    continue;
                }
                subscriptions.extend(requested);
                queue_ipc_message(
                    &mut write_buffer,
                    &mut write_buffer_size,
                    &encode(msg_type, r#"{"success": true}"#),
                )?;
                continue;
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
            Event::WorkspaceFocusChanged { old, current }
                if subscriptions.contains("workspace") =>
            {
                (
                    1 << 31,
                    serde_json::json!({"change":"focus","old":old,"current":current}),
                )
            }
            Event::WorkspaceActiveWindowChanged { .. } => continue,
            Event::WorkspacesChanged { .. }
            | Event::WorkspaceActivated { .. }
            | Event::WorkspaceUrgencyChanged { .. }
                if subscriptions.contains("workspace") =>
            {
                (
                    1 << 31,
                    serde_json::json!({"change":"reload","old":null,"current":null}),
                )
            }
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
        let keyboard_layouts = keyboard_layouts(self);

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
        let keyboard = self.swayward.seat.get_keyboard().unwrap();
        let idx = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap();
            xkb.active_layout().0
        });

        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        {
            let mut event_state = server.event_stream_state.borrow_mut();
            let state = &mut event_state.keyboard_layouts;
            if state.keyboard_layouts.as_ref().unwrap().current_idx == idx {
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
        query_state.config = config_reply(&self.swayward.config.borrow());
        query_state.binding_modes = binding_modes(&self.swayward.config.borrow());
        query_state.binding_state = binding_state(&self.swayward.binding_mode);
        refresh_input_query_state(&self.swayward, &mut query_state);
    }

    pub fn ipc_refresh_layout(&mut self) {
        let previous_tree = self
            .swayward
            .ipc_server
            .as_ref()
            .and_then(|server| serde_json::from_str(&server.query_state.borrow().tree).ok());
        self.ipc_refresh_workspaces();
        if let Some(server) = &self.swayward.ipc_server {
            let mut query_state = server.query_state.borrow_mut();
            query_state.binding_state = binding_state(&self.swayward.binding_mode);
            refresh_input_query_state(&self.swayward, &mut query_state);
            refresh_query_state(
                &self.swayward.layout,
                &self.swayward.global_space,
                &self.swayward.marks_by_window,
                &self.swayward.marks_by_container,
                &mut query_state,
            );
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
            serde_json::from_str::<swayward_ipc::Node>(&server.query_state.borrow().tree).ok();
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
            .map(Box::new);

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
                    let current = current_node.clone();
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
                events.push(Event::WorkspaceUrgencyChanged { id, urgent });
            }

            // Check if this workspace became focused.
            let is_focused = Some(id) == focused_ws_id;
            if is_focused && !ipc_ws.is_focused {
                if let Some(current) = find_workspace_by_id(&current_tree, id).cloned() {
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
                if let Some(current) = find_workspace_by_id(&current_tree, id).cloned() {
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
                            | Event::WorkspaceFocusChanged { .. }
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
                    moved.then_some("move"),
                    sway_floating_changed.then_some("floating"),
                    title_changed.then_some("title"),
                    fullscreen_changed.then_some("fullscreen_mode"),
                    marks_changed.then_some("mark"),
                ]
                .into_iter()
                .flatten()
                {
                    let container = if change == "move" && shown_from_scratchpad {
                        serde_json::json!({"nodes":[container]})
                    } else {
                        container.clone()
                    };
                    events.push(Event::SwayWindowChanged {
                        change: change.into(),
                        container,
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
        if focused_id.is_none() && ipc_focused_id.is_some() {
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

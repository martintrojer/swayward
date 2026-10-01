use directories::BaseDirs;

use super::requests::dispatch;

#[path = "transport/events.rs"]
mod events;
use events::{sway_event, SwayEventType};

use super::*;

struct EventStreamClient {
    pub(super) registration: EventStreamRegistration,
    pub(super) events: Receiver<Event>,
    pub(super) disconnect: Receiver<()>,
    pub(super) read: Box<dyn AsyncRead + Unpin>,
    pub(super) write: Box<dyn AsyncWrite + Unpin>,
    pub(super) subscriptions: HashSet<String>,
    pub(super) ctx: ClientCtx,
}

#[derive(Clone)]
pub(super) struct ClientCtx {
    pub(super) query_state: Rc<RefCell<QueryState>>,
    pub(super) event_streams: Rc<RefCell<Vec<EventStreamSender>>>,
    pub(super) next_event_stream_id: Rc<Cell<u64>>,
    pub(super) commands: channel::Sender<CommandRequest>,
}

/// Removes a subscriber's sender when its client future exits, whether by a
/// clean close, EOF or an error, instead of waiting for the next event to
/// discover the dead channel.
pub(super) struct EventStreamRegistration {
    id: u64,
    event_streams: Rc<RefCell<Vec<EventStreamSender>>>,
}

impl Drop for EventStreamRegistration {
    fn drop(&mut self) {
        self.event_streams
            .borrow_mut()
            .retain(|stream| stream.id != self.id);
    }
}

pub(super) struct EventStreamSender {
    pub(super) id: u64,
    pub(super) events: Sender<Event>,
    pub(super) disconnect: Sender<()>,
}

pub(super) struct CommandRequest {
    pub(super) kind: RequestKind,
    /// The serialised reply.
    pub(super) reply: Sender<Vec<u8>>,
}

pub(super) enum RequestKind {
    Command(String),
    /// Serialise one query reply from live compositor state.
    Query(MessageType),
    /// Establish an event diff baseline immediately before subscribing.
    RefreshEventState,
}

pub(super) fn socket_dir() -> PathBuf {
    socket_dir_from(BaseDirs::new().and_then(|dirs| dirs.runtime_dir().map(PathBuf::from)))
}

pub(super) fn socket_dir_from(runtime_dir: Option<PathBuf>) -> PathBuf {
    runtime_dir.unwrap_or_else(env::temp_dir)
}

pub(super) fn default_socket_path(
    dir: PathBuf,
    wayland_socket_name: &OsStr,
    pid: u32,
    id: u64,
) -> PathBuf {
    dir.join(format!(
        "swayward-ipc.{}.{pid}.{id}.sock",
        wayland_socket_name.to_string_lossy()
    ))
}

pub(super) fn select_socket_path(default: PathBuf, requested: Option<PathBuf>) -> PathBuf {
    requested.filter(|path| !path.exists()).unwrap_or(default)
}

pub(super) fn bind_listener(path: &std::path::Path) -> anyhow::Result<UnixListener> {
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

pub(super) fn on_new_ipc_client(state: &mut State, stream: UnixStream) {
    let Some(server) = &state.swayward.ipc_server else {
        return;
    };
    let ctx = ClientCtx {
        query_state: server.query_state.clone(),
        event_streams: server.event_streams.clone(),
        next_event_stream_id: server.next_event_stream_id.clone(),
        commands: server.commands.clone(),
    };
    let stream = match state.swayward.event_loop.adapt_io(stream) {
        Ok(stream) => stream,
        Err(err) => {
            warn!("error making IPC stream async: {err:?}");
            return;
        }
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

pub(super) async fn handle_client(
    ctx: ClientCtx,
    stream: Async<'static, UnixStream>,
) -> anyhow::Result<()> {
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
        trace!(?msg_type, raw_type, payload_len, "received IPC request");
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
            let id = ctx.next_event_stream_id.get();
            ctx.next_event_stream_id.set(id.wrapping_add(1));
            ctx.event_streams.borrow_mut().push(EventStreamSender {
                id,
                events: events_tx,
                disconnect: disconnect_tx,
            });
            let registration = EventStreamRegistration {
                id,
                event_streams: ctx.event_streams.clone(),
            };
            write
                .write_all(&encode(msg_type, r#"{"success": true}"#))
                .await
                .context("error writing IPC reply")?;

            if subscriptions.iter().any(|event| event == "tick") {
                write
                    .write_all(&swayward_ipc::wire::encode_raw(
                        SwayEventType::Tick as u32,
                        r#"{"first":true,"payload":""}"#,
                    ))
                    .await
                    .context("error writing initial tick event")?;
            }
            return handle_event_stream_client(EventStreamClient {
                registration,
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
            None if raw_type == IPC_SYNC => br#"{"success": false}"#.to_vec(),
            None => br#"{"success":false,"error":"not implemented"}"#.to_vec(),
        };
        write
            .write_all(&swayward_ipc::wire::encode_raw_bytes(raw_type, &reply))
            .await
            .context("error writing IPC reply")?;
    }
}

/// `IPC_SYNC`, `sway/include/ipc.h:20`. Not a `MessageType`: sway never
/// implemented it and only keeps the code reserved.
const IPC_SYNC: u32 = 11;

pub(super) async fn refresh_event_state(ctx: &ClientCtx) {
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

pub(super) fn parse_subscriptions(payload: &[u8]) -> Option<Vec<String>> {
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
                    | "barconfig_update"
                    | "binding"
                    | "tick"
                    | "input"
            )
        })
        .then_some(subscriptions)
}

async fn handle_event_stream_client(client: EventStreamClient) -> anyhow::Result<()> {
    let EventStreamClient {
        registration: _registration,
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
                        None if raw_type == IPC_SYNC => br#"{"success": false}"#.to_vec(),
                        None => br#"{"success":false,"error":"not implemented"}"#.to_vec(),
                    };
                    queue_ipc_message(
                        &mut write_buffer,
                        &mut write_buffer_size,
                        &swayward_ipc::wire::encode_raw_bytes(raw_type, &reply),
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
                        &encode_raw(SwayEventType::Tick as u32, r#"{"first":true,"payload":""}"#),
                    )?;
                }
                continue;
            }
            StreamInput::Read(Err(error)) => {
                return Err(error).context("error reading IPC event stream");
            }
            StreamInput::Event(event) => event,
        };
        let Some((msg_type, payload)) = sway_event(event, &query_state.borrow()) else {
            continue;
        };
        if !subscriptions.contains(msg_type.subscription_name()) {
            continue;
        }
        let msg_type = msg_type as u32;
        let payload = serde_json::to_string(&payload).context("error formatting event")?;
        let buf = swayward_ipc::wire::encode_raw(msg_type, &payload);
        queue_ipc_message(&mut write_buffer, &mut write_buffer_size, &buf)?;
    }
}

/// Append `message` to a subscriber's write buffer, growing the nominal size
/// the way sway does (`sway/sway/ipc-server.c:946-955`): double until the
/// queued bytes fit, then disconnect if the size exceeds 4,000,000. Because
/// the size only takes power-of-two multiples of 128, the first size past
/// the limit is 4,194,304, so a client is dropped once about 2 MiB is queued,
/// not 4 MB. This matches sway on purpose.
pub(super) fn queue_ipc_message(
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

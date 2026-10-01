struct ScratchDir(std::path::PathBuf);

impl ScratchDir {
    fn new(tag: &str) -> Self {
        let path = std::env::var_os("SWAYWARD_TEST_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/var/tmp"))
            .join(format!(
                "swayward-{tag}.{}.{}",
                std::process::id(),
                NEXT_TEST_SCRATCH.fetch_add(1, Ordering::Relaxed),
            ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, path: impl AsRef<std::path::Path>) -> std::path::PathBuf {
        self.0.join(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

static NEXT_TEST_SCRATCH: AtomicU64 = AtomicU64::new(0);

fn read_ipc_reply(fixture: &mut Fixture, stream: &mut UnixStream) -> (u32, String) {
    let (reply, _) = read_ipc_reply_with_remainder(fixture, stream, Vec::new());
    reply
}

fn read_ipc_reply_with_remainder(
    fixture: &mut Fixture,
    stream: &mut UnixStream,
    mut response: Vec<u8>,
) -> ((u32, String), Vec<u8>) {
    stream.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        fixture.dispatch();
        let mut buf = [0; 4096];
        match stream.read(&mut buf) {
            Ok(0) => panic!("IPC connection closed before a reply"),
            Ok(len) => response.extend_from_slice(&buf[..len]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("error reading IPC reply: {error}"),
        }
        if response.len() >= swayward_ipc::wire::HEADER_SIZE {
            let payload_len = u32::from_ne_bytes(response[6..10].try_into().unwrap()) as usize;
            if response.len() >= swayward_ipc::wire::HEADER_SIZE + payload_len {
                let msg_type = u32::from_ne_bytes(response[10..14].try_into().unwrap());
                let payload = String::from_utf8(
                    response[swayward_ipc::wire::HEADER_SIZE..][..payload_len].to_vec(),
                )
                .unwrap();
                let consumed = swayward_ipc::wire::HEADER_SIZE + payload_len;
                let remainder = response.split_off(consumed);
                return ((msg_type, payload), remainder);
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for IPC reply");
    }
}

/// Like `read_ipc_reply_with_remainder`, but returns None instead of panicking
/// when no further event arrives. Used to drain a burst whose length is the
/// thing under test.
fn try_read_ipc_reply_with_remainder(
    fixture: &mut Fixture,
    stream: &mut UnixStream,
    mut response: Vec<u8>,
) -> Option<((u32, String), Vec<u8>)> {
    stream.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_millis(200);
    loop {
        fixture.dispatch();
        let mut buf = [0; 4096];
        match stream.read(&mut buf) {
            Ok(0) => return None,
            Ok(len) => response.extend_from_slice(&buf[..len]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("error reading IPC reply: {error}"),
        }
        if response.len() >= swayward_ipc::wire::HEADER_SIZE {
            let payload_len = u32::from_ne_bytes(response[6..10].try_into().unwrap()) as usize;
            if response.len() >= swayward_ipc::wire::HEADER_SIZE + payload_len {
                let msg_type = u32::from_ne_bytes(response[10..14].try_into().unwrap());
                let payload = String::from_utf8(
                    response[swayward_ipc::wire::HEADER_SIZE..][..payload_len].to_vec(),
                )
                .unwrap();
                let consumed = swayward_ipc::wire::HEADER_SIZE + payload_len;
                let remainder = response.split_off(consumed);
                return Some(((msg_type, payload), remainder));
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
    }
}

/// Every workspace node in a GET_TREE reply, in tree order.
fn collect_workspace_nodes(node: &Value, out: &mut Vec<Value>) {
    if node["type"] == "workspace" {
        out.push(node.clone());
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node[key].as_array() {
            for child in children {
                collect_workspace_nodes(child, out);
            }
        }
    }
}

fn query_ipc(fixture: &mut Fixture, stream: &mut UnixStream, message_type: MessageType) -> Value {
    query_ipc_with_payload(fixture, stream, message_type, "")
}

fn get_tree(fixture: &mut Fixture) -> Value {
    query_fixture(fixture, MessageType::GetTree)
}

fn get_workspaces(fixture: &mut Fixture) -> Value {
    query_fixture(fixture, MessageType::GetWorkspaces)
}

fn get_outputs(fixture: &mut Fixture) -> Value {
    query_fixture(fixture, MessageType::GetOutputs)
}

fn query_fixture(fixture: &mut Fixture, message_type: MessageType) -> Value {
    let socket = fixture
        .swayward()
        .ipc_server
        .as_ref()
        .and_then(|server| server.socket_path.clone())
        .expect("fixture has no IPC server");
    let mut stream = UnixStream::connect(socket).unwrap();
    query_ipc(fixture, &mut stream, message_type)
}

fn query_ipc_with_payload(
    fixture: &mut Fixture,
    stream: &mut UnixStream,
    message_type: MessageType,
    payload: &str,
) -> Value {
    stream
        .write_all(&swayward_ipc::wire::encode(message_type, payload))
        .unwrap();
    let (reply_type, payload) = read_ipc_reply(fixture, stream);
    assert_eq!(reply_type, message_type as u32);
    serde_json::from_str(&payload).unwrap()
}

/// Two fixtures must get distinct, live sockets, and those sockets must sit
/// outside `$XDG_RUNTIME_DIR` so the nested-compositor cleanup glob cannot
/// delete them mid-test. That glob is what made the conformance runner fail
/// one file per run for a whole session.
#[test]
fn two_ipc_fixtures_get_distinct_live_sockets() {
    no_test_server_adopts_the_ambient_swaysock();

    let (_first, first_socket) = ipc_fixture();
    let (_second, second_socket) = ipc_fixture();

    assert_ne!(
        first_socket, second_socket,
        "each fixture needs its own socket path"
    );
    for socket in [&first_socket, &second_socket] {
        assert!(
            !socket.starts_with("/run/user"),
            "{} must not sit in the swept runtime directory",
            socket.display()
        );
        assert!(
            !socket.starts_with(std::env::temp_dir()),
            "{} must not sit on tmpfs",
            socket.display()
        );
    }
    for socket in [&first_socket, &second_socket] {
        UnixStream::connect(socket).unwrap_or_else(|error| {
            panic!("{} must still be connectable: {error}", socket.display())
        });
    }
}

// No test may construct its server through `IpcServer::start`, which adopts
// the ambient `$SWAYSOCK`. Reviewing a diff does not catch a reintroduced
// caller, so assert it against every Rust source under src/tests. This remains
// a helper rather than a separate test to preserve the pre-split test list.
fn no_test_server_adopts_the_ambient_swaysock() {
    let mut pending = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/tests")];
    let needle = format!("IpcServer::{}(", "start");
    let mut adopting_callers = Vec::new();

    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && std::fs::read_to_string(&path).unwrap().contains(&needle)
            {
                adopting_callers.push(path);
            }
        }
    }

    assert!(
        adopting_callers.is_empty(),
        "use IpcServer::start_at with test_socket_path(); `start` reads $SWAYSOCK \
         and can hijack the operator's live sway session; callers: {adopting_callers:?}"
    );
}

/// A private socket path for a test server.
///
/// Never let a test reach `IpcServer::start`. That derives a path under
/// `$XDG_RUNTIME_DIR` and, following sway, adopts `$SWAYSOCK` when no file
/// exists at it (`sway/sway/ipc-server.c:99-104`). A test process inherits the
/// operator's interactive `SWAYSOCK`, so if anything has unlinked that path
/// while their compositor still holds the bound listener, the test binds a
/// second listener on the name and steals every new connection from the live
/// session: `swaymsg` stops reaching the real compositor for as long as the
/// session lasts, which took an operator's display down.
///
/// The temp directory also keeps these sockets clear of the
/// `/run/user/$UID/swayward-ipc.*.sock` cleanup glob that nested-compositor
/// scripts run, which used to delete a live socket mid-test and surface as an
/// intermittent ENOENT somewhere unrelated.
fn test_socket_path() -> std::path::PathBuf {
    crate::ipc::server::test_socket_path("ipc-test.sock")
}

fn ipc_fixture() -> (Fixture, std::path::PathBuf) {
    ipc_fixture_with_config(swayward_config::Config::default())
}

/// Starts an IPC-enabled fixture on a private test socket.
///
/// Keyboard layouts are initialized here so configured and default fixtures
/// expose the same initial input state to subscribers and GET_INPUTS clients.
fn ipc_fixture_with_config(
    config: swayward_config::Config,
) -> (Fixture, std::path::PathBuf) {
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    (fixture, socket)
}

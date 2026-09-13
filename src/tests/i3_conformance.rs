//! Runner for unmodified layout tests from i3's Perl testsuite.

use std::ffi::OsStr;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::Fixture;
use crate::utils::transaction::Transaction;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

fn socket_path(kind: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "swayward-i3-{kind}-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

fn create_window(fixture: &mut Fixture, client: super::client::ClientId, request: &Value) -> u32 {
    let window = fixture.client(client).create_window();
    if let Some(app_id) = request["app_id"].as_str() {
        window.xdg_toplevel.set_app_id(app_id.to_owned());
    }
    if let Some(name) = request["name"].as_str() {
        window.set_title(name);
    }
    window.surface.id().protocol_id()
}

fn map_window(fixture: &mut Fixture, client: super::client::ClientId, surface_id: u32) -> i64 {
    let surface = fixture
        .client(client)
        .state
        .windows
        .iter()
        .find(|window| window.surface.id().protocol_id() == surface_id)
        .unwrap()
        .surface
        .clone();
    fixture.client(client).window(&surface).commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    fixture
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface_id)
                .then(|| crate::ipc::tree::window_id(mapped.id()))
        })
        .unwrap()
}

fn remove_window_for_surface(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    surface_id: u32,
) -> bool {
    let window = {
        fixture.swayward().layout.windows().find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface_id)
                .then(|| (mapped.id(), mapped.window.clone()))
        })
    };
    let Some((id, window)) = window else {
        return false;
    };
    fixture.swayward().unmark(Some(id), None);
    fixture
        .swayward()
        .layout
        .remove_window(&window, Transaction::new());
    let windows = &mut fixture.client(client).state.windows;
    if let Some(index) = windows
        .iter()
        .position(|window| window.surface.id().protocol_id() == surface_id)
    {
        windows.swap_remove(index);
    }
    true
}

fn settle_configures(fixture: &mut Fixture, client: super::client::ClientId) {
    fixture.double_roundtrip(client);
    let windows = &mut fixture.client(client).state.windows;
    for window in windows {
        let count = window.configures_received.len();
        if window.configures_looked_at == count {
            continue;
        }
        window.configures_looked_at = count;
        let Some((_, configure)) = window.configures_received.last() else {
            continue;
        };
        let size = configure.size;
        if size.0 > 0 && size.1 > 0 {
            window.set_size(size.0 as u16, size.1 as u16);
        }
        window.ack_last_and_commit();
    }
    fixture.double_roundtrip(client);
}

fn reap_closed_windows(fixture: &mut Fixture, client: super::client::ClientId) {
    fixture.double_roundtrip(client);
    let closed = fixture
        .client(client)
        .state
        .windows
        .iter()
        .filter(|window| window.close_requested)
        .map(|window| window.surface.id().protocol_id())
        .collect::<Vec<_>>();
    for surface_id in closed {
        remove_window_for_surface(fixture, client, surface_id);
    }
}

fn close_window(fixture: &mut Fixture, client: super::client::ClientId, id: i64) -> bool {
    let surface_id = fixture.swayward().layout.windows().find_map(|(_, mapped)| {
        (crate::ipc::tree::window_id(mapped.id()) == id)
            .then(|| mapped.toplevel().wl_surface().id().protocol_id())
    });
    surface_id.is_some_and(|surface_id| remove_window_for_surface(fixture, client, surface_id))
}

fn translate_config(config: &str) -> Result<swayward_config::Config, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = socket_path("config");
    std::fs::write(&path, config).map_err(|error| error.to_string())?;
    let output = Command::new(root.join("contrib/sway-to-kdl"))
        .arg(&path)
        .output()
        .map_err(|error| error.to_string());
    let _ = std::fs::remove_file(&path);
    let output = output?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().eq("manual attention: none") {
        return Err(format!("i3 config translation was incomplete:\n{stderr}"));
    }
    let translated = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    swayward_config::Config::parse_mem(&translated).map_err(|error| format!("{error:?}"))
}

fn handle_control(fixture: &mut Fixture, client: super::client::ClientId, stream: UnixStream) {
    let mut request = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut request)
        .unwrap();
    let request: Value = serde_json::from_str(&request).unwrap();
    let reply = match request["action"].as_str().unwrap() {
        "config" => match translate_config(request["config"].as_str().unwrap()) {
            Ok(config) => {
                fixture.niri_state().reload_config(Ok(config));
                json!({ "success": true })
            }
            Err(error) => json!({ "success": false, "error": error }),
        },
        "create" => json!({ "handle": create_window(fixture, client, &request) }),
        "open" => {
            let handle = create_window(fixture, client, &request);
            json!({ "id": map_window(fixture, client, handle) })
        }
        "map" => json!({
            "id": map_window(fixture, client, request["handle"].as_u64().unwrap() as u32)
        }),
        "close" => {
            json!({ "success": close_window(fixture, client, request["id"].as_i64().unwrap()) })
        }
        "focused" => json!({
            "id": fixture
                .swayward()
                .layout
                .focus()
                .map(|mapped| crate::ipc::tree::window_id(mapped.id()))
        }),
        "prepare_resize" => {
            settle_configures(fixture, client);
            json!({ "success": true })
        }
        "reap_closed" => {
            let settle = request["settle_configures"].as_bool() == Some(true);
            if settle {
                settle_configures(fixture, client);
            }
            reap_closed_windows(fixture, client);
            json!({ "success": true })
        }
        action => panic!("unknown i3 test control action: {action}"),
    };
    writeln!(&stream, "{reply}").unwrap();
}

fn run_i3_test(test: &str) {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 0.;
    config.animations.window_movement.0.off = true;
    config.animations.window_resize.anim.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();

    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start(&handle, Some(OsStr::new("i3-tests"))).unwrap();
    let ipc_socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.niri_state().ipc_refresh_layout();

    let control_path = socket_path("control");
    let control = UnixListener::bind(&control_path).unwrap();
    control.set_nonblocking(true).unwrap();

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg(root.join("tests/i3/t").join(test))
        .env("I3SOCK", &ipc_socket)
        .env("SWAYWARD_TEST_CONTROL", &control_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        fixture.dispatch();
        match control.accept() {
            Ok((stream, _)) => handle_control(&mut fixture, client, stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("test control accept failed: {error}"),
        }
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            assert!(
                status.success(),
                "i3 test {test} failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            break;
        }
        assert!(Instant::now() < deadline, "i3 test {test} timed out");
        thread::yield_now();
    }
}

/// Upstream i3 files that pass in full, one per line, comments with `#`.
///
/// A conformance slice adds a file here the moment it goes green. Keeping the
/// list in its own file rather than in this runner lets slices land in
/// parallel without editing the same Rust source.
const PASSING: &str = include_str!("../../tests/i3/passing.txt");

#[test]
fn i3_config_translation_rejects_unhandled_directives() {
    let error = translate_config("font monospace\nmystery value\n").unwrap_err();
    assert!(error.contains("manual attention: 1 directive(s)"));
    assert!(error.contains("unhandled: mystery value"));
}

fn passing_tests() -> impl Iterator<Item = &'static str> {
    PASSING
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty())
}

#[test]
fn i3_conformance_runner() {
    // `SWAYWARD_I3_TEST` selects a single file, including one with known
    // failures, so conformance findings stay executable without turning the
    // default gate red.
    if let Ok(selected) = std::env::var("SWAYWARD_I3_TEST") {
        run_i3_test(&selected);
        return;
    }

    let mut count = 0;
    for test in passing_tests() {
        run_i3_test(test);
        count += 1;
    }
    assert!(count > 0, "tests/i3/passing.txt lists no conformance files");
}

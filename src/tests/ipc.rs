//! IPC conformance tests. The empirical coverage boundary and known gaps are
//! recorded in `docs/IPC_ORACLE_COVERAGE.md`.

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use swayward_ipc::MessageType;
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::*;
use crate::ipc::tree::{describe_outputs, describe_tree, describe_workspaces};
use crate::layout::tiling_tree::{IpcNode, Layout as TreeLayout, NodeId};
use crate::layout::LayoutElement as _;

fn collect_focused_nodes(node: &swayward_ipc::Node, ids: &mut Vec<i64>) {
    if node.focused {
        ids.push(node.id);
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect_focused_nodes(child, ids);
    }
}

fn assert_same_shape(expected: &Value, actual: &Value, path: &str) {
    assert_eq!(
        json_type(expected),
        json_type(actual),
        "JSON type at {path}"
    );
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let expected_keys = expected.keys().collect::<BTreeSet<_>>();
            let actual_keys = actual.keys().collect::<BTreeSet<_>>();
            assert_eq!(expected_keys, actual_keys, "keys at {path}");
            if let Some(expected_type) = expected.get("type") {
                assert_eq!(
                    Some(expected_type),
                    actual.get("type"),
                    "node type at {path}"
                );
            }
            for (key, value) in expected {
                assert_same_shape(value, &actual[key], &format!("{path}.{key}"));
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if path.ends_with(".nodes")
                || path.ends_with(".floating_nodes")
                || matches!(path, "$workspaces" | "$outputs")
            {
                assert_eq!(expected.len(), actual.len(), "array length at {path}");
                for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                    assert_same_shape(expected, actual, &format!("{path}[{index}]"));
                }
            } else if let Some(expected) = expected.first() {
                for (index, actual) in actual.iter().enumerate() {
                    assert_same_shape(expected, actual, &format!("{path}[{index}]"));
                }
            }
        }
        _ => {}
    }
}

fn assert_event_shape(expected: &Value, actual: &Value, path: &str) {
    assert_eq!(
        json_type(expected),
        json_type(actual),
        "JSON type at {path}"
    );
    if path.ends_with(".change") {
        assert_eq!(expected, actual, "event change at {path}");
    }
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            assert_eq!(
                expected.keys().collect::<BTreeSet<_>>(),
                actual.keys().collect::<BTreeSet<_>>(),
                "keys at {path}"
            );
            for (key, value) in expected {
                assert_event_shape(value, &actual[key], &format!("{path}.{key}"));
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if let Some(expected) = expected.first() {
                for (index, actual) in actual.iter().enumerate() {
                    assert_event_shape(expected, actual, &format!("{path}[{index}]"));
                }
            }
        }
        _ => {}
    }
}

fn assert_focus_matches_fixture(expected: &Value, actual: &Value, path: &str) {
    let expected_children = expected["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(expected["floating_nodes"].as_array().unwrap());
    let actual_children = actual["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(actual["floating_nodes"].as_array().unwrap());
    let id_map = expected_children
        .zip(actual_children)
        .map(|(expected, actual)| (expected["id"].clone(), actual["id"].clone()))
        .collect::<Vec<_>>();
    let expected_focus = expected["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            id_map
                .iter()
                .find_map(|(expected, actual)| (expected == id).then_some(actual.clone()))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        expected_focus.as_slice(),
        actual["focus"].as_array().unwrap(),
        "focus at {path}"
    );

    for key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[key].as_array().unwrap())
            .enumerate()
        {
            assert_focus_matches_fixture(expected, actual, &format!("{path}.{key}[{index}]"));
        }
    }
}

fn assert_percent_value_matches_fixture(expected: &Value, actual: &Value, path: &str) {
    match (expected["percent"].as_f64(), actual["percent"].as_f64()) {
        (Some(expected), Some(actual)) => assert!(
            (expected - actual).abs() < 1e-9,
            "percent at {path}: expected {expected}, got {actual}"
        ),
        (None, None) => {}
        _ => panic!(
            "percent at {path}: expected {}, got {}",
            expected["percent"], actual["percent"]
        ),
    }
}

fn assert_rectangle_roles_match_fixture(expected: &Value, actual: &Value, path: &str) {
    if expected["type"] == "con" && expected["nodes"].as_array().unwrap().is_empty() {
        let expected_rect = &expected["rect"];
        let actual_rect = &actual["rect"];
        for role in ["window_rect", "deco_rect", "geometry"] {
            assert_eq!(
                expected[role] == *expected_rect,
                actual[role] == *actual_rect,
                "{role} outer-rect relationship at {path}"
            );
        }

        assert_eq!(
            expected["deco_rect"]["height"].as_i64().unwrap() > 0,
            actual["deco_rect"]["height"].as_i64().unwrap() > 0,
            "titlebar presence at {path}"
        );

        for dimension in ["width", "height"] {
            let expected_window = expected["window_rect"][dimension].as_i64().unwrap();
            let expected_outer = expected_rect[dimension].as_i64().unwrap();
            let actual_window = actual["window_rect"][dimension].as_i64().unwrap();
            let actual_outer = actual_rect[dimension].as_i64().unwrap();
            assert!(
                actual_window <= actual_outer,
                "window_rect {dimension} at {path}"
            );
            if expected_window < expected_outer {
                assert!(
                    actual_window < actual_outer,
                    "window_rect {dimension} at {path}"
                );
            }
        }
    }

    for key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[key].as_array().unwrap())
            .enumerate()
        {
            assert_rectangle_roles_match_fixture(
                expected,
                actual,
                &format!("{path}.{key}[{index}]"),
            );
        }
    }
}

fn assert_percent_matches_fixture(expected: &Value, actual: &Value, path: &str) {
    if expected["type"] == "con" && !expected["nodes"].as_array().unwrap().is_empty() {
        assert_percent_value_matches_fixture(expected, actual, path);
    }

    for key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[key].as_array().unwrap())
            .enumerate()
        {
            assert_percent_matches_fixture(expected, actual, &format!("{path}.{key}[{index}]"));
        }
    }

    let expected_children = expected["nodes"].as_array().unwrap();
    let actual_children = actual["nodes"].as_array().unwrap();
    let expected_sum = expected_children
        .iter()
        .map(|child| child["percent"].as_f64())
        .sum::<Option<f64>>();
    if expected_sum.is_some_and(|sum| (sum - 1.).abs() < 1e-9) {
        let actual_sum = actual_children
            .iter()
            .map(|child| child["percent"].as_f64())
            .sum::<Option<f64>>();
        assert!(
            actual_sum.is_some_and(|sum| (sum - 1.).abs() < 1e-9),
            "percent sum at {path}: got {actual_sum:?}"
        );
    }

    if expected["type"] != "con" || expected_children.is_empty() {
        assert_percent_value_matches_fixture(expected, actual, path);
    }
}

fn assert_fixture_string_values(expected: &Value, actual: &Value, path: &str) {
    for key in ["floating", "scratchpad_state"] {
        assert_eq!(expected[key], actual[key], "{key} at {path}");
    }
    for child_key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[child_key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[child_key].as_array().unwrap())
            .enumerate()
        {
            assert_fixture_string_values(expected, actual, &format!("{path}.{child_key}[{index}]"));
        }
    }
}

fn assert_tree_values_match_fixture(expected: &Value, actual: &Value, path: &str) {
    assert_eq!(expected["focused"], actual["focused"], "focused at {path}");
    if expected["type"] != "workspace" && expected["name"].is_string() {
        for key in ["layout", "orientation"] {
            assert_eq!(expected[key], actual[key], "{key} at {path}");
        }
    }
    // The sway capture used its host font, while the headless harness uses the
    // test environment's font. Keep enough tolerance for titlebar metrics, but
    // not enough for a wrong layout or unit-size placeholder rectangle.
    for key in ["x", "y", "width", "height"] {
        let expected = expected["rect"][key].as_i64().unwrap();
        let actual = actual["rect"][key].as_i64().unwrap();
        assert!(
            (expected - actual).abs() <= 10,
            "rect.{key} at {path}: expected {expected}, got {actual}"
        );
    }
    if expected["type"] != "output" || expected["name"] == "__i3" {
        assert_eq!(expected["name"], actual["name"], "name at {path}");
    }
    for child_key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[child_key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[child_key].as_array().unwrap())
            .enumerate()
        {
            assert_tree_values_match_fixture(
                expected,
                actual,
                &format!("{path}.{child_key}[{index}]"),
            );
        }
    }
}

fn assert_node_schema_appears_in_fixtures(actual: &Value, fixtures: &[Value], path: &str) {
    let actual_keys = actual.as_object().unwrap().keys().collect::<BTreeSet<_>>();
    let actual_type = &actual["type"];
    let matches = fixtures.iter().any(|fixture| {
        fixture["type"] == *actual_type
            && fixture.as_object().unwrap().keys().collect::<BTreeSet<_>>() == actual_keys
    });
    assert!(
        matches,
        "unknown {:?} key set at {path}: {actual_keys:?}",
        actual_type
    );
    for (key, children) in [
        ("nodes", &actual["nodes"]),
        ("floating_nodes", &actual["floating_nodes"]),
    ] {
        for (index, child) in children.as_array().unwrap().iter().enumerate() {
            assert_node_schema_appears_in_fixtures(
                child,
                fixtures,
                &format!("{path}.{key}[{index}]"),
            );
        }
    }
}

fn find_json_node_with_mark<'a>(value: &'a Value, mark: &str) -> Option<&'a Value> {
    if value["marks"]
        .as_array()
        .is_some_and(|marks| marks.iter().any(|value| value == mark))
    {
        return Some(value);
    }
    ["nodes", "floating_nodes"].into_iter().find_map(|key| {
        value[key]
            .as_array()?
            .iter()
            .find_map(|child| find_json_node_with_mark(child, mark))
    })
}

fn find_json_node_with_app_id<'a>(value: &'a Value, app_id: &str) -> Option<&'a Value> {
    if value["app_id"] == app_id {
        return Some(value);
    }
    ["nodes", "floating_nodes"].into_iter().find_map(|key| {
        value[key]
            .as_array()?
            .iter()
            .find_map(|child| find_json_node_with_app_id(child, app_id))
    })
}

fn find_json_node<'a>(value: &'a Value, node_type: &str, focused: bool) -> Option<&'a Value> {
    if value["type"] == node_type && (!focused || value["focused"] == true) {
        return Some(value);
    }
    ["nodes", "floating_nodes"].into_iter().find_map(|key| {
        value[key]
            .as_array()?
            .iter()
            .find_map(|child| find_json_node(child, node_type, focused))
    })
}

fn collect_fixture_nodes(value: &Value, nodes: &mut Vec<Value>) {
    nodes.push(value.clone());
    for key in ["nodes", "floating_nodes"] {
        for child in value[key].as_array().unwrap() {
            collect_fixture_nodes(child, nodes);
        }
    }
}

fn nested_live_tree() -> Value {
    let config = swayward_config::Config::parse_mem("layout { border { on; }; }").unwrap();
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    for title in ["fixture-1", "fixture-2", "fixture-3"] {
        let window = f.client(id).create_window();
        window.xdg_toplevel.set_app_id(title.into());
        window.set_title(title);
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(id);
        let window = f.client(id).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(id);
    }
    f.swayward().layout.consume_or_expel_window_left(None);
    f.swayward().layout.move_down();
    let mut stream = UnixStream::connect(socket).unwrap();
    query_ipc(&mut f, &mut stream, MessageType::GetTree)
}

fn nested_fixture_tree() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/nested_h_in_v.tree.json"
    ))
    .unwrap()
}

fn nested_representation_live_tree() -> Value {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let client = f.add_client();
    for (index, title) in ["fixture-1", "fixture-2", "fixture-3"]
        .into_iter()
        .enumerate()
    {
        if index == 2 {
            assert!(crate::command::execute(f.niri_state(), "split horizontal")[0].success);
        }
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(title.into());
        window.set_title(title);
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    let swayward = f.swayward();
    serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap()
}

fn mixed_live_tree() -> Value {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for floating in [false, true] {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }
    let swayward = f.swayward();
    serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap()
}

fn mixed_fixture_tree() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_floating.tree.json"
    ))
    .unwrap()
}

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

fn query_ipc(fixture: &mut Fixture, stream: &mut UnixStream, message_type: MessageType) -> Value {
    query_ipc_with_payload(fixture, stream, message_type, "")
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
    }
    for socket in [&first_socket, &second_socket] {
        UnixStream::connect(socket).unwrap_or_else(|error| {
            panic!("{} must still be connectable: {error}", socket.display())
        });
    }
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
    static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

    std::env::temp_dir().join(format!(
        "swayward-ipc-test.{}.{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

/// No test may construct its server through `IpcServer::start`, which adopts
/// the ambient `$SWAYSOCK`. Reviewing a diff does not catch a reintroduced
/// caller, so assert it against every Rust source in the test tree.
#[test]
fn no_test_server_adopts_the_ambient_swaysock() {
    fn scan(path: &std::path::Path, needle: &str, callers: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, needle, callers);
            } else if path.extension().is_some_and(|extension| extension == "rs")
                && std::fs::read_to_string(&path).unwrap().contains(needle)
            {
                callers.push(path);
            }
        }
    }

    // Assembled at runtime so this test does not match itself.
    let needle = format!("IpcServer::{}(", "start");
    let mut adopting_callers = Vec::new();
    scan(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests"),
        &needle,
        &mut adopting_callers,
    );
    assert!(
        adopting_callers.is_empty(),
        "use IpcServer::start_at with test_socket_path(); `start` reads $SWAYSOCK \
         and can hijack the operator's live sway session; callers: {adopting_callers:?}"
    );
}

fn ipc_fixture() -> (Fixture, std::path::PathBuf) {
    let mut fixture = Fixture::new();
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    (fixture, socket)
}

#[test]
fn ipc_refresh_without_a_seat_keyboard_does_not_panic() {
    let (mut fixture, _) = ipc_fixture();
    fixture.swayward().seat.remove_keyboard();

    fixture.niri_state().ipc_refresh_keyboard_layout_index();
    fixture.niri_state().ipc_keyboard_layouts_changed();
}

#[test]
fn get_inputs_and_seats_return_sway_schema_and_values() {
    let mut fixture = Fixture::new();
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::keyboard("wayland-keyboard-seat0"),
        },
    );
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::pointer("wayland-pointer-seat0"),
        },
    );
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    fixture.niri_state().ipc_refresh_layout();

    let mut stream = UnixStream::connect(socket).unwrap();
    let inputs = query_ipc(&mut fixture, &mut stream, MessageType::GetInputs);
    let mut sway_inputs: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/sway/inputs.json")).unwrap();
    sway_inputs
        .as_array_mut()
        .unwrap()
        .sort_by_key(|input| input["identifier"].as_str().unwrap().to_owned());
    assert_eq!(inputs, sway_inputs);

    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::libinput_pointer("Logitech G703 LS"),
        },
    );
    let inputs = query_ipc(&mut fixture, &mut stream, MessageType::GetInputs);
    let sway_libinput: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/inputs-libinput.json"
    ))
    .unwrap();
    let actual_libinput = inputs
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["identifier"] == "1133:16518:Logitech_G703_LS")
        .unwrap();
    assert_eq!(actual_libinput, &sway_libinput[0]);

    let seats = query_ipc(&mut fixture, &mut stream, MessageType::GetSeats);
    let focused = crate::ipc::tree::window_id(fixture.swayward().layout.focus().unwrap().id());
    assert_eq!(
        seats,
        serde_json::json!([{
            "name": "headless",
            "capabilities": 3,
            "focus": focused,
            "devices": inputs
        }])
    );
}

#[test]
fn exec_does_not_inherit_the_ipc_listener() {
    let (mut fixture, _socket) = ipc_fixture();
    let output = std::env::temp_dir().join(format!("swayward-exec-fds-{}", std::process::id()));
    let temporary = output.with_extension("pending");
    let command = format!(
        "exec sh -c 'ls -l /proc/self/fd > {} && mv {} {}'",
        temporary.display(),
        temporary.display(),
        output.display()
    );
    assert!(crate::command::execute(fixture.niri_state(), &command)[0].success);

    let deadline = Instant::now() + Duration::from_secs(2);
    while !output.exists() {
        assert!(Instant::now() < deadline, "exec did not produce fd listing");
        std::thread::sleep(Duration::from_millis(10));
    }
    let inherited = std::fs::read_to_string(&output).unwrap();
    std::fs::remove_file(output).unwrap();
    assert!(
        !inherited.lines().any(|line| line.contains(" -> socket:[")),
        "exec inherited a socket: {inherited}"
    );
}

#[test]
fn get_bar_config_distinguishes_no_bars_from_an_unknown_id() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();

    assert_eq!(
        query_ipc(&mut fixture, &mut stream, MessageType::GetBarConfig),
        serde_json::json!([])
    );
    assert_eq!(
        query_ipc_with_payload(
            &mut fixture,
            &mut stream,
            MessageType::GetBarConfig,
            "bar-0",
        ),
        serde_json::json!({"success": false, "error": "No bar with that ID"})
    );
}

/// A request type outside `MessageType` must get a reply, not a disconnect.
///
/// Sway answers `IPC_SYNC` with `{"success": false}`
/// (`sway/sway/ipc-server.c:919-924`) and keeps the connection open for
/// anything else it does not know (`ipc-server.c:927-929`). Decoding the
/// header through `MessageType::try_from` turned both into a `?`-propagated
/// error that dropped the client with no JSON body.
#[test]
fn unknown_request_types_get_a_structured_reply_and_keep_the_connection() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();

    // IPC_SYNC, sway/include/ipc.h:20.
    stream
        .write_all(&swayward_ipc::wire::encode_raw(11, ""))
        .unwrap();
    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(reply_type, 11);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"success": false})
    );

    stream
        .write_all(&swayward_ipc::wire::encode_raw(9999, ""))
        .unwrap();
    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(reply_type, 9999);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"success": false, "error": "not implemented"})
    );

    // The connection survives both, so a normal request still answers.
    let version = query_ipc(&mut fixture, &mut stream, MessageType::GetVersion);
    assert_eq!(version["variant"], "swayward");
}

#[test]
fn captured_workspace_event_sequences_pin_order_and_multiplicity() {
    for (fixture, expected) in [
        (
            include_str!("../../tests/fixtures/sway/events/workspace-switch-empty.sequence.json"),
            &["init", "focus", "focus", "empty"][..],
        ),
        (
            include_str!("../../tests/fixtures/sway/events/workspace-close-last.sequence.json"),
            &["empty"][..],
        ),
        (
            include_str!("../../tests/fixtures/sway/events/workspace-rename.sequence.json"),
            &["rename"][..],
        ),
        (
            include_str!(
                "../../tests/fixtures/sway/events/workspace-move-right-empty-destination.sequence.json"
            ),
            &[][..],
        ),
        (
            include_str!(
                "../../tests/fixtures/sway/events/workspace-move-right-occupied-destination.sequence.json"
            ),
            &[][..],
        ),
        (
            include_str!(
                "../../tests/fixtures/sway/events/workspace-move-right-last-source.sequence.json"
            ),
            &[][..],
        ),
    ] {
        let events = serde_json::from_str::<Vec<Value>>(fixture).unwrap();
        let changes = events
            .iter()
            .map(|event| event["change"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(changes, expected);
    }
}

#[test]
fn get_config_returns_raw_top_level_config_after_reload() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();
    let root = std::env::temp_dir().join(format!("swayward-get-config-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("included.kdl"), "layout { gaps 7; }\n").unwrap();
    let source = "include \"included.kdl\"\n";
    let config = swayward_config::Config::parse(&root.join("config.kdl"), source)
        .config
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    fixture.niri_state().reload_config(Ok(config));

    let reply = query_ipc(&mut fixture, &mut stream, MessageType::GetConfig);
    assert_eq!(reply, serde_json::json!({"config": source}));
}

#[test]
fn input_subscription_emits_added_and_removed_with_get_inputs_payload() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["input"]"#,
        ))
        .unwrap();
    let ((msg_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);

    let device = TestDevice::keyboard("test keyboard");
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded { device },
    );
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 21);
    let added = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(added["change"], "added");

    let mut query = UnixStream::connect(&socket).unwrap();
    let inputs = query_ipc(&mut fixture, &mut query, MessageType::GetInputs);
    assert_eq!(added["input"], inputs[0]);

    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceRemoved { device },
    );
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 21);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change": "removed", "input": added["input"]})
    );
    assert!(remainder.is_empty());
}

#[test]
fn input_events_do_not_leak_to_a_tick_only_subscriber() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["tick"]"#,
        ))
        .unwrap();
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);

    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::pointer("test pointer"),
        },
    );
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::Tick {
            payload: "barrier".into(),
            first: false,
        });
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 7);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"first": false, "payload": "barrier"})
    );
    assert!(remainder.is_empty());
}

#[test]
fn input_subscription_emits_xkb_keymap_and_layout_from_current_payload() {
    let config =
        swayward_config::Config::parse_mem(r#"input { keyboard { xkb { layout "us,ru"; }; }; }"#)
            .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::keyboard("test keyboard"),
        },
    );

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["input"]"#,
        ))
        .unwrap();
    let ((_, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(payload, r#"{"success": true}"#);

    fixture.niri_state().ipc_keyboard_layouts_changed();
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 21);
    let keymap = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(keymap["change"], "xkb_keymap");
    let mut query = UnixStream::connect(&socket).unwrap();
    let inputs = query_ipc(&mut fixture, &mut query, MessageType::GetInputs);
    assert_eq!(keymap["input"], inputs[0]);

    set_xkb_layout(&mut fixture, 1);
    fixture.niri_state().ipc_refresh_keyboard_layout_index();
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 21);
    let layout = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(layout["change"], "xkb_layout");
    let inputs = query_ipc(&mut fixture, &mut query, MessageType::GetInputs);
    assert_eq!(layout["input"], inputs[0]);
    assert_eq!(layout["input"]["xkb_active_layout_index"], 1);
    assert_eq!(layout["input"]["xkb_active_layout_name"], "Russian");
    assert!(remainder.is_empty());
}

#[test]
fn input_event_queue_overflow_disconnects_a_non_reading_subscriber() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["input"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    for _ in 0..4097 {
        fixture.swayward().ipc_server.as_ref().unwrap().send_event(
            swayward_ipc::legacy::Event::SwayInputChanged {
                change: "added".into(),
                input: serde_json::json!({"identifier":"0:0:test","name":"test","type":"pointer"}),
            },
        );
    }
    for _ in 0..10 {
        fixture.dispatch();
    }
    subscriber.set_nonblocking(true).unwrap();
    let mut bytes = Vec::new();
    loop {
        let mut buffer = [0; 64 * 1024];
        match subscriber.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => bytes.extend_from_slice(&buffer[..length]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                panic!("subscriber remained connected after its input event queue overflowed")
            }
            Err(error) => panic!("error reading subscriber: {error}"),
        }
    }
}

#[test]
fn output_subscription_emits_exact_event() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1280, 720));
    fixture.add_output(2, (1280, 720));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["output"]"#,
        ))
        .unwrap();
    let ((msg_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);

    fixture.replace_outputs(vec![((0, 0), (1280, 720))]);
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 1);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change": "unspecified"})
    );
    assert!(remainder.is_empty());
}

#[test]
fn output_event_is_not_sent_to_a_tick_only_subscriber() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1280, 720));
    fixture.add_output(2, (1280, 720));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["tick"]"#,
        ))
        .unwrap();
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);

    fixture.replace_outputs(vec![((0, 0), (1280, 720))]);
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::Tick {
            payload: "barrier".into(),
            first: false,
        });
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 7);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"first": false, "payload": "barrier"})
    );
    assert!(remainder.is_empty());
}

#[test]
fn shutdown_subscription_emits_exact_exit_event() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["shutdown"]"#,
        ))
        .unwrap();
    let ((msg_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);

    fixture.niri_state().request_stop("exit");
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 6);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change": "exit"})
    );
    assert!(remainder.is_empty());
}

#[test]
fn shutdown_event_is_not_sent_to_a_workspace_only_subscriber() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert!(remainder.is_empty());

    fixture.niri_state().request_stop("exit");
    for _ in 0..10 {
        fixture.dispatch();
    }
    subscriber.set_nonblocking(true).unwrap();
    let mut byte = [0];
    assert_eq!(
        subscriber.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn tick_subscription_emits_initial_event_before_real_ticks() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["tick"]"#,
        ))
        .unwrap();

    let ((msg_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 7);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"first": true, "payload": ""})
    );

    let mut sender = UnixStream::connect(&socket).unwrap();
    sender
        .write_all(&swayward_ipc::wire::encode(MessageType::SendTick, "ready"))
        .unwrap();
    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut sender);
    assert_eq!(reply_type, MessageType::SendTick as u32);
    assert_eq!(payload, r#"{"success":true}"#);
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 7);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"first": false, "payload": "ready"})
    );
    assert!(remainder.is_empty());
}

#[test]
fn non_tick_subscription_does_not_emit_an_initial_tick() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();

    let ((msg_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::WorkspaceReloaded);
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, 1 << 31);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change": "reload", "old": null, "current": null})
    );
    assert!(remainder.is_empty());
}

#[test]
fn event_queue_overflow_removes_a_non_reading_subscriber() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["output"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let server = fixture.swayward().ipc_server.as_ref().unwrap();
    assert_eq!(server.event_stream_count(), 1);
    for _ in 0..4097 {
        fixture.swayward().ipc_output_changed();
    }
    assert_eq!(
        fixture
            .swayward()
            .ipc_server
            .as_ref()
            .unwrap()
            .event_stream_count(),
        0
    );
}

#[test]
fn non_reading_event_subscriber_is_disconnected_without_blocking_ipc() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["tick"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let payload = "x".repeat(1024 * 1024);
    for _ in 0..4 {
        fixture.swayward().ipc_server.as_ref().unwrap().send_event(
            swayward_ipc::legacy::Event::Tick {
                payload: payload.clone(),
                first: false,
            },
        );
        fixture.dispatch();
    }

    subscriber.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut buffer = [0; 64 * 1024];
    loop {
        fixture.dispatch();
        match subscriber.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "subscriber was not disconnected");
            }
            Err(error) => panic!("error reading subscriber: {error}"),
        }
    }

    let mut liveness = UnixStream::connect(&socket).unwrap();
    let reply = query_ipc(&mut fixture, &mut liveness, MessageType::GetVersion);
    assert_eq!(reply["variant"], "swayward");
}

#[test]
fn event_subscription_does_not_block_a_concurrent_query() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    let mut query = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(MessageType::GetVersion, ""))
        .unwrap();

    let (msg_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);

    let (msg_type, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(msg_type, MessageType::GetVersion as u32);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap()["variant"],
        "swayward"
    );
}

fn subscribe_to_window_events(fixture: &mut Fixture, socket: &std::path::Path) -> UnixStream {
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);
    subscriber
}

fn map_test_window(fixture: &mut Fixture, client: super::client::ClientId, app_id: &str) {
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id(app_id.into());
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
}

#[test]
fn captured_window_map_sequences_pin_focus_order_and_multiplicity() {
    for (fixture, expected) in [
        (
            include_str!("../../tests/fixtures/sway/events/window-map-focused.sequence.json"),
            &["new", "title", "focus"][..],
        ),
        (
            include_str!("../../tests/fixtures/sway/events/window-map-unfocused.sequence.json"),
            &["new", "title"][..],
        ),
    ] {
        let events: Vec<Value> = serde_json::from_str(fixture).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event["change"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn mapping_a_focused_window_emits_new_then_focus() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();
    let client = fixture.add_client();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    map_test_window(&mut fixture, client, "focused-map");
    fixture.niri_state().update_keyboard_focus();
    assert!(fixture
        .swayward()
        .layout
        .windows()
        .any(|(_, mapped)| mapped.is_focused()));
    fixture.niri_state().ipc_refresh_layout();

    let mut remainder = Vec::new();
    let changes = (0..2)
        .map(|_| {
            let ((event_type, payload), next) =
                read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone());
            remainder = next;
            assert_eq!(event_type, (1 << 31) | 3);
            serde_json::from_str::<Value>(&payload).unwrap()["change"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(changes, ["new", "focus"]);
}

#[test]
fn mapping_an_unfocused_window_emits_only_new() {
    let mut config = swayward_config::Config::default();
    config.window_rules.push(swayward_config::WindowRule {
        matches: vec![swayward_config::window_rule::Match {
            app_id: Some("^unfocused-map$".parse().unwrap()),
            ..Default::default()
        }],
        open_focused: Some(false),
        ..Default::default()
    });
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "existing-focus");
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    map_test_window(&mut fixture, client, "unfocused-map");
    fixture.niri_state().update_keyboard_focus();
    fixture.niri_state().ipc_refresh_layout();

    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, (1 << 31) | 3);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap()["change"],
        "new"
    );
    assert!(
        remainder.is_empty(),
        "unexpected second window event was buffered"
    );
    subscriber.set_nonblocking(true).unwrap();
    fixture.dispatch();
    let mut byte = [0];
    assert!(matches!(
        subscriber.read(&mut byte),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

/// A subscribed connection is still a normal IPC connection. Sway keeps every
/// client in `ipc_client_handle_readable` and dispatches whatever arrives next
/// through `ipc_client_handle_command`; `IPC_SUBSCRIBE` only sets
/// `client->subscribed_events` and falls through to `exit_cleanup`
/// (`sway/sway/ipc-server.c:730-784`). Nothing there forbids a later
/// `IPC_GET_TREE` (`ipc-server.c:815-823`), and i3ipc stacks reuse one fd for
/// both. Queries must be answered, the replies must be current, and the
/// subscription must survive them.
#[test]
fn a_subscribed_connection_still_answers_queries_and_keeps_its_events() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();
    let client = fixture.add_client();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    // A query on the subscribed fd gets a reply of the requested type.
    let tree = query_ipc(&mut fixture, &mut subscriber, MessageType::GetTree);
    assert_eq!(tree["type"], "root");
    let workspaces = query_ipc(&mut fixture, &mut subscriber, MessageType::GetWorkspaces);
    assert!(workspaces.is_array(), "get_workspaces must return an array");

    // The subscription survives, and events queued after the query arrive.
    map_test_window(&mut fixture, client, "subscribe-then-query");
    fixture.niri_state().update_keyboard_focus();
    fixture.niri_state().ipc_refresh_layout();
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, (1 << 31) | 3);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap()["change"],
        "new"
    );

    // A second query on the same fd reflects state as of now, not the snapshot
    // taken when the connection was accepted.
    subscriber
        .write_all(&swayward_ipc::wire::encode(MessageType::GetTree, ""))
        .unwrap();
    let mut pending = remainder;
    let payload = loop {
        let ((message_type, payload), next) =
            read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, pending);
        if message_type == MessageType::GetTree as u32 {
            break payload;
        }
        pending = next;
        // Window events may be queued ahead of the reply; nothing else may be.
        assert_eq!(message_type, (1 << 31) | 3, "unexpected message on the fd");
    };
    let tree: Value = serde_json::from_str(&payload).unwrap();
    assert!(
        find_json_node_with_app_id(&tree, "subscribe-then-query").is_some(),
        "get_tree after subscribe must show the window mapped since: {tree}"
    );
}

#[test]
fn workspace_window_and_mode_events_match_sway_shapes() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-event".into());
    window.set_title("fixture-event");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace","window","mode"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::WorkspaceActivated {
            id: 1,
            focused: true,
        },
    );
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/workspace.reload.json"
    ))
    .unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$workspace",
    );

    let focused_id = fixture
        .swayward()
        .layout
        .focus()
        .map(|window| window.id().get());
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let container = super::super::ipc::server::find_node_by_id(
        &tree,
        crate::ipc::tree::window_id_from_raw(focused_id.unwrap()),
    )
    .unwrap()
    .clone();
    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::SwayWindowChanged {
            change: "focus".into(),
            container,
        },
    );
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/window.focus.json"
    ))
    .unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$window",
    );

    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::BindingModeChanged {
            mode: "default".into(),
            pango_markup: false,
        },
    );
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/mode.default.json"
    ))
    .unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");
}

#[test]
fn niri_only_window_events_do_not_leak_onto_sway_subscriptions() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window","tick"]"#,
        ))
        .unwrap();
    read_ipc_reply(&mut fixture, &mut subscriber);
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::WindowLayoutsChanged { changes: vec![] });
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::Tick {
            payload: "barrier".into(),
            first: false,
        });

    let (event_type, _) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 7);
}

#[test]
fn workspace_focus_events_mark_only_the_new_workspace_focused() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let mut remainder = Vec::new();
    for name in ["2", "3", "1"] {
        assert!(
            crate::command::execute(fixture.niri_state(), &format!("workspace {name}"))[0].success
        );
        loop {
            let ((event_type, payload), next) =
                read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
            remainder = next;
            assert_eq!(event_type, 1 << 31);
            let event = serde_json::from_str::<Value>(&payload).unwrap();
            if event["change"] == "focus" {
                assert_eq!(event["current"]["name"], name);
                assert_eq!(event["current"]["focused"], true, "{event}");
                assert_eq!(event["old"]["focused"], false, "{event}");
                break;
            }
        }
    }
}

#[test]
fn workspace_urgency_event_matches_sway_shape() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    for app_id in ["urgent-target", "focused"] {
        map_test_window(&mut fixture, client, app_id);
    }

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    set_test_window_urgent(&mut fixture, "urgent-target");
    fixture.niri_state().ipc_refresh_layout();

    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected = serde_json::from_str::<Value>(include_str!(
        "../../tests/fixtures/sway/events/workspace.urgent.json"
    ))
    .unwrap();
    assert_event_shape(&expected, &actual, "$workspace");
    assert_eq!(actual["change"], "urgent");
    assert_eq!(actual["old"], Value::Null);
    assert_eq!(actual["current"]["urgent"], true);
}

#[test]
fn workspace_move_event_matches_sway_shape() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1280, 720));
    fixture.niri_state().ipc_refresh_layout();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let current =
        serde_json::from_value(find_json_node(&tree, "workspace", false).unwrap().clone()).unwrap();
    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::WorkspaceMoved {
            current: Box::new(current),
        },
    );

    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected = serde_json::from_str::<Value>(include_str!(
        "../../tests/fixtures/sway/events/workspace.move.json"
    ))
    .unwrap();
    assert_eq!(actual["change"], expected["change"]);
    assert_eq!(actual["old"], Value::Null);
    assert_eq!(actual["current"]["type"], "workspace");
}

#[test]
fn workspace_rename_event_matches_sway_shape() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    assert!(
        crate::command::execute(fixture.niri_state(), "rename workspace to event-renamed")[0]
            .success
    );
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected = serde_json::from_str::<Value>(include_str!(
        "../../tests/fixtures/sway/events/workspace.rename.json"
    ))
    .unwrap();
    assert_event_shape(&expected, &actual, "$workspace");
    assert_eq!(actual["change"], "rename");
    assert_eq!(actual["old"], Value::Null);
    assert_eq!(actual["current"]["name"], "event-renamed");
}

#[test]
fn run_command_returns_one_outcome_per_command_and_keeps_connection_alive() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "focus left; frobnicate",
        ))
        .unwrap();

    let (msg_type, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(msg_type, MessageType::RunCommand as u32);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([
            {"success": true},
            {"success": false, "error": "Unknown/invalid command 'frobnicate'", "parse_error": true}
        ])
    );

    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::RunCommand, "nop"))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([{"success": true}])
    );
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[test]
fn mark_event_matches_captured_sway_schema() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("event-one".into());
    window.set_title("event-one");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "mark event-mark")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/window.mark.json"
    ))
    .unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$window",
    );
}

#[test]
fn close_event_matches_captured_sway_schema_before_removal() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("event-one".into());
    window.set_title("event-one");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let mapped = fixture
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id())
                .then(|| mapped.window.clone())
        })
        .unwrap();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    fixture
        .swayward()
        .layout
        .remove_window(&mapped, crate::utils::transaction::Transaction::new());
    fixture.niri_state().ipc_refresh_layout();
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/window.close.json"
    ))
    .unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$window",
    );
}

#[test]
fn marks_round_trip_through_commands_get_marks_and_tree() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let id = fixture.add_client();
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.set_title("fixture-1");
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "mark testmark",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([{"success": true}])
    );

    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::GetMarks, ""))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!(["testmark"])
    );

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let marked = find_json_node(&tree, "con", true).unwrap();
    let oracle: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/sway/marked.tree.json")).unwrap();
    let expected = find_json_node(&oracle, "con", true).unwrap();
    assert_eq!(marked["marks"], expected["marks"]);

    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "mark --add second, mark --add --toggle testmark; [con_mark=second] unmark",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert!(serde_json::from_str::<Vec<Value>>(&reply)
        .unwrap()
        .iter()
        .all(|outcome| outcome["success"] == true));
    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::GetMarks, ""))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([])
    );
}

#[derive(Debug)]
struct TestInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TestDevice {
    name: &'static str,
    keyboard: bool,
    libinput: bool,
}

impl TestDevice {
    fn keyboard(name: &'static str) -> Self {
        Self {
            name,
            keyboard: true,
            libinput: false,
        }
    }

    fn pointer(name: &'static str) -> Self {
        Self {
            name,
            keyboard: false,
            libinput: false,
        }
    }

    fn libinput_pointer(name: &'static str) -> Self {
        Self {
            name,
            keyboard: false,
            libinput: true,
        }
    }
}

impl crate::input::backend_ext::NiriInputDevice for TestDevice {
    fn sway_libinput(&self) -> Option<Value> {
        self.libinput.then(|| {
            serde_json::json!({
                "send_events": "enabled",
                "accel_speed": 0.0,
                "accel_profile": "adaptive",
                "natural_scroll": "disabled",
                "left_handed": "disabled",
                "middle_emulation": "disabled",
                "scroll_method": "none",
                "scroll_button": 274,
                "scroll_button_lock": "disabled"
            })
        })
    }

    fn output(&self, _state: &crate::swayward::State) -> Option<smithay::output::Output> {
        None
    }
}

impl smithay::backend::input::Device for TestDevice {
    fn id(&self) -> String {
        self.name.into()
    }

    fn name(&self) -> String {
        self.name.into()
    }

    fn has_capability(&self, capability: smithay::backend::input::DeviceCapability) -> bool {
        capability
            == if self.keyboard {
                smithay::backend::input::DeviceCapability::Keyboard
            } else {
                smithay::backend::input::DeviceCapability::Pointer
            }
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        self.libinput.then_some((16518, 1133))
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[derive(Debug)]
struct TestKeyEvent {
    device: TestDevice,
    key: u32,
    count: u32,
    state: smithay::backend::input::KeyState,
}

#[derive(Debug)]
struct TestButtonEvent {
    device: TestDevice,
    button: u32,
    state: smithay::backend::input::ButtonState,
}

/// Absolute pointer motion, so a test can drive an interactive drag.
///
/// Without this the conformance adapter could only teleport the cursor with
/// `Swayward::move_cursor`, which updates pointer contents but never reaches the
/// pointer grab, so `Layout::interactive_move_update` never ran and a dragged
/// window never moved.
#[derive(Debug)]
struct TestMotionAbsoluteEvent {
    device: TestDevice,
    x: f64,
    y: f64,
    output_size: smithay::utils::Size<f64, smithay::utils::Logical>,
}

#[derive(Debug)]
struct TestAxisEvent {
    device: TestDevice,
    horizontal_v120: f64,
    vertical_v120: f64,
}

impl smithay::backend::input::Event<TestInput> for TestKeyEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::Event<TestInput> for TestButtonEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::Event<TestInput> for TestAxisEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::PointerAxisEvent<TestInput> for TestAxisEvent {
    fn amount(&self, _axis: smithay::backend::input::Axis) -> Option<f64> {
        None
    }

    fn amount_v120(&self, axis: smithay::backend::input::Axis) -> Option<f64> {
        Some(match axis {
            smithay::backend::input::Axis::Horizontal => self.horizontal_v120,
            smithay::backend::input::Axis::Vertical => self.vertical_v120,
        })
    }

    fn source(&self) -> smithay::backend::input::AxisSource {
        smithay::backend::input::AxisSource::Wheel
    }

    fn relative_direction(
        &self,
        _axis: smithay::backend::input::Axis,
    ) -> smithay::backend::input::AxisRelativeDirection {
        smithay::backend::input::AxisRelativeDirection::Identical
    }
}

impl smithay::backend::input::PointerButtonEvent<TestInput> for TestButtonEvent {
    fn button_code(&self) -> u32 {
        self.button
    }

    fn state(&self) -> smithay::backend::input::ButtonState {
        self.state
    }
}

impl smithay::backend::input::Event<TestInput> for TestMotionAbsoluteEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::AbsolutePositionEvent<TestInput> for TestMotionAbsoluteEvent {
    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }

    fn x_transformed(&self, width: i32) -> f64 {
        self.x * f64::from(width) / self.output_size.w
    }

    fn y_transformed(&self, height: i32) -> f64 {
        self.y * f64::from(height) / self.output_size.h
    }
}

impl smithay::backend::input::PointerMotionAbsoluteEvent<TestInput> for TestMotionAbsoluteEvent {}

impl smithay::backend::input::KeyboardKeyEvent<TestInput> for TestKeyEvent {
    fn key_code(&self) -> smithay::backend::input::Keycode {
        self.key.into()
    }

    fn state(&self) -> smithay::backend::input::KeyState {
        self.state
    }

    fn count(&self) -> u32 {
        self.count
    }
}

impl smithay::backend::input::InputBackend for TestInput {
    type Device = TestDevice;
    type KeyboardKeyEvent = TestKeyEvent;
    type PointerAxisEvent = TestAxisEvent;
    type PointerButtonEvent = TestButtonEvent;
    type PointerMotionEvent = smithay::backend::input::UnusedEvent;
    type PointerMotionAbsoluteEvent = TestMotionAbsoluteEvent;
    type GestureSwipeBeginEvent = smithay::backend::input::UnusedEvent;
    type GestureSwipeUpdateEvent = smithay::backend::input::UnusedEvent;
    type GestureSwipeEndEvent = smithay::backend::input::UnusedEvent;
    type GesturePinchBeginEvent = smithay::backend::input::UnusedEvent;
    type GesturePinchUpdateEvent = smithay::backend::input::UnusedEvent;
    type GesturePinchEndEvent = smithay::backend::input::UnusedEvent;
    type GestureHoldBeginEvent = smithay::backend::input::UnusedEvent;
    type GestureHoldEndEvent = smithay::backend::input::UnusedEvent;
    type TouchDownEvent = smithay::backend::input::UnusedEvent;
    type TouchUpEvent = smithay::backend::input::UnusedEvent;
    type TouchMotionEvent = smithay::backend::input::UnusedEvent;
    type TouchCancelEvent = smithay::backend::input::UnusedEvent;
    type TouchFrameEvent = smithay::backend::input::UnusedEvent;
    type TabletToolAxisEvent = smithay::backend::input::UnusedEvent;
    type TabletToolProximityEvent = smithay::backend::input::UnusedEvent;
    type TabletToolTipEvent = smithay::backend::input::UnusedEvent;
    type TabletToolButtonEvent = smithay::backend::input::UnusedEvent;
    type SwitchToggleEvent = smithay::backend::input::UnusedEvent;
    type SpecialEvent = ();
}

fn active_workspace_name(fixture: &mut Fixture) -> Option<String> {
    fixture
        .swayward()
        .layout
        .active_workspace()
        .and_then(|workspace| workspace.name().cloned())
}

pub(super) fn pointer_button(fixture: &mut Fixture, button: u32, pressed: bool) {
    pointer_button_from(
        fixture,
        TestDevice::pointer("test pointer"),
        button,
        pressed,
    );
}

fn pointer_button_from(fixture: &mut Fixture, device: TestDevice, button: u32, pressed: bool) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::PointerButton {
            event: TestButtonEvent {
                device,
                button,
                state: if pressed {
                    smithay::backend::input::ButtonState::Pressed
                } else {
                    smithay::backend::input::ButtonState::Released
                },
            },
        },
    );
}

/// Absolute pointer motion through the real input path, so a pointer grab sees
/// it and an interactive drag actually tracks the cursor.
pub(super) fn pointer_motion_absolute(fixture: &mut Fixture, x: f64, y: f64) {
    let output = fixture.swayward().global_space.outputs().next().cloned();
    let output_size = output
        .and_then(|output| {
            fixture
                .swayward()
                .global_space
                .output_geometry(&output)
                .map(|geo| geo.size.to_f64())
        })
        .unwrap_or_else(|| smithay::utils::Size::from((1920., 1080.)));
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::PointerMotionAbsolute {
            event: TestMotionAbsoluteEvent {
                device: TestDevice::pointer("test pointer"),
                x,
                y,
                output_size,
            },
        },
    );
}

pub(super) fn pointer_axis(fixture: &mut Fixture, horizontal_v120: f64, vertical_v120: f64) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::PointerAxis {
            event: TestAxisEvent {
                device: TestDevice::pointer("test pointer"),
                horizontal_v120,
                vertical_v120,
            },
        },
    );
}

pub(super) fn key_event(fixture: &mut Fixture, key: u32, pressed: bool) {
    key_event_from(fixture, TestDevice::keyboard("test keyboard"), key, pressed);
}

fn key_event_from(fixture: &mut Fixture, device: TestDevice, key: u32, pressed: bool) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::Keyboard {
            event: TestKeyEvent {
                device,
                key,
                count: u32::from(pressed),
                state: if pressed {
                    smithay::backend::input::KeyState::Pressed
                } else {
                    smithay::backend::input::KeyState::Released
                },
            },
        },
    );
}

pub(super) fn type_key_chords(fixture: &mut Fixture, chords: &[&[u32]]) {
    for chord in chords {
        for &key in *chord {
            key_event(fixture, key, true);
        }
        for &key in chord.iter().rev() {
            key_event(fixture, key, false);
        }
    }
}

#[test]
fn mouse_input_device_binding_prefers_exact_device_and_wildcard_matches_another() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "rename workspace to wildcard-mouse"; }
            MouseLeft input-device="0:0:first_mouse" { command "rename workspace to exact-mouse"; }
            MouseRight input-device="0:0:first_mouse" { command "rename workspace to wrong-mouse"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("first mouse"),
            0x110,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("second mouse"),
            0x111,
            pressed,
        );
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("second mouse"),
            0x110,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard-mouse")
    );
}

#[test]
fn device_identifier_matches_sways_libinput_format() {
    use crate::input::backend_ext::NiriInputDevice as _;

    let device = TestDevice::keyboard("  keyboard with spaces  ");
    assert_eq!(device.sway_identifier(), "0:0:keyboard_with_spaces");
}

#[test]
fn input_device_binding_prefers_exact_device_and_wildcard_matches_another() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            x { command "rename workspace to wildcard"; }
            x input-device="0:0:first_keyboard" { command "rename workspace to exact"; }
            z input-device="0:0:first_keyboard" { command "rename workspace to wrong"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("first keyboard"),
            53,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact")
    );

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("second keyboard"),
            52,
            pressed,
        );
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong")
    );

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("second keyboard"),
            53,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );
}

fn set_xkb_layout(fixture: &mut Fixture, layout: u32) {
    let keyboard = fixture.swayward().seat.get_keyboard().unwrap();
    keyboard.with_xkb_state(fixture.niri_state(), |mut context| {
        context.set_layout(smithay::input::keyboard::Layout(layout));
    });
}

#[test]
fn group_binding_overrides_wildcard_only_in_its_active_group() {
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { xkb { layout "us,ru,us"; }; }; }
        binds {
            x { command "rename workspace to wildcard"; };
            Group2+x { command "rename workspace to exact"; };
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );

    set_xkb_layout(&mut fixture, 1);
    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact")
    );
    set_xkb_layout(&mut fixture, 2);
    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );
}

#[test]
fn translated_keysym_binding_fires_in_its_xkb_layout() {
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { xkb { layout "us,ru"; }; }; }
        binds { Cyrillic_ze { command "rename workspace to cyrillic"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[33]]);
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("cyrillic")
    );

    set_xkb_layout(&mut fixture, 1);
    type_key_chords(&mut fixture, &[&[33]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("cyrillic")
    );
}

fn add_tiled_windows(fixture: &mut Fixture, client: super::client::ClientId, count: usize) {
    for _ in 0..count {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}

#[test]
fn titlebar_wheel_binding_takes_precedence_over_tab_focus() {
    let config = swayward_config::Config::parse_mem(
        r#"layout { gaps 0; }
        binds {
            WheelScrollDown mouse-regions="titlebar" { command "mark bound"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    add_tiled_windows(&mut fixture, client, 3);
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus left")[0].success);
    let focused = fixture.swayward().layout.focus().unwrap().id();
    fixture.niri_state().move_cursor((100., 10.).into());
    pointer_axis(&mut fixture, 0., 120.);

    let swayward = fixture.swayward();
    assert!(swayward
        .marks_by_window
        .values()
        .chain(swayward.marks_by_container.values())
        .flatten()
        .any(|mark| mark == "bound"));
    assert_eq!(swayward.layout.focus().unwrap().id(), focused);
}

#[test]
fn pointer_button_binding_requires_the_configured_rendered_region() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace startup"; }
            MouseLeft mouse-regions="contents" { command "workspace clicked"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("clicked")
        .is_none());
}

#[test]
fn numlock_qualified_binding_dispatches_only_while_numlock_is_active() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Num+a { command "rename workspace to numlocked"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_none());

    key_event(&mut fixture, 77, true);
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .num_lock
    );
    key_event(&mut fixture, 77, false);
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .num_lock
    );
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_some());
}

#[test]
fn unqualified_binding_dispatches_while_numlock_is_active() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Mod4+a { command "rename workspace to numlocked"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 77, true);
    key_event(&mut fixture, 77, false);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_some());
}

#[test]
fn modifier_bindcode_matches_without_its_own_modifier() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { "code:133" release=true { command "rename workspace to super-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 133, true);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("super-release")
        .is_none());
    key_event(&mut fixture, 133, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("super-release")
        .is_some());
}

#[test]
fn overview_arrow_keys_move_between_workspaces() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 2", "workspace 1", "split vertical"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    for _ in 0..2 {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let first_workspace = active_workspace_idx(&mut fixture);
    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    assert!(
        fixture.swayward().keyboard_focus.is_overview(),
        "overview opened with keyboard focus {:?}",
        fixture.swayward().keyboard_focus
    );

    for (key, expected_workspace) in [(116, first_workspace + 1), (111, first_workspace)] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
        assert_eq!(
            active_workspace_idx(&mut fixture),
            expected_workspace,
            "keycode {key} did not focus workspace index {expected_workspace}"
        );
    }
}

#[test]
fn overview_arrow_keys_wrap_at_the_ends() {
    // The overview shows the whole stack at once, so an arrow that stops dead
    // at the last workspace reads as a broken key rather than as an edge. With
    // only two workspaces one of the two arrows always looked dead, which is
    // how this was reported. Sway's own `workspace next` wraps.
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 1", "workspace 2"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    // The monitor also holds a trailing unnamed workspace, so the wrap target
    // is the last index rather than the last *named* one. Drive to index 0
    // first so the wrap is unambiguous.
    crate::command::execute(fixture.niri_state(), "workspace 1");
    // Let the workspace-switch animation finish: the wrapping helpers defer to
    // the plain clamped ones while a switch is in flight.
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.swayward().layout.advance_animations();
    fixture.swayward().clock.set_complete_instantly(false);
    assert_eq!(active_workspace_idx(&mut fixture), 0);
    let last = fixture.swayward().layout.workspaces().count() - 1;

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();

    // Up from the first workspace wraps to the last, and Down from the last
    // wraps back to the first.
    for (key, expected) in [(111, last), (116, 0)] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
        // Settle the switch animation: a wrap issued mid-switch falls back to
        // the clamped helper and would test the wrong thing.
        fixture.swayward().clock.set_complete_instantly(true);
        fixture.swayward().layout.advance_animations();
        fixture.swayward().clock.set_complete_instantly(false);
        assert_eq!(
            active_workspace_idx(&mut fixture),
            expected,
            "keycode {key} did not wrap to workspace index {expected}"
        );
    }
}

#[test]
fn ordinary_modified_keysym_bind_still_matches() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+Return { command "rename workspace to modified"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[133, 36]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("modified")
        .is_some());
}

#[test]
fn translated_keysym_uses_post_transition_consumed_modifiers() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Alt+at { command "rename workspace to translated"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[64, 50, 11]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("translated")
        .is_some());
}

#[test]
fn bindcode_uses_the_xkb_keycode_from_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { "code:39" { command "rename workspace to bindcode"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("bindcode")
        .is_some());
}

#[test]
fn release_key_binding_dispatches_only_on_release_through_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    key_event(&mut fixture, 53, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn another_key_cancels_a_held_release_binding_without_an_ipc_event() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "nop release"; }; }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1280, 720));
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    key_event(&mut fixture, 53, true);
    key_event(&mut fixture, 52, true);
    key_event(&mut fixture, 53, false);
    key_event(&mut fixture, 52, false);
    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::SwayBinding {
            command: "sentinel".into(),
            event_state_mask: vec![],
            input_codes: vec![],
            input_code: 0,
            symbols: vec!["t".into()],
            symbol: Some("t".into()),
            input_type: "keyboard".into(),
        },
    );

    let mut commands = Vec::new();
    loop {
        let (message_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
        assert_eq!(message_type, (1 << 31) | 5);
        let command = serde_json::from_str::<Value>(&payload).unwrap()["binding"]["command"]
            .as_str()
            .unwrap()
            .to_owned();
        commands.push(command.clone());
        if command == "sentinel" {
            break;
        }
    }
    assert_eq!(commands, ["sentinel"]);
}

#[test]
fn release_key_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            x { command "mode other"; }
            x release=true { command "workspace key-released"; }
        }
        mode "other" { y { command "nop"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_survives_config_reload_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace key-released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    super::i3_conformance::reload_test_config(&mut fixture, "font monospace\n").unwrap();
    assert_eq!(fixture.swayward().binding_mode, "default");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_is_not_replaced_by_the_new_modes_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace original-release"; }; }
        mode "other" { x release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("original-release")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "mode other"; }
            MouseLeft release=true { command "workspace mouse-released"; }
        }
        mode "other" { MouseLeft release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("mouse-released")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_dispatches_only_on_release() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { MouseLeft release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    pointer_button(&mut fixture, 0x110, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn pointer_button_event_dispatches_a_real_mouse_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace startup"; }
            MouseLeft { command "workspace clicked"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("clicked")
        .is_some());
}

#[test]
fn binding_modes_switch_binds_emit_events_and_list_over_ipc() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+R { command "mode resize"; }; }
        mode "resize" {
            Super+1 { command "workspace 7"; };
            Escape { command "mode default"; };
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode","binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/mode.resize.json"
    ))
    .unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    type_key_chords(&mut fixture, &[&[133, 10]]);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 5);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/binding.run.json"
    ))
    .unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$binding",
    );

    let swayward = fixture.swayward();
    // The bind switched to workspace 7, which is what this asserts. It is not
    // necessarily first: sway sorts numbered workspaces numerically
    // (sway/sway/tree/output.c:387-405), so the startup workspace 1 precedes it.
    assert!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .any(|workspace| workspace.num == 7 && workspace.focused)
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/mode.default.json"
    ))
    .unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    let mut query = UnixStream::connect(socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "resize"])
    );

    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"name": "default"})
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    let state = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(state, serde_json::json!({"name": "resize"}));
    assert_eq!(state.as_object().unwrap().len(), 1);
}

#[test]
fn command_bind_executes_the_sway_command_path() {
    let config = swayward_config::Config::parse_mem(
        "binds { Super+1 repeat=false { command \"workspace 7\"; }; }",
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::Keyboard {
            event: TestKeyEvent {
                device: TestDevice::keyboard("test keyboard"),
                key: 133,
                count: 1,
                state: smithay::backend::input::KeyState::Pressed,
            },
        },
    );
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .logo
    );
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::Keyboard {
            event: TestKeyEvent {
                device: TestDevice::keyboard("test keyboard"),
                key: 10,
                count: 2,
                state: smithay::backend::input::KeyState::Pressed,
            },
        },
    );

    let swayward = fixture.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(workspaces[0].num, 7);
}

#[test]
fn empty_workspace_commands_return_sway_failures() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for (command, error) in [
        (
            "focus floating",
            "Failed to find a floating container in workspace.",
        ),
        (
            "focus mode_toggle",
            "Failed to find a floating container in workspace.",
        ),
        (
            "focus tiling",
            "Failed to find a tiling container in workspace.",
        ),
        ("resize grow height 10 px", "Cannot resize nothing"),
        ("resize grow width 10 px", "Cannot resize nothing"),
        ("resize grow width 10 px or 5 ppt", "Cannot resize nothing"),
        ("resize set 50 ppt 50 ppt", "Cannot resize nothing"),
        ("resize shrink height 10 px", "Cannot resize nothing"),
        ("resize shrink width 10 px", "Cannot resize nothing"),
        ("resize invalid", "Cannot resize nothing"),
        ("scratchpad show", "Scratchpad is empty"),
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert_eq!(outcome.len(), 1, "{command}: {outcome:?}");
        assert!(!outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(outcome[0].error.as_deref(), Some(error), "{command}");
    }
}

#[test]
fn focus_floating_succeeds_when_a_floating_window_exists() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    for _ in 0..2 {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus floating")[0].success);
}

#[test]
fn criteria_with_no_matches_returns_sway_failure() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    assert_eq!(
        crate::command::execute(fixture.niri_state(), r#"[app_id="missing"] nop"#),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No matching node.".into()),
            parse_error: None,
        }]
    );
}

#[test]
fn layout_and_split_commands_preserve_a_focused_floating_window_and_the_tree() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    fixture.swayward().layout.toggle_window_floating(None);

    let mut stream = UnixStream::connect(socket).unwrap();
    for (command, expected_reply) in [
        ("split v", r#"[{"success":true}]"#),
        (
            "layout tabbed",
            r#"[{"success":false,"error":"Unable to change layout of floating windows"}]"#,
        ),
        (
            "layout toggle split",
            r#"[{"success":false,"error":"Unable to change layout of floating windows"}]"#,
        ),
    ] {
        let before = fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree();

        stream
            .write_all(&swayward_ipc::wire::encode(
                MessageType::RunCommand,
                command,
            ))
            .unwrap();
        let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
        assert_eq!(reply, expected_reply, "reply for {command}");

        let after = fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree();
        assert_eq!(after, before, "tree changed after {command}");
    }
}

fn add_two_tiled_windows(fixture: &mut Fixture) {
    let client = fixture.add_client();
    for app_id in ["left", "right"] {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}

fn tiled_window_rects(fixture: &mut Fixture) -> Vec<Value> {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn collect(value: &Value, rects: &mut Vec<Value>) {
        if value["type"] == "con" && value["app_id"].is_string() {
            rects.push(value["rect"].clone());
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    collect(child, rects);
                }
            }
        }
    }
    let mut rects = Vec::new();
    collect(&tree, &mut rects);
    rects.sort_by_key(|rect| rect["x"].as_i64().unwrap());
    rects
}

#[test]
fn reloaded_gap_defaults_do_not_change_an_existing_workspace() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let initial = swayward_config::Config::parse_mem(
        r#"layout {
            gaps 10
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(initial);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);
    assert_eq!(before[0]["y"], 8);
    assert_eq!(before[1]["y"], 8);

    let path = std::env::temp_dir().join(format!(
        "swayward-gap-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(
        &path,
        r#"layout {
            gaps 16
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "reload")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::from_str::<Value>(include_str!(
            "../../tests/fixtures/sway/events/workspace.reload.json"
        ))
        .unwrap()
    );
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 16.);
    assert_eq!(tiled_window_rects(&mut fixture), before);

    std::fs::remove_file(path).unwrap();
}

#[test]
fn runtime_gaps_all_changes_existing_workspaces() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.outer_gaps_configured = true;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner all set 16")[0].success);
    let after = tiled_window_rects(&mut fixture);
    assert_ne!(after, before);
    assert_eq!(after[0]["x"], 14);
    assert_eq!(after[0]["y"], 14);
    assert_eq!(after[1]["y"], 14);
}

#[test]
fn reload_rereads_config_and_emits_the_sway_workspace_event() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let path = std::env::temp_dir().join(format!(
        "swayward-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, "layout { gaps 7; }").unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "reload")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/workspace.reload.json"
    ))
    .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(), expected);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 7.);

    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_config_reload_keeps_the_compositor_responsive() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let malformed =
        swayward_config::Config::parse_mem("binds { Mod+H { command; }; }").map_err(|error| {
            assert!(format!("{error:?}").contains("expected command"));
        });
    assert!(malformed.is_err());

    fixture.niri_state().reload_config(malformed);

    let outcome = crate::command::execute(fixture.niri_state(), "nop");
    assert_eq!(outcome.len(), 1);
    assert!(outcome[0].success, "{outcome:?}");
}

#[test]
fn reload_replaces_map_time_rules_while_windows_are_mapped() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let first = fixture.client(client).create_window();
    first.xdg_toplevel.set_app_id("special".into());
    first.commit();
    let surface = first.surface.clone();
    fixture.roundtrip(client);
    let first = fixture.client(client).window(&surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let first_id = fixture.swayward().layout.focus().unwrap().id();

    super::i3_conformance::reload_test_config(
        &mut fixture,
        r#"for_window [app_id="special"] mark reloaded"#,
    )
    .unwrap();

    let second = fixture.client(client).create_window();
    second.xdg_toplevel.set_app_id("special".into());
    second.commit();
    let surface = second.surface.clone();
    fixture.roundtrip(client);
    let second = fixture.client(client).window(&surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let second_id = fixture.swayward().layout.focus().unwrap().id();

    let marks = &fixture.swayward().marks_by_window;
    assert!(marks.get(&first_id).is_none_or(Vec::is_empty));
    assert_eq!(
        marks.get(&second_id).map(Vec::as_slice),
        Some(["reloaded".to_owned()].as_slice())
    );
}

#[test]
fn title_format_updates_get_tree_and_titlebar_after_client_title_change() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("format-app".into());
    window.set_title("before");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    assert!(crate::command::execute(fixture.niri_state(), "border normal")[0].success);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[app_id="format-app"] title_format [%app_id|%shell|%class|%instance|%sandbox_engine|%sandbox_app_id|%sandbox_instance_id] %title"#,
    );
    assert!(outcome[0].success);

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "[format-app|xdg_shell|||||] before"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert_eq!(
        workspace.tiling().titlebar_titles(),
        ["[format-app|xdg_shell|||||] before"]
    );

    let window = fixture.client(client).window(&surface);
    window.set_title("%app_id");
    window.commit();
    fixture.double_roundtrip(client);

    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "[format-app|xdg_shell|||||] %app_id"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert_eq!(
        workspace.tiling().titlebar_titles(),
        ["[format-app|xdg_shell|||||] %app_id"]
    );

    assert!(crate::command::execute(fixture.niri_state(), "title_format %title")[0].success);
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "%app_id"
    );
}

#[test]
fn translated_for_window_nop_has_no_observable_window_effect() {
    fn mapped_leaf(config: Option<&str>) -> Value {
        let mut fixture = Fixture::new();
        fixture.add_output(1, (1920, 1080));
        if let Some(config) = config {
            super::i3_conformance::reload_test_config(&mut fixture, config).unwrap();
        }
        let client = fixture.add_client();
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id("nop-target".into());
        window.set_title("unchanged");
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);

        let swayward = fixture.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let mut leaf = find_json_node(&tree, "con", true).unwrap().clone();
        let leaf = leaf.as_object_mut().unwrap();
        leaf.remove("id");
        leaf.remove("foreign_toplevel_identifier");
        Value::Object(leaf.clone())
    }

    let baseline = mapped_leaf(Some(
        r#"for_window [app_id="^does-not-match$"] nop arbitrary comment text"#,
    ));
    let with_nop = mapped_leaf(Some(
        r#"for_window [app_id="^nop-target$"] nop arbitrary comment text"#,
    ));
    assert_eq!(with_nop, baseline);
}

#[test]
fn translated_map_time_sticky_command_executes_for_the_mapped_window() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            match app-id="^sticky-map$"
            open-floating true
            sway-for-window-command "sticky enable"
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("sticky-map".into());
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let floating = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0];
    assert_eq!(floating["app_id"], "sticky-map");
    assert_eq!(floating["sticky"], true);
}

#[test]
fn for_window_applies_matching_command_when_window_maps() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            r#"for_window [app_id="^dialog$"] floating enable"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([{"success": true}])
    );

    let id = fixture.add_client();
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("dialog".into());
    window.set_title("Dialog");
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "floating_con", false).unwrap()["app_id"],
        "dialog"
    );
}

#[test]
fn live_ipc_descriptions_match_sway_schema_and_values() {
    let config = swayward_config::Config::parse_mem(
        "layout { gaps 0; outer-gaps { left 0; right 0; top 0; bottom 0; }; border { on; width 2; }; }",
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1270, 1408));
    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let id = f.add_client();
    let window = f.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.set_title("fixture-1");
    window.set_size(696, 491);
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let mut stream = UnixStream::connect(&socket).unwrap();
    let ours = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.tree.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_fixture_string_values(&fixture, &ours, "$tree");
    assert_tree_values_match_fixture(&fixture, &ours, "$tree");
    assert_focus_matches_fixture(&fixture, &ours, "$tree");
    assert_percent_matches_fixture(&fixture, &ours, "$tree");
    assert_eq!(
        ours["nodes"][1]["nodes"][0]["nodes"][0]["geometry"],
        fixture["nodes"][1]["nodes"][0]["nodes"][0]["geometry"],
        "tiled leaf geometry must remain the client's natural map-time geometry"
    );
    assert_eq!(
        fixture["nodes"][1]["nodes"][0]["representation"],
        ours["nodes"][1]["nodes"][0]["representation"],
        "workspace representation at $tree.nodes[1].nodes[0]"
    );
    let fixture_trees = [
        include_str!("../../tests/fixtures/sway/empty.tree.json"),
        include_str!("../../tests/fixtures/sway/empty_named.tree.json"),
        include_str!("../../tests/fixtures/sway/fullscreen.tree.json"),
        include_str!("../../tests/fixtures/sway/marked.tree.json"),
        include_str!("../../tests/fixtures/sway/named_workspace.tree.json"),
        include_str!("../../tests/fixtures/sway/nested_h_in_v.tree.json"),
        include_str!("../../tests/fixtures/sway/numbered_sparse.tree.json"),
        include_str!("../../tests/fixtures/sway/one_floating.tree.json"),
        include_str!("../../tests/fixtures/sway/one_window.tree.json"),
        include_str!("../../tests/fixtures/sway/stacked.tree.json"),
        include_str!("../../tests/fixtures/sway/tabbed.tree.json"),
        include_str!("../../tests/fixtures/sway/two_split_h.tree.json"),
        include_str!("../../tests/fixtures/sway/two_split_v.tree.json"),
        include_str!("../../tests/fixtures/sway/two_workspaces.tree.json"),
    ];
    let mut fixture_nodes = Vec::new();
    for fixture in fixture_trees {
        collect_fixture_nodes(&serde_json::from_str(fixture).unwrap(), &mut fixture_nodes);
    }
    assert_node_schema_appears_in_fixtures(&ours, &fixture_nodes, "$tree");

    let scratch = &ours["nodes"][0];
    assert_eq!(scratch["name"], "__i3");
    assert_eq!(scratch["nodes"][0]["name"], "__i3_scratch");
    assert!(ours["nodes"][1]["nodes"][0]["nodes"][0]["app_id"].is_string());

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.workspaces.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$workspaces");

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.outputs.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$outputs");

    let output_name = f.niri_output(1).name();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(workspaces.as_array().unwrap().len(), 1);
    assert_eq!(workspaces[0]["num"], 1);
    assert_eq!(workspaces[0]["name"], "1");
    assert_eq!(workspaces[0]["output"], output_name);

    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(outputs.as_array().unwrap().len(), 1);
    assert_eq!(outputs[0]["name"], output_name);

    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "mark fixture-mark",
        ))
        .unwrap();
    let (_, outcome) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&outcome).unwrap(),
        serde_json::json!([{"success": true}])
    );
    let marks = query_ipc(&mut f, &mut stream, MessageType::GetMarks);
    assert_eq!(marks, serde_json::json!(["fixture-mark"]));
}

#[test]
fn focus_parent_then_layout_targets_the_parent_of_the_focused_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [None, Some("split v")] {
        if let Some(command) = command {
            assert!(crate::command::execute(f.niri_state(), command)[0].success);
        }
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    // `split v` retargets the singleton workspace root rather than wrapping it
    // (sway container.c:1565), so the two windows sit directly under the
    // workspace. `focus parent` then focuses that root, and `layout tabbed`
    // targets its parent, the workspace itself.
    assert_eq!(workspace["layout"], "tabbed");
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 2);
    assert!(workspace["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["focused"] == false));
}

#[test]
fn focus_child_from_workspace_restores_the_floating_child() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for floating in [false, true] {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }
    let floating = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_workspace_focused());
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), floating);

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    assert_ne!(f.swayward().layout.focus().unwrap().id(), floating);
}

#[test]
fn focused_container_can_be_marked_and_targeted_by_con_id() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    f.swayward().layout.consume_or_expel_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark parent")[0].success);
    let swayward = f.swayward();
    assert!(!swayward.marks_by_container.is_empty());
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let parent = find_json_node_with_mark(&tree, "parent").unwrap();
    let parent_id = parent["id"].as_i64().unwrap();

    let outcome = crate::command::execute(
        f.niri_state(),
        &format!("[con_id={parent_id}] layout tabbed"),
    );
    assert!(outcome[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["layout"],
        "tabbed"
    );

    let outcome = crate::command::execute(f.niri_state(), "[con_id=__focused__] layout stacked");
    assert!(outcome[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["layout"],
        "stacked"
    );
}

#[test]
fn container_mark_survives_singleton_flattening() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark survivor")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout toggle split")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=survivor] focus");
    assert!(outcome[0].success, "{outcome:?}");
}

#[test]
fn view_criteria_exclude_splits_but_container_criteria_include_them() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["first", "second", "third"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    f.swayward().layout.consume_or_expel_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark split")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let split_id = find_json_node_with_mark(&tree, "split").unwrap()["id"]
        .as_i64()
        .unwrap();

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=split] layout tabbed");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error, None);
    let outcome = crate::command::execute(
        f.niri_state(),
        &format!("[con_id={split_id}] layout stacking"),
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error, None);

    let outcome = crate::command::execute(f.niri_state(), "[all] kill");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error, None);
}

#[test]
fn focused_leaf_con_id_matches_get_tree_and_focused_criteria() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("focused-leaf".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let focused_id = find_json_node(&tree, "con", true).unwrap()["id"]
        .as_i64()
        .unwrap();

    for (criterion, mark) in [
        (format!(r#"con_id={focused_id}"#), "numeric"),
        ("con_id=__focused__".to_owned(), "focused"),
    ] {
        let result = crate::command::execute(
            f.niri_state(),
            &format!(r#"[{criterion} app_id="focused-leaf"] mark {mark}"#),
        );
        assert!(result[0].success, "{criterion}: {result:?}");
    }
    let focused = f.swayward().layout.focus().unwrap().id();
    assert_eq!(
        f.swayward().marks_by_window.get(&focused).unwrap(),
        &["focused".to_owned()]
    );

    crate::command::execute(f.niri_state(), &format!(r#"[id={focused_id}] mark x11-id"#));
    assert_eq!(
        f.swayward().marks_by_window.get(&focused).unwrap(),
        &["focused".to_owned()],
        "a native Wayland view must not expose its con_id as an X11 window id"
    );

    let result = crate::command::execute(f.niri_state(), "[con_id=not-a-number] nop");
    assert_eq!(result[0].parse_error, Some(true));
    assert_eq!(
        result[0].error.as_deref(),
        Some("The value for 'con_id' should be '__focused__' or numeric")
    );
}

#[test]
fn swap_con_id_and_mark_preserve_focus_and_reject_invalid_targets() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();
    for name in ["first", "second", "third"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(name.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("[con_id={}] focus", crate::ipc::tree::window_id(ids[0]))
        )[0]
        .success
    );
    assert!(crate::command::execute(f.niri_state(), "mark target")[0].success);
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("[con_id={}] focus", crate::ipc::tree::window_id(ids[2]))
        )[0]
        .success
    );
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!(
                "swap container with con_id {}",
                crate::ipc::tree::window_id(ids[1])
            )
        )[0]
        .success
    );
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert!(crate::command::execute(f.niri_state(), "swap container with mark target")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);

    let x11_id = crate::ipc::tree::window_id(ids[0]);
    let unknown_id =
        crate::command::execute(f.niri_state(), &format!("swap container with id {x11_id}"));
    assert_eq!(
        unknown_id[0].error.as_deref(),
        Some(format!("Failed to find id '{x11_id}'").as_str())
    );

    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let fourth = f.client(client).create_window();
    fourth.xdg_toplevel.set_app_id("fourth".into());
    fourth.commit();
    let surface = fourth.surface.clone();
    f.roundtrip(client);
    let fourth = f.client(client).window(&surface);
    fourth.attach_new_buffer();
    fourth.ack_last_and_commit();
    f.double_roundtrip(client);
    let fourth = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    let parent = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .focused_container_node()
        .unwrap();
    let child = crate::ipc::tree::window_id(fourth);
    let result = crate::command::execute(
        f.niri_state(),
        &format!("swap container with con_id {child}"),
    );
    assert_eq!(
        result[0].error.as_deref(),
        Some("Cannot swap ancestor and descendant")
    );
    assert_eq!(
        f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .focused_container_node(),
        Some(parent)
    );

    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    let self_id = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    for (command, expected) in [
        (
            "swap container with con_id 999",
            "Failed to find con_id '999'",
        ),
        (
            &format!("swap container with con_id {self_id}"),
            "Cannot swap a container with itself",
        ),
    ] {
        let result = crate::command::execute(f.niri_state(), command);
        assert_eq!(result[0].error.as_deref(), Some(expected));
    }
}

#[test]
fn map_time_marks_remain_globally_unique() {
    let mut config = swayward_config::Config::default();
    for app_id in ["first", "second"] {
        config.window_rules.push(swayward_config::WindowRule {
            matches: vec![swayward_config::window_rule::Match {
                app_id: Some(format!("^{app_id}$").parse().unwrap()),
                ..Default::default()
            }],
            sway_for_window_commands: vec!["mark --add shared".into()],
            ..Default::default()
        });
    }
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert_eq!(
        f.swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "shared"))
            .count(),
        1
    );
}

#[test]
fn marks_are_globally_unique_across_windows_and_containers() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark keep")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark --add unique")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace second")[0].success);
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark --toggle unique")[0].success);

    fn mark_count(state: &crate::swayward::State, expected: &str) -> usize {
        state
            .swayward
            .marks_by_window
            .values()
            .chain(state.swayward.marks_by_container.values())
            .flatten()
            .filter(|mark| mark.as_str() == expected)
            .count()
    }
    assert_eq!(mark_count(f.niri_state(), "unique"), 1);
    assert_eq!(mark_count(f.niri_state(), "keep"), 1);

    assert!(crate::command::execute(f.niri_state(), "mark --toggle unique")[0].success);
    assert_eq!(mark_count(f.niri_state(), "unique"), 0);
    assert_eq!(mark_count(f.niri_state(), "keep"), 1);

    assert!(crate::command::execute(f.niri_state(), "mark unique")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark unique")[0].success);
    assert_eq!(mark_count(f.niri_state(), "unique"), 1);
    assert!(f
        .swayward()
        .marks_by_container
        .values()
        .all(|marks| !marks.iter().any(|mark| mark == "unique")));

    assert!(crate::command::execute(f.niri_state(), "workspace empty")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark unique")[0].success);
    assert_eq!(mark_count(f.niri_state(), "unique"), 0);
}

#[test]
fn closing_last_window_focuses_workspace_node() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let mapped_id = f
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id())
                .then(|| mapped.id())
        })
        .unwrap();
    let focused_window = crate::ipc::tree::window_id(mapped_id);
    let focused_workspace =
        crate::ipc::tree::workspace_id(f.swayward().layout.active_workspace().unwrap().id().get());

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [focused_window]);

    let window = f.client(client).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    focused.clear();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [focused_workspace]);
}

#[test]
fn get_tree_has_one_focused_node_after_scratchpad_cycle() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["scratch", "tiled"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("inactive".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);

    for command in [
        r#"[app_id="scratch"] move scratchpad"#,
        "scratchpad show",
        "scratchpad show",
        "scratchpad show",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused.len(), 1, "focused nodes: {focused:?}");
    let focused_id = focused[0];
    let focused_workspace = tree
        .nodes
        .iter()
        .flat_map(|output| &output.nodes)
        .find(|workspace| {
            workspace
                .nodes
                .iter()
                .chain(&workspace.floating_nodes)
                .any(|node| node.id == focused_id)
        })
        .unwrap();
    assert_eq!(focused_workspace.focus.first(), Some(&focused_id));

    assert!(crate::command::execute(f.niri_state(), "workspace empty")[0].success);
    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(
        focused,
        [crate::ipc::tree::workspace_id(
            swayward.layout.active_workspace().unwrap().id().get()
        )]
    );
}

#[test]
fn scratchpad_hides_focused_window_and_show_cycles_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    let swayward = f.swayward();
    assert!(swayward.layout.focus().is_none());
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert_eq!(tree.nodes[0].nodes[0].floating_nodes.len(), 2);
    assert!(tree.nodes[0].nodes[0]
        .floating_nodes
        .iter()
        .all(|node| node.scratchpad_state.as_deref() == Some("fresh")));

    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let first = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let second = f.swayward().layout.focus().unwrap().id();
    assert_ne!(first, second);
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 2);
    assert_eq!(surfaces.len(), 2);
}

#[test]
fn directional_move_emits_one_settled_sway_move_event() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    for app_id in ["left", "moved"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.set_title(app_id);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let moved_id = f.swayward().layout.focus().unwrap().id();
    let before = {
        let swayward = f.swayward();
        serde_json::to_value(crate::ipc::tree::describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap()
    };
    let before =
        super::super::ipc::server::find_node_by_id(&before, crate::ipc::tree::window_id(moved_id))
            .unwrap()["rect"]["x"]
            .as_i64()
            .unwrap();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "move");
    assert_eq!(
        event["container"]["id"],
        crate::ipc::tree::window_id(moved_id)
    );
    assert!(event["container"]["rect"]["x"].as_i64().unwrap() < before);
    let expected: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/events/window.move.json"
    ))
    .unwrap();
    assert_event_shape(&expected, &event, "$window");
}

#[test]
fn scratchpad_show_moves_visible_window_to_current_workspace_and_focuses_it() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let scratchpad = f.client(client).create_window();
    scratchpad.xdg_toplevel.set_app_id("event-one".into());
    scratchpad.set_title("event-one");
    scratchpad.commit();
    let scratchpad_surface = scratchpad.surface.clone();
    f.roundtrip(client);
    let scratchpad = f.client(client).window(&scratchpad_surface);
    scratchpad.attach_new_buffer();
    scratchpad.ack_last_and_commit();
    f.double_roundtrip(client);
    let scratchpad_id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move to scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace target")[0].success);

    let tiled = f.client(client).create_window();
    tiled.commit();
    let tiled_surface = tiled.surface.clone();
    f.roundtrip(client);
    let tiled = f.client(client).window(&tiled_surface);
    tiled.attach_new_buffer();
    tiled.ack_last_and_commit();
    f.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let focused = f.swayward().layout.focus().unwrap();
    assert_eq!(focused.id(), scratchpad_id);
    let focused_window = focused.window.clone();
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("target"));
    assert!(workspace.has_window(&focused_window));
    let mut changes = Vec::new();
    for _ in 0..2 {
        let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
        assert_eq!(event_type, (1 << 31) | 3);
        changes.push(
            serde_json::from_str::<Value>(&payload).unwrap()["change"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert_eq!(changes, ["move", "focus"]);
}

#[test]
fn moving_fullscreen_window_to_scratchpad_clears_its_fullscreen_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let shown = f.swayward().layout.focus().unwrap().window.clone();
    assert_eq!(f.swayward().layout.fullscreen_mode(&shown), None);

    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    assert!(!f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_floating(&shown));
}

#[test]
fn workspace_fullscreen_descendant_does_not_move_to_an_adjacent_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("right".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    let second = f.client(client).create_window();
    second.commit();
    let second_surface = second.surface.clone();
    f.roundtrip(client);
    let second = f.client(client).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(client);
    for command in ["focus parent", "fullscreen enable", "focus child"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let id = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "move right")[0].success);

    let swayward = f.swayward();
    let (_, mapped) = swayward
        .layout
        .windows()
        .find(|(_, mapped)| mapped.id() == id)
        .unwrap();
    let output = swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&mapped.window))
        .and_then(|(monitor, _, _)| monitor)
        .unwrap()
        .output_name();
    assert_eq!(output, "left");
}

#[test]
fn targeted_fullscreen_toggle_replaces_another_windows_fullscreen() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    let first = f.swayward().layout.focus().unwrap().window.clone();
    map_test_window(&mut f, client, "second");
    let second = f.swayward().layout.focus().unwrap().window.clone();

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="first"] fullscreen enable"#)[0].success
    );
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="second"] fullscreen toggle"#)[0]
            .success
    );

    assert_eq!(f.swayward().layout.fullscreen_mode(&first), None);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&second),
        Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
    );
}

#[test]
fn targeted_global_fullscreen_selects_and_focuses_the_windows_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.set_title("target");
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let target = f.swayward().layout.focus().unwrap().window.clone();
    let target_workspace = f.swayward().layout.active_workspace().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "workspace other")[0].success);
    assert_ne!(
        f.swayward().layout.active_workspace().unwrap().id(),
        target_workspace
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[title="target"] fullscreen enable global"#
        )[0]
        .success
    );

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().id(),
        target_workspace
    );
    assert_eq!(f.swayward().layout.focus().unwrap().window, target);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&target),
        Some(crate::layout::tiling_tree::FullscreenMode::Global)
    );
}

#[test]
fn scratchpad_show_disables_target_workspace_and_global_fullscreen() {
    for fullscreen in ["fullscreen enable", "fullscreen enable global"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        let mut ids = Vec::new();

        for _ in 0..2 {
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
            ids.push(f.swayward().layout.focus().unwrap().id());
        }

        assert!(crate::command::execute(f.niri_state(), fullscreen)[0].success);
        assert!(f.swayward().layout.focused_fullscreen_mode().is_some());
        let first = crate::ipc::tree::window_id(ids[0]);
        assert!(
            crate::command::execute(f.niri_state(), &format!("[con_id={first}] move scratchpad"))
                [0]
            .success
        );
        assert!(
            crate::command::execute(f.niri_state(), &format!("[con_id={first}] scratchpad show"))
                [0]
            .success
        );

        assert_eq!(
            f.swayward().layout.focused_fullscreen_mode(),
            None,
            "{fullscreen}"
        );
        assert!(
            !f.swayward().layout.global_fullscreen_active(),
            "{fullscreen}"
        );
    }
}

#[test]
fn scratchpad_show_toggles_the_only_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for command in ["move scratchpad", "scratchpad show"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    assert!(f.swayward().layout.focus().is_some());
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
}

#[test]
fn empty_scratch_workspace_is_always_serialized() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    for command in ["move scratchpad", "scratchpad show"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert_eq!(tree.nodes[0].nodes[0].name.as_deref(), Some("__i3_scratch"));
    assert!(tree.nodes[0].nodes[0].floating_nodes.is_empty());
}

#[test]
fn get_workspaces_distinguishes_seat_focus_from_output_visibility() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(2);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.focused)
            .count(),
        1
    );
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.visible)
            .count(),
        2
    );
    assert!(
        workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .unwrap()
            .visible
    );
}

#[test]
fn workspace_commands_create_sparse_global_identities() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let client = f.add_client();
    for command in ["workspace 1", "workspace 3", "workspace 7"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.num, workspace.name.as_str(), workspace.focused))
            .collect::<Vec<_>>(),
        [(1, "1", false), (3, "3", false), (7, "7", true)]
    );
}

#[test]
fn focus_next_and_prev_follow_the_immediate_parent_layout() {
    for layout in ["splith", "splitv", "tabbed", "stacking"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        for app_id in ["first", "second"] {
            let window = f.client(client).create_window();
            window.xdg_toplevel.set_app_id(app_id.into());
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }

        assert!(crate::command::execute(f.niri_state(), &format!("layout {layout}"))[0].success);
        assert!(crate::command::execute(f.niri_state(), r#"[app_id="first"] focus"#)[0].success);
        let first = f.swayward().layout.focus().unwrap().id();

        let outcome = crate::command::execute(f.niri_state(), "focus next");
        assert!(outcome[0].success, "{layout}: {outcome:?}");
        assert_ne!(f.swayward().layout.focus().unwrap().id(), first, "{layout}");

        let outcome = crate::command::execute(f.niri_state(), "focus prev");
        assert!(outcome[0].success, "{layout}: {outcome:?}");
        assert_eq!(f.swayward().layout.focus().unwrap().id(), first, "{layout}");
    }

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(f.swayward().layout.focus().is_none());
    let outcome = crate::command::execute(f.niri_state(), "focus next");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(f.swayward().layout.focus().is_none());
}

#[test]
fn criteria_directional_move_uses_the_materialized_target_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["target", "middle", "target", "focused"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    let focused = f.swayward().layout.focus().unwrap().id();

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="target"] move right"#);

    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.clone().unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["middle", "target", "focused", "target"]);

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="target"] move left"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.clone().unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["target", "middle", "target", "focused"]);
}

#[test]
fn criteria_commands_do_not_change_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for app_id in ["target", "focused"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
    }
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="target"] mark selected"#)[0].success
    );

    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert_eq!(surfaces.len(), 2);
}

#[test]
fn multi_target_mark_moves_to_last_match_and_unmark_clears_every_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();

    for title in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("shared-app".into());
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="shared-app"] mark shared"#);
    assert_eq!(outcome.len(), 1);
    assert!(outcome[0].success);
    assert!(f
        .swayward()
        .marks_by_window
        .get(&ids[0])
        .is_none_or(Vec::is_empty));
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[1]).map(Vec::as_slice),
        Some(["shared".to_owned()].as_slice())
    );

    for (id, mark) in ids.iter().zip(["first", "second"]) {
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!(
                r#"[con_id="{}"] mark {mark}"#,
                crate::ipc::tree::window_id(*id)
            ),
        );
        assert!(outcome[0].success);
    }
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="shared-app"] unmark"#)[0].success);
    assert!(ids.iter().all(|id| f
        .swayward()
        .marks_by_window
        .get(id)
        .is_none_or(Vec::is_empty)));
}

#[test]
fn semicolon_starts_a_new_criteria_scope() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();
    for app_id in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    let outcomes = crate::command::execute(
        f.niri_state(),
        r#"[app_id="first"] mark first; [app_id="second"] mark second"#,
    );

    assert!(outcomes.iter().all(|outcome| outcome.success));
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[0]).map(Vec::as_slice),
        Some(["first".to_owned()].as_slice())
    );
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[1]).map(Vec::as_slice),
        Some(["second".to_owned()].as_slice())
    );
}

#[test]
fn comma_chain_keeps_the_original_criteria_targets() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "mark original")[0].success);
    let outcomes = crate::command::execute(
        f.niri_state(),
        "[con_mark=original] unmark original, mark retained",
    );
    assert!(outcomes.iter().all(|outcome| outcome.success));
    assert_eq!(
        f.swayward().marks_by_window.values().next().unwrap(),
        &["retained"]
    );
}

#[test]
fn output_workspaces_and_move_replacements_use_next_free_numbers() {
    let mut f = Fixture::new();
    f.add_named_output_at("fake-0".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("fake-1".into(), (100, 100), Some((100, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output fake-1")[0].success);
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.output.as_str(), workspace.name.as_str()))
            .collect::<Vec<_>>(),
        [("fake-0", "1"), ("fake-1", "2")]
    );

    assert!(crate::command::execute(f.niri_state(), "focus output fake-0")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move workspace to output fake-1")[0].success);
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.output == "fake-0")
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>(),
        ["3"]
    );
}

#[test]
fn rename_workspace_updates_name_number_and_rejects_collisions() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    for command in [
        "workspace 5",
        "rename workspace to 7: web",
        "workspace mail",
        "rename workspace mail to inbox",
        "rename workspace inbox to mail",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    let collision = crate::command::execute(f.niri_state(), "rename workspace mail to 7: web");
    assert!(!collision[0].success);
    for command in [
        "rename workspace mail to chat",
        "rename workspace chat to CHAT",
        "rename workspace chat to 9 web",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert!(!crate::command::execute(f.niri_state(), "rename workspace to next")[0].success);

    let swayward = f.swayward();
    assert_eq!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .map(|workspace| (workspace.num, workspace.name.as_str()))
            .collect::<Vec<_>>(),
        [(1, "1"), (7, "7: web"), (9, "9 web")]
    );
}

#[test]
fn tiled_and_floating_default_borders_remain_independent_in_get_tree() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            sway-border "pixel"
            sway-border-width 5
            sway-floating-border "normal"
            sway-floating-border-width 2
        }
        window-rule {
            match app-id="floating"
            open-floating true
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    for app_id in ["tiled", "floating"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let tiled = &workspace["nodes"][0];
    let floating = &workspace["floating_nodes"][0];
    assert_eq!(tiled["border"], "pixel");
    assert_eq!(tiled["current_border_width"], 5);
    assert_eq!(floating["border"], "normal");
    assert_eq!(floating["current_border_width"], 2);
}

#[test]
fn edge_border_modes_apply_to_workspace_edges_and_visible_view_count() {
    fn window_nodes(config: &str, windows: usize) -> Vec<Value> {
        let config = swayward_config::Config::parse_mem(config).unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (800, 600));
        let client = f.add_client();
        for _ in 0..windows {
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        let swayward = f.swayward();
        swayward.layout.update_render_elements(None);
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let workspace = &tree["nodes"][1]["nodes"][0];
        workspace["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(workspace["floating_nodes"].as_array().unwrap())
            .cloned()
            .collect()
    }

    let config = |mode: &str, smart: &str, gaps: u8| {
        format!(
            r#"layout {{
                gaps {gaps}
                hide-edge-borders "{mode}"
                smart-borders "{smart}"
            }}
            window-rule {{ sway-border "pixel"; sway-border-width 7; }}"#
        )
    };

    let vertical = window_nodes(&config("vertical", "off", 0), 2);
    assert_eq!(vertical[0]["window_rect"]["x"], 0);
    assert_eq!(vertical[0]["window_rect"]["y"], 7);
    assert_eq!(vertical[0]["window_rect"]["width"], 393);
    assert_eq!(vertical[0]["window_rect"]["height"], 586);
    assert_eq!(vertical[1]["window_rect"]["x"], 7);
    assert_eq!(vertical[1]["window_rect"]["y"], 7);
    assert_eq!(vertical[1]["window_rect"]["width"], 393);
    assert_eq!(vertical[1]["window_rect"]["height"], 586);

    let horizontal = window_nodes(&config("horizontal", "off", 0), 2);
    assert_eq!(horizontal[0]["window_rect"]["x"], 7);
    assert_eq!(horizontal[0]["window_rect"]["y"], 0);
    assert_eq!(horizontal[0]["window_rect"]["width"], 386);
    assert_eq!(horizontal[0]["window_rect"]["height"], 600);
    assert_eq!(horizontal[1]["window_rect"]["x"], 7);
    assert_eq!(horizontal[1]["window_rect"]["y"], 0);
    assert_eq!(horizontal[1]["window_rect"]["width"], 386);
    assert_eq!(horizontal[1]["window_rect"]["height"], 600);

    let smart_single = window_nodes(&config("none", "on", 0), 1);
    assert_eq!(
        smart_single[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
    let smart_two = window_nodes(&config("none", "on", 0), 2);
    assert!(smart_two.iter().all(|node| {
        node["window_rect"] == serde_json::json!({ "x": 7, "y": 7, "width": 386, "height": 586 })
    }));
    let smart_and_edges = window_nodes(&config("both", "on", 0), 2);
    assert_eq!(
        smart_and_edges[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 393, "height": 600 })
    );
    assert_eq!(
        smart_and_edges[1]["window_rect"],
        serde_json::json!({ "x": 7, "y": 0, "width": 393, "height": 600 })
    );

    let no_gaps = window_nodes(&config("none", "no-gaps", 0), 1);
    assert_eq!(
        no_gaps[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
    let with_gaps = window_nodes(&config("none", "no-gaps", 16), 1);
    assert_eq!(with_gaps[0]["window_rect"]["x"], 7);
    assert_eq!(with_gaps[0]["window_rect"]["y"], 7);

    let floating = window_nodes(
        &format!(
            "{}\nwindow-rule {{ open-floating true; }}",
            config("both", "on", 0)
        ),
        1,
    );
    assert_eq!(floating[0]["window_rect"]["x"], 7);
    assert_eq!(floating[0]["window_rect"]["y"], 7);
    assert_eq!(
        floating[0]["window_rect"]["width"].as_i64().unwrap(),
        floating[0]["rect"]["width"].as_i64().unwrap() - 14
    );
    assert_eq!(
        floating[0]["window_rect"]["height"].as_i64().unwrap(),
        floating[0]["rect"]["height"].as_i64().unwrap() - 14
    );
    assert_eq!(floating[0]["current_border_width"], 7);

    let initial = swayward_config::Config::parse_mem(&config("none", "off", 0)).unwrap();
    let mut f = Fixture::with_config(initial);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.niri_state()
        .reload_config(Ok(swayward_config::Config::parse_mem(&config(
            "both", "on", 0,
        ))
        .unwrap()));
    let swayward = f.swayward();
    swayward.layout.update_render_elements(None);
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let node = find_json_node(&tree, "con", false).unwrap();
    assert_eq!(
        node["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
}

#[test]
fn configured_border_width_matches_rendering_and_tree_for_tiled_and_floating_windows() {
    let config =
        swayward_config::Config::parse_mem(r#"layout { border { on; width 7; }; }"#).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for (node_type, floating) in [("con", false), ("floating_con", true)] {
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
        let swayward = f.swayward();
        let tile = swayward
            .layout
            .active_workspace()
            .unwrap()
            .tiles()
            .next()
            .unwrap();
        assert_eq!(tile.effective_border_width(), Some(7.));
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let node = find_json_node(&tree, node_type, false).unwrap();
        assert_eq!(node["border"], "normal");
        assert_eq!(node["current_border_width"], 7);
    }

    assert!(crate::command::execute(f.niri_state(), "border none")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let node = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(node["border"], "none");
    assert_eq!(node["current_border_width"], 0);
}

#[test]
fn border_command_updates_rendering_and_tree_metadata() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for (command, style, width, has_titlebar, rendered_width) in [
        ("border none", "none", 0, false, None),
        ("border pixel 3", "pixel", 3, false, Some(3.)),
        ("border normal 5", "normal", 5, true, Some(5.)),
        ("border toggle", "none", 0, false, None),
        ("border toggle", "pixel", 1, false, Some(1.)),
        ("border toggle", "normal", 2, true, Some(2.)),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let swayward = f.swayward();
        let mapped = swayward.layout.focus().unwrap();
        assert_eq!(
            swayward.layout.window_border(&mapped.window),
            Some((
                match style {
                    "none" => swayward_ipc::command::BorderStyle::None,
                    "pixel" => swayward_ipc::command::BorderStyle::Pixel,
                    "normal" => swayward_ipc::command::BorderStyle::Normal,
                    _ => unreachable!(),
                },
                width
            ))
        );
        let tile = swayward
            .layout
            .active_workspace()
            .unwrap()
            .tiles()
            .next()
            .unwrap();
        assert_eq!(tile.effective_border_width(), rendered_width);
        assert_eq!(tile.has_sway_titlebar(), has_titlebar);
        let tree = describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        );
        let node = tree
            .nodes
            .iter()
            .flat_map(|output| &output.nodes)
            .flat_map(|workspace| workspace.nodes.iter().chain(&workspace.floating_nodes))
            .next()
            .unwrap();
        assert_eq!(format!("{:?}", node.border).to_ascii_lowercase(), style);
        assert_eq!(node.current_border_width, i32::from(width));
    }

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    for (command, style, width) in [("border none", "none", 0), ("border pixel 7", "pixel", 7)] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let node = find_json_node(&tree, "floating_con", false).unwrap();
        assert_eq!(node["border"], style, "{command}");
        assert_eq!(node["current_border_width"], width, "{command}");
    }
}

#[test]
fn border_csd_fails_without_client_decoration_support() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "border csd");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("This window doesn't support client side decorations")
    );
}

#[test]
fn criteria_targeted_move_workspace_moves_all_matches_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["special", "special", "ordinary"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="ordinary"] focus"#)[0].success);
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="special"] move workspace target"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let workspaces = f.swayward().layout.workspaces().collect::<Vec<_>>();
    let source = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() != Some("target"))
        .unwrap()
        .2;
    let target = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
        .unwrap()
        .2;
    assert_eq!(source.windows().count(), 1);
    assert_eq!(
        source.active_window().and_then(|window| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
        }),
        Some("ordinary".into())
    );
    assert_eq!(target.windows().count(), 2);
}

#[test]
fn criteria_move_workspace_to_output_uses_the_matched_workspace() {
    let mut f = Fixture::new();
    f.add_named_output_at("west".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("east".into(), (100, 100), Some((200, 0)));
    let client = f.add_client();

    assert!(
        crate::command::execute(f.niri_state(), "focus output middle, workspace target")
            .iter()
            .all(|outcome| outcome.success)
    );
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("moveme".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus output west")[0].success);

    let workspace_output = |f: &mut Fixture| {
        f.swayward()
            .layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
            .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output_name().clone()))
            .unwrap()
    };

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="moveme"] move workspace to output right"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(workspace_output(&mut f), "east");

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[workspace="target"] move workspace to middle"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(workspace_output(&mut f), "middle");

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[workspace="target"] move workspace to output missing"#,
    );
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Can't find output with name/direction 'missing'")
    );
    assert_eq!(workspace_output(&mut f), "middle");
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "west");

    let mut f = Fixture::new();
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("east".into(), (100, 100), Some((200, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let floating = f.swayward().layout.focus().unwrap().window.clone();
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    f.swayward().layout.move_floating_window(
        Some(&floating),
        swayward_ipc::legacy::PositionChange::AdjustFixed(20.),
        swayward_ipc::legacy::PositionChange::AdjustFixed(10.),
        false,
    );
    let old_center = f.swayward().layout.window_center(&floating).unwrap();

    assert!(crate::command::execute(f.niri_state(), "move workspace to output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "east");
    let new_center = f.swayward().layout.window_center(&floating).unwrap();
    assert_eq!(new_center.x - old_center.x, 100);
    assert_eq!(new_center.y, old_center.y);
    let missing = crate::command::execute(f.niri_state(), "move workspace to output right");
    assert!(!missing[0].success);
    assert_eq!(
        missing[0].error.as_deref(),
        Some("Can't find output with name/direction 'right'")
    );
}

#[test]
fn criteria_move_workspace_ignores_hidden_scratchpad_matches() {
    let mut f = Fixture::new();
    f.add_named_output_at("fake-0".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("fake-1".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();

    for (output, workspace, app_id, scratchpad) in [
        ("fake-0", "ws0", "a", false),
        ("fake-1", "ws1", "b", false),
        ("fake-1", "ws1", "c", true),
    ] {
        let command = format!("focus output {output}, workspace {workspace}");
        assert!(crate::command::execute(f.niri_state(), &command)
            .iter()
            .all(|outcome| outcome.success));
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if scratchpad {
            assert!(crate::command::execute(f.niri_state(), "move to scratchpad")[0].success);
        }
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id=".*"] move workspace to output fake-1"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let swayward = f.swayward();
    assert!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .any(|workspace| workspace.name == "ws0" && workspace.output == "fake-1")
    );
}

#[test]
fn cross_workspace_swap_exchanges_positions_marks_and_fullscreen() {
    let mut f = Fixture::new();
    f.add_output_at(1, (600, 800), Some((0, 0)));
    f.add_output_at(2, (1000, 800), Some((600, 0)));
    let client = f.add_client();

    let mut windows = Vec::new();
    for (output, workspace, mark, fullscreen) in [
        ("headless-1", "one", "A", true),
        ("headless-2", "two", "B", false),
    ] {
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {output}, workspace {workspace}")
        )
        .iter()
        .all(|outcome| outcome.success));
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mapped = f.swayward().layout.focus().unwrap();
        windows.push((workspace, mapped.id(), mapped.window.clone()));
        assert!(crate::command::execute(f.niri_state(), &format!("mark {mark}"))[0].success);
        if fullscreen {
            assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
        }
    }

    let result = crate::command::execute(f.niri_state(), "[con_mark=B] swap container with mark A");
    assert!(result[0].success, "{result:?}");
    let (one_id, two_id) = {
        let layout = &f.swayward().layout;
        let one = layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("one"))
            .unwrap()
            .2
            .id();
        let two = layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("two"))
            .unwrap()
            .2
            .id();
        (one, two)
    };
    assert_eq!(
        f.swayward().layout.window_workspace_id(&windows[0].2),
        Some(two_id)
    );
    assert_eq!(
        f.swayward().layout.window_workspace_id(&windows[1].2),
        Some(one_id)
    );
    for workspace in [one_id, two_id] {
        let tree = f
            .swayward()
            .layout
            .workspaces()
            .find(|(_, _, candidate)| candidate.id() == workspace)
            .unwrap()
            .2
            .ipc_tiling_tree();
        assert_eq!(tree.nodes().len(), 2);
    }
    let first_center = f.swayward().layout.window_center(&windows[0].2).unwrap();
    let second_center = f.swayward().layout.window_center(&windows[1].2).unwrap();
    assert!(first_center.x >= 600, "{first_center:?}");
    assert!(second_center.x < 600, "{second_center:?}");
    assert_eq!(f.swayward().layout.fullscreen_mode(&windows[0].2), None);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&windows[1].2),
        Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
    );
    assert!(f
        .swayward()
        .marks_by_window
        .get(&windows[0].1)
        .is_some_and(|marks| marks.as_slice() == ["A"]));
    assert!(f
        .swayward()
        .marks_by_window
        .get(&windows[1].1)
        .is_some_and(|marks| marks.as_slice() == ["B"]));
    assert_eq!(
        f.swayward()
            .marks_by_window
            .values()
            .flatten()
            .filter(|mark| *mark == "A" || *mark == "B")
            .count(),
        2
    );
}

#[test]
fn criteria_targeted_move_workspace_preserves_a_container_subtree() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace source")[0].success);
    for app_id in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="first"] focus"#)[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark group")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace target")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=group] move workspace target");
    assert!(outcome[0].success, "{outcome:?}");
    let workspaces = f.swayward().layout.workspaces().collect::<Vec<_>>();
    let source = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("source"))
        .unwrap()
        .2;
    let target = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
        .unwrap()
        .2;
    assert_eq!(source.windows().count(), 0);
    assert_eq!(target.windows().count(), 3);
    assert_eq!(target.ipc_tiling_tree().nodes().len(), 5);
    assert!(target.windows().any(|window| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
            == Some("first".into())
    }));
}

#[test]
fn criteria_targeted_scratchpad_show_toggles_every_matching_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.set_title("toggle-window");
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    for expected_hidden in [0, 2, 0] {
        let outcome =
            crate::command::execute(f.niri_state(), r#"[title="toggle-"] scratchpad show"#);
        assert!(outcome[0].success, "{outcome:?}");
        assert_eq!(
            f.swayward().layout.scratchpad_windows().count(),
            expected_hidden
        );
    }
    for expected_hidden in [1, 2] {
        assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
        assert_eq!(
            f.swayward().layout.scratchpad_windows().count(),
            expected_hidden
        );
    }
}

#[test]
fn criteria_targeted_scratchpad_show_toggles_each_match_from_its_own_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut windows = Vec::new();
    for title in ["mixed-toggle-1", "mixed-toggle-2"] {
        let window = f.client(client).create_window();
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mapped = f
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .active_window()
            .unwrap();
        windows.push((mapped.window.clone(), mapped.id()));
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    let [(first, first_id), (second, _)] = windows.as_slice() else {
        unreachable!()
    };
    let first_id = crate::ipc::tree::window_id(*first_id);
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("[con_id={first_id}] scratchpad show")
        )[0]
        .success
    );
    assert!(!f.swayward().layout.is_scratchpad_hidden(first));
    assert!(f.swayward().layout.is_scratchpad_hidden(second));

    let outcome =
        crate::command::execute(f.niri_state(), r#"[title="mixed-toggle-"] scratchpad show"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert!(f.swayward().layout.is_scratchpad_hidden(first));
    assert!(!f.swayward().layout.is_scratchpad_hidden(second));
}

#[test]
fn criteria_targeted_scratchpad_commands_move_only_the_matching_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["ordinary", "special"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="special"] move scratchpad"#)[0].success
    );
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    let ordinary = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .active_window()
        .unwrap();
    assert_eq!(
        crate::utils::with_toplevel_role(ordinary.toplevel(), |role| role.app_id.clone()),
        Some("ordinary".into())
    );
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="special"] scratchpad show"#)[0].success
    );
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.windows().count(), 2);
    let active = workspace.active_window().unwrap();
    assert_eq!(
        crate::utils::with_toplevel_role(active.toplevel(), |role| role.app_id.clone()),
        Some("special".into())
    );
}

fn dialog_rect_after_parent_move(animations_off: bool) -> Value {
    let mut config = swayward_config::Config::default();
    config.animations.off = animations_off;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let first = f.client(client).create_window();
    first.commit();
    let first_surface = first.surface.clone();
    f.roundtrip(client);
    let first = f.client(client).window(&first_surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    f.double_roundtrip(client);

    let parent = f.client(client).create_window();
    parent.xdg_toplevel.set_app_id("parent".into());
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    if !animations_off {
        assert!(f.swayward().layout.are_animations_ongoing(None));
    }

    let child = f.client(client).create_window();
    child.xdg_toplevel.set_app_id("dialog".into());
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn find_app(value: &Value) -> Option<&Value> {
        if value["app_id"] == "dialog" {
            return Some(value);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| value[key].as_array()?.iter().find_map(find_app))
    }
    find_app(&tree).unwrap()["rect"].clone()
}

fn fullscreen_parent_after_child_map(
    policy: swayward_config::PopupDuringFullscreen,
) -> (bool, bool) {
    let mut config = swayward_config::Config {
        popup_during_fullscreen: policy,
        ..Default::default()
    };
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    let parent = f.client(client).create_window();
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);
    let parent_id = f.swayward().layout.focus().unwrap().id();
    let parent_window = f.swayward().layout.focus().unwrap().window.clone();
    f.swayward().layout.set_fullscreen(&parent_window, true);

    let child = f.client(client).create_window();
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    (
        f.swayward()
            .layout
            .fullscreen_mode(&parent_window)
            .is_some(),
        f.swayward().layout.focus().unwrap().id() != parent_id,
    )
}

#[test]
fn popup_during_fullscreen_policies_use_xdg_toplevel_parent() {
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::Smart),
        (true, true)
    );
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::Ignore),
        (true, false)
    );
    assert_eq!(
        fullscreen_parent_after_child_map(swayward_config::PopupDuringFullscreen::LeaveFullscreen),
        (false, false)
    );
}

#[test]
fn dialog_placement_uses_parent_layout_position_during_animation() {
    assert_eq!(
        dialog_rect_after_parent_move(false),
        dialog_rect_after_parent_move(true)
    );
}

#[test]
fn disabled_focus_follows_mouse_keeps_focus_when_pointer_crosses_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1024, 768));
    f.add_output(2, (1024, 768));
    let client = f.add_client();

    f.niri_focus_output(2);
    let focused = f.client(client).create_window();
    focused.commit();
    let surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused_id = f.swayward().layout.focus().unwrap().id();

    let location = (500., 0.).into();
    let under = f.swayward().contents_under(location);
    f.swayward().handle_focus_follows_mouse(&under);
    f.niri_state().move_cursor(location);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused_id);
    let active = f.swayward().layout.active_output().unwrap().clone();
    assert_eq!(active, f.niri_output(2));
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        location
    );
}

#[test]
fn dialog_with_hidden_scratchpad_parent_does_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let parent = f.client(client).create_window();
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let child = f.client(client).create_window();
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(f.swayward().layout.windows().count(), 2);
}

#[test]
fn floating_rejects_hidden_scratchpad_window_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="hidden"] floating enable"#);
    assert_eq!(outcome.len(), 1);
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Can't change floating on hidden scratchpad container")
    );
}

#[test]
fn resize_rejects_hidden_scratchpad_window_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="hidden"] resize grow width 10 px"#,
    );
    assert_eq!(outcome.len(), 1);
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot resize a hidden scratchpad container")
    );
}

#[test]
fn bare_directional_move_crosses_each_adjacent_output_without_wrapping() {
    let mut f = Fixture::new();
    for (name, position) in [
        ("top-left", (0, 0)),
        ("top-right", (800, 0)),
        ("bottom-right", (800, 600)),
        ("bottom-left", (0, 600)),
    ] {
        f.add_named_output_at(name.into(), (800, 600), Some(position));
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {name}, workspace {name}-workspace")
        )
        .iter()
        .all(|outcome| outcome.success));
    }

    assert!(crate::command::execute(f.niri_state(), "workspace top-left-workspace")[0].success);
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let window_id = f.swayward().layout.focus().unwrap().id();

    for (command, expected_output) in [
        ("move right", "top-right"),
        ("move down", "bottom-right"),
        ("move left", "bottom-left"),
        ("move up", "top-left"),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.id() == window_id)
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected_output,
            "{command}"
        );
    }
}

#[test]
fn criteria_directional_move_crosses_outputs_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    let moved = f.client(client).create_window();
    moved.xdg_toplevel.set_app_id("moved".into());
    moved.commit();
    let surface = moved.surface.clone();
    f.roundtrip(client);
    let moved = f.client(client).window(&surface);
    moved.attach_new_buffer();
    moved.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let focused = f.client(client).create_window();
    focused.commit();
    let surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="moved"] move right"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert!(f
        .swayward()
        .layout
        .windows()
        .find(
            |(_, mapped)| crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
                role.app_id.as_deref() == Some("moved")
            })
        )
        .unwrap()
        .0
        .is_some_and(|monitor| monitor.output_name() == "right"));
}

#[test]
fn criteria_move_output_right_uses_layout_positions_during_workspace_animation() {
    let mut f = Fixture::new();
    for (name, position) in [
        ("top-left", (0, 0)),
        ("top-right", (800, 0)),
        ("bottom-left", (0, 600)),
        ("bottom-right", (800, 600)),
    ] {
        f.add_named_output_at(name.into(), (800, 600), Some(position));
    }
    for (output, workspace) in [
        ("top-left", "top-left-workspace"),
        ("top-right", "top-right-workspace"),
        ("bottom-left", "bottom-left-workspace"),
        ("bottom-right", "bottom-right-workspace"),
    ] {
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {output}, workspace {workspace}")
        )
        .iter()
        .all(|outcome| outcome.success));
    }

    let client = f.add_client();
    for workspace in ["top-left-workspace", "bottom-left-workspace"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("moveme".into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(f.swayward().layout.are_animations_ongoing(None));
    let bottom = f
        .swayward()
        .layout
        .windows()
        .find(|(monitor, mapped)| {
            monitor.is_some_and(|monitor| monitor.output_name() == "bottom-left")
                && crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
                    role.app_id.as_deref() == Some("moveme")
                })
        })
        .map(|(_, mapped)| mapped.window.clone())
        .unwrap();
    assert!(f.swayward().layout.window_center(&bottom).unwrap().y >= 600);
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output right"#)[0]
            .success
    );
    let workspace_counts = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace
                .name()
                .map(|name| (name.to_owned(), workspace.windows().count()))
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(workspace_counts["top-right-workspace"], 1);
    assert_eq!(workspace_counts["bottom-right-workspace"], 1);
}

#[test]
fn move_output_direction_uses_the_windows_output_and_stops_at_the_edge() {
    let mut f = Fixture::new();
    f.add_named_output_at("right".into(), (100, 100), Some((200, 100)));
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("left".into(), (100, 100), Some((0, 100)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("moveme".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let window_id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    for expected in ["middle", "left"] {
        assert!(
            crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output left"#)[0]
                .success
        );
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.id() == window_id)
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected
        );
    }

    let outcome =
        &crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output left"#)[0];
    assert!(!outcome.success);
    assert_eq!(
        outcome.error.as_deref(),
        Some("Can't find output with name/direction 'left'")
    );
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == window_id)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        "left"
    );
}

#[test]
fn move_output_accepts_direction_name_current_and_workspace_forms() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let outputs = [f.niri_output(1).name(), f.niri_output(2).name()];
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move output current")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move output right")[0].success);
    let focused = f.swayward().layout.focus().unwrap().id();
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == focused)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        &outputs[1]
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("move container to output {}", outputs[0])
        )[0]
        .success
    );
    assert!(crate::command::execute(f.niri_state(), "move workspace output right")[0].success);
}

#[test]
fn sticky_accepts_sway_boolean_words_and_reports_tree_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    for (value, expected) in [
        ("enable", true),
        ("toggle", false),
        ("enabled", true),
        ("off", false),
        ("yes", true),
        ("0", false),
        ("1", true),
        ("no", false),
        ("on", true),
        ("disable", false),
        ("active", true),
        ("unknown", false),
    ] {
        assert!(crate::command::execute(f.niri_state(), &format!("sticky {value}"))[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        assert_eq!(
            find_json_node(&tree, "floating_con", false).unwrap()["sticky"],
            expected
        );
    }
}

#[test]
fn sticky_without_a_container_matches_sway_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert_eq!(
        crate::command::execute(f.niri_state(), "sticky enable")[0]
            .error
            .as_deref(),
        Some("No current container")
    );
}

#[test]
fn workspace_criteria_uses_sparse_and_named_sway_identities() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for workspace in ["1", "7", "mail"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(workspace.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    for (workspace, mark) in [("7", "sparse"), ("mail", "named")] {
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!(r#"[workspace="^{workspace}$"] mark {mark}"#)
            )[0]
            .success
        );
    }

    let swayward = f.swayward();
    let marked_apps = swayward
        .layout
        .windows()
        .filter_map(|(_, window)| {
            swayward.marks_by_window.get(&window.id()).map(|marks| {
                let app_id = crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.clone().unwrap()
                });
                (app_id, marks.clone())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        marked_apps,
        [
            ("7".into(), vec!["sparse".into()]),
            ("mail".into(), vec!["named".into()])
        ]
    );
}

#[test]
fn workspace_next_and_prev_on_output_wrap_in_stored_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for workspace in ["1", "5", "6:a", "6:b"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "workspace next_on_output")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("1".into())
    );
    assert!(crate::command::execute(f.niri_state(), "workspace prev_on_output")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("6:b".into())
    );
}

#[test]
fn workspace_next_and_prev_cross_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let first_output = f.niri_output(1).name();
    let second_output = f.niri_output(2).name();
    let client = f.add_client();
    for (workspace, output) in [("1", &first_output), ("2", &second_output)] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!("workspace {workspace} output {output}")
            )[0]
            .success
        );
    }
    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        first_output
    );
    assert!(crate::command::execute(f.niri_state(), "workspace next")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        second_output
    );
}

#[test]
fn killing_focused_workspace_closes_tiled_and_floating_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 9")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    for floating in [false, true] {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }

    let victims = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .windows()
        .map(|window| window.window.clone())
        .collect::<Vec<_>>();
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "kill")[0].success);
    f.double_roundtrip(client);

    assert_eq!(
        f.client(client)
            .state
            .windows
            .iter()
            .filter(|window| window.close_requested)
            .count(),
        2
    );
    for victim in victims {
        f.swayward()
            .layout
            .remove_window(&victim, crate::utils::transaction::Transaction::new());
    }
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.number(), Some(7));
    assert_eq!(workspace.windows().count(), 0);
    let mut numbers = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| workspace.number())
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    // Workspace 1 is gone, not missing. It was created empty with the output,
    // and focus left it for workspace 9 without ever placing a window on it,
    // so sway destroys it. Measured on real sway 1.11 (headless, one output):
    // focusing an empty workspace 7 then switching away leaves
    // get_workspaces reporting ['1', '2', '9'] with no 7, while an empty
    // workspace that still holds focus is reported. See
    // workspace_consider_destroy, sway/tree/workspace.c:313-330, reached from
    // seat_set_focus, sway/input/seat.c:1244.
    assert_eq!(numbers, [7, 9]);
}

#[test]
fn closing_last_window_removes_inactive_named_workspace_from_ipc() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let mapped = f
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id())
                .then(|| mapped.window.clone())
        })
        .unwrap();

    assert!(crate::command::execute(f.niri_state(), "workspace active")[0].success);
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);
    f.swayward()
        .layout
        .remove_window(&mapped, crate::utils::transaction::Transaction::new());
    f.niri_state().ipc_refresh_layout();

    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    let names = workspaces
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["active"]);
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    assert!(tree.to_string().contains("active"));
    assert!(!tree.to_string().contains(r#"\"name\":\"7\""#));

    let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected = serde_json::from_str::<Value>(include_str!(
        "../../tests/fixtures/sway/events/workspace.empty.json"
    ))
    .unwrap();
    assert_eq!(
        actual.as_object().unwrap().keys().collect::<BTreeSet<_>>(),
        expected
            .as_object()
            .unwrap()
            .keys()
            .collect::<BTreeSet<_>>()
    );
    assert_same_shape(
        &expected["current"],
        &actual["current"],
        "$workspace.current",
    );
    assert_eq!(actual["change"], "empty");
    assert_eq!(actual["current"]["name"], "7");
    assert_eq!(actual["current"]["focused"], false);
    assert_eq!(actual["current"]["nodes"], serde_json::json!([]));

    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    let after_prev = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(
        after_prev
            .as_array()
            .unwrap()
            .iter()
            .find(|workspace| workspace["focused"] == true)
            .unwrap()["name"],
        "active"
    );

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let recreated = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(
        recreated
            .as_array()
            .unwrap()
            .iter()
            .filter(|workspace| workspace["num"] == 7)
            .count(),
        1
    );
}

#[test]
fn initial_workspace_name_comes_from_the_first_available_default_mode_binding() {
    for (config, expected) in [
        (
            r#"binds {
                code:24 { command "workspace keycode-first"; }
                X { command "workspace keysym-second"; }
            }"#,
            "keycode-first",
        ),
        (
            r#"binds {
                X { command "workspace keysym-first"; }
                code:24 { command "workspace keycode-second"; }
            }"#,
            "keysym-first",
        ),
        (
            r#"binds {
                X { command "workspace next"; }
                Y { command "workspace prev"; }
                Z { command "workspace next_on_output"; }
                A { command "workspace prev_on_output"; }
                B { command "workspace back_and_forth"; }
                C { command "workspace current"; }
                D { command "workspace number"; }
                code:24 { command "workspace number 7: eggs"; }
            }"#,
            "7: eggs",
        ),
        (
            r#"binds {
                X { focus-workspace "typed"; }
                Y { command "workspace string-second"; }
            }"#,
            "typed",
        ),
        (
            r#"binds {
                X { focus-workspace 7; }
            }
            mode "other" {
                Y { command "workspace ignored-mode"; }
            }"#,
            "7",
        ),
        (
            r#"binds {
                X { command "workspace   3"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace 3; exec foo"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace 3"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace --no-auto-back-and-forth number 3:three"; }
            }"#,
            "3:three",
        ),
    ] {
        let config = swayward_config::Config::parse_mem(config).unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1920, 1080));
        assert_eq!(
            f.swayward().layout.active_workspace().unwrap().sway_name(),
            Some(expected.to_owned())
        );
    }

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for (command, expected) in [
        ("workspace foobar", "foobar"),
        ("workspace   3", "3"),
        ("workspace 3; exec foo", "3"),
        ("workspace 3", "3"),
        (
            "workspace --no-auto-back-and-forth number 3:three",
            "3:three",
        ),
    ] {
        let config = swayward_config::Config::parse_mem(&format!(
            "binds {{\n    X {{ command {command:?}; }}\n}}"
        ))
        .unwrap();
        f.swayward()
            .layout
            .initialize_workspaces_from_bindings(&config);
        assert_eq!(
            f.swayward().layout.active_workspace().unwrap().sway_name(),
            Some(expected.to_owned())
        );
    }

    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace taken"; }
            code:24 { command "workspace fresh"; }
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("taken".to_owned())
    );
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(2);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("fresh".to_owned())
    );
}

#[test]
fn configured_workspace_is_destroyed_when_empty_and_inactive() {
    let config = swayward_config::Config::parse_mem(r#"workspace "configured" {}"#).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    assert!(
        crate::command::execute(f.niri_state(), "rename workspace configured to renamed")[0]
            .success
    );
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    f.swayward().clock.set_complete_instantly(true);
    f.swayward().layout.advance_animations();
    f.swayward().clock.set_complete_instantly(false);

    assert!(!f
        .swayward()
        .layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("renamed")));
}

#[test]
fn named_workspace_has_no_number_and_active_empty_workspace_remains_visible() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "workspace mail")[0].success);
    f.niri_state().ipc_refresh_layout();

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(workspaces.len(), 2);
    let numbered = workspaces
        .iter()
        .find(|workspace| workspace.num == 1)
        .unwrap();
    assert_eq!(numbered.name, "1");
    let named = workspaces
        .iter()
        .find(|workspace| workspace.name == "mail")
        .unwrap();
    assert_eq!(named.num, -1);
    assert!(named.visible);
    assert!(named.focused);
}

#[test]
fn negative_and_unnumbered_workspace_names_report_minus_one_without_affecting_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    for workspace in ["mail", "-42: negative", "7: numbered"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }
    assert!(crate::command::execute(f.niri_state(), "rename workspace mail to inbox")[0].success);
    f.niri_state().ipc_refresh_layout();

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.name.as_str(), workspace.num))
            .collect::<Vec<_>>(),
        [
            ("1", 1),
            ("7: numbered", 7),
            ("inbox", -1),
            ("-42: negative", -1)
        ]
    );
}

#[test]
fn relative_move_includes_empty_active_workspace_and_uses_direction() {
    for (source, direction) in [(1, "next"), (3, "prev")] {
        let mut f = Fixture::new();
        for output in 1..=3 {
            f.add_output(output, (1920, 1080));
        }
        let outputs = [
            f.niri_output(1).name(),
            f.niri_output(2).name(),
            f.niri_output(3).name(),
        ];
        let client = f.add_client();

        assert!(crate::command::execute(f.niri_state(), &format!("workspace {source}"))[0].success);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        for workspace in 1..=3 {
            assert!(
                crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0]
                    .success
            );
            assert!(
                crate::command::execute(
                    f.niri_state(),
                    &format!("workspace {workspace} output {}", outputs[workspace - 1])
                )[0]
                .success
            );
        }

        assert!(crate::command::execute(
            f.niri_state(),
            &format!("workspace {source}, move workspace {direction}")
        )
        .iter()
        .all(|outcome| outcome.success));

        let swayward = f.swayward();
        let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
        let window_counts = workspaces
            .iter()
            .map(|workspace| (workspace.num, workspace.focus.len()))
            .collect::<Vec<_>>();
        assert_eq!(window_counts, [(1, 0), (2, 1), (3, 0)]);
    }
}

#[test]
fn targeted_focus_reveals_a_hidden_scratchpad_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.set_title("target");
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(f.niri_state(), r#"[title="target"] focus workspace"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 0);
    assert!(f.swayward().layout.focus().is_some());
}

fn set_test_window_urgent(f: &mut Fixture, app_id: &str) {
    f.swayward().layout.with_windows_mut(|window, _| {
        if crate::utils::with_toplevel_role(window.toplevel(), |role| {
            role.app_id.as_deref() == Some(app_id)
        }) {
            window.set_urgent(true);
        }
    });
}

fn test_window_is_urgent(f: &mut Fixture, app_id: &str) -> bool {
    f.swayward()
        .layout
        .windows()
        .find(|(_, window)| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| {
                role.app_id.as_deref() == Some(app_id)
            })
        })
        .unwrap()
        .1
        .is_urgent()
}

#[test]
fn focusing_an_urgent_window_clears_urgency_immediately() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["target", "focused"] {
        map_test_window(&mut f, client, app_id);
    }
    set_test_window_urgent(&mut f, "target");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspaces = serde_json::to_value(describe_workspaces(
        &swayward.layout,
        &swayward.global_space,
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "target").unwrap()["urgent"],
        true
    );
    assert!(workspaces
        .as_array()
        .unwrap()
        .iter()
        .any(|ws| ws["urgent"] == true));

    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "target"));
    set_test_window_urgent(&mut f, "target");
    assert!(!test_window_is_urgent(&mut f, "target"));
}

#[test]
fn urgent_criteria_selects_windows_by_urgency_timestamp() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["oldest", "latest", "focused"] {
        map_test_window(&mut f, client, app_id);
    }
    set_test_window_urgent(&mut f, "oldest");
    std::thread::sleep(Duration::from_millis(2));
    set_test_window_urgent(&mut f, "latest");

    assert!(crate::command::execute(f.niri_state(), "[urgent=oldest] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "oldest"));
    assert!(crate::command::execute(f.niri_state(), "[urgent=latest] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "latest"));
}

#[test]
fn cross_workspace_focus_delays_urgency_clear_without_restarting_timer() {
    let config = swayward_config::Config {
        urgent_timeout_ms: 40,
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "other");
    set_test_window_urgent(&mut f, "target");

    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert!(test_window_is_urgent(&mut f, "target"));
    std::thread::sleep(Duration::from_millis(25));
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    std::thread::sleep(Duration::from_millis(25));
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "target"));
}

#[test]
fn closing_a_window_cancels_its_pending_urgency_timer() {
    let config = swayward_config::Config {
        urgent_timeout_ms: 20,
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    let target_surface = f.client(client).state.windows[0].surface.clone();
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "other");
    set_test_window_urgent(&mut f, "target");
    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().urgency_timers.len(), 1);

    f.client(client).window(&target_surface).attach_null();
    f.client(client).window(&target_surface).commit();
    f.double_roundtrip(client);
    assert!(f.swayward().urgency_timers.is_empty());
    std::thread::sleep(Duration::from_millis(30));
    f.double_roundtrip(client);
}

#[test]
fn targeted_focus_selects_the_requested_unfocused_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["first", "middle", "last"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="middle"] focus"#)[0].success);

    let focused_app_id = f.swayward().layout.focus().map(|window| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
    });
    assert_eq!(focused_app_id, Some(Some("middle".into())));
}

#[test]
fn workspace_auto_back_and_forth_honors_global_and_command_settings() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = true;

    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }

    // Enabled + active target + previous workspace: bounce.
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );

    // A different target switches normally instead of bouncing.
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(2)
    );

    // The command-level override suppresses the enabled global option.
    assert!(
        crate::command::execute(f.niri_state(), "workspace --no-auto-back-and-forth 2",)[0].success
    );
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(2)
    );

    // With the option disabled, selecting the active workspace remains there.
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = false;
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );
}

#[test]
fn move_no_auto_back_and_forth_changes_the_same_workspace_destination() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = true;

    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);

    let client = f.add_client();
    for app_id in ["normal", "suppressed"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="normal"] move workspace 1"#)[0].success
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[app_id="suppressed"] move --no-auto-back-and-forth window to workspace 1"#,
        )[0]
        .success
    );

    let workspace_apps = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace.number().map(|number| {
                let apps = workspace
                    .windows()
                    .filter_map(|window| {
                        crate::utils::with_toplevel_role(window.toplevel(), |role| {
                            role.app_id.clone()
                        })
                    })
                    .collect::<Vec<_>>();
                (number, apps)
            })
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(workspace_apps[&2], ["normal"]);
    assert_eq!(workspace_apps[&1], ["suppressed"]);
}

#[test]
fn workspace_back_and_forth_without_history_uses_sway_error() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "workspace back_and_forth",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(
        reply,
        r#"[{"success":false,"error":"There is no previous workspace"}]"#
    );
}

#[test]
fn workspace_back_and_forth_recreates_a_reaped_previous_workspace() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();

    for command in ["workspace 1", "workspace 2"] {
        stream
            .write_all(&swayward_ipc::wire::encode(
                MessageType::RunCommand,
                command,
            ))
            .unwrap();
        let (_, reply) = read_ipc_reply(&mut f, &mut stream);
        assert_eq!(reply, r#"[{"success":true}]"#);
    }

    f.swayward().clock.set_complete_instantly(true);
    f.swayward().layout.advance_animations();
    f.swayward().clock.set_complete_instantly(false);
    assert!(f
        .swayward()
        .layout
        .workspaces()
        .all(|(_, _, workspace)| workspace.number() != Some(1)));

    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "workspace back_and_forth",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(reply, r#"[{"success":true}]"#);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );
}

#[test]
fn move_workspace_back_and_forth_targets_the_previous_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(workspace.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "move workspace back_and_forth")[0].success);

    let workspace_apps = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace.number().map(|number| {
                let apps = workspace
                    .windows()
                    .map(|window| {
                        crate::utils::with_toplevel_role(window.toplevel(), |role| {
                            role.app_id.clone().unwrap()
                        })
                    })
                    .collect::<Vec<_>>();
                (number, apps)
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workspace_apps,
        [(1, vec!["1".into(), "2".into()]), (2, vec![])]
    );
}

#[test]
fn move_workspace_current_keeps_the_window_on_its_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move workspace current")[0].success);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    // Find workspace 7 rather than assuming it is first: sway sorts numbered
    // workspaces numerically (sway/sway/tree/output.c:387-405), so the startup
    // workspace 1 precedes it. The point of this test is that the window stays
    // on the workspace it was on, not where that workspace sorts.
    let seven = workspaces
        .iter()
        .find(|workspace| workspace.num == 7)
        .expect("workspace 7 exists");
    assert_eq!(seven.focus.len(), 1);
}

#[test]
fn move_to_workspace_creates_the_target_and_moves_the_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move to workspace 7")[0].success);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    let workspace = workspaces
        .iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.focus.len(), 1);
    assert!(!workspace.focused);
}

#[test]
fn workspace_output_assignment_moves_the_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let target_output = f.niri_output(2).name();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let command = format!("workspace 7 output {target_output}");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    let swayward = f.swayward();
    let workspace = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.output, target_output);
}

#[test]
fn ipc_output_rects_use_global_positions() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let swayward = f.swayward();
    let outputs = describe_outputs(&swayward.layout, &swayward.global_space);
    let rects = outputs.iter().map(|output| output.rect).collect::<Vec<_>>();
    assert_eq!(rects[0].x, 0);
    assert_eq!(rects[0].width, 1280);
    assert_eq!(rects[1].x, 1280);
    assert_eq!(rects[1].width, 1920);

    let root = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(root.rect.width, 3200);
    assert_eq!(root.rect.height, 1080);
}

#[test]
fn stale_tree_leaf_is_omitted_without_panicking() {
    let tree = IpcNode::Leaf {
        id: NodeId(1),
        window: (),
        percent: Some(1.),
        focused: false,
        rect: Default::default(),
        deco_rect: None,
        border: (swayward_ipc::command::BorderStyle::Normal, 2),
        border_edges: crate::utils::ResizeEdge::all(),
    };
    assert!(crate::ipc::tree::describe_tiling(
        tree,
        &|_| None,
        Default::default(),
        &Default::default(),
        &Default::default(),
        crate::layout::workspace::WorkspaceId::specific(1)
    )
    .is_none());

    let tree = IpcNode::Split {
        id: NodeId(0),
        layout: TreeLayout::SplitH,
        percent: None,
        rect: Default::default(),
        focus: vec![NodeId(1)],
        focused: false,
        fullscreen_mode: 0,
        children: vec![IpcNode::Leaf {
            id: NodeId(1),
            window: (),
            percent: Some(1.),
            focused: false,
            rect: Default::default(),
            deco_rect: None,
            border: (swayward_ipc::command::BorderStyle::Normal, 2),
            border_edges: crate::utils::ResizeEdge::all(),
        }],
    };
    let node = crate::ipc::tree::describe_tiling(
        tree,
        &|_| None,
        Default::default(),
        &Default::default(),
        &Default::default(),
        crate::layout::workspace::WorkspaceId::specific(1),
    )
    .unwrap();
    assert!(node.nodes.is_empty());
}

#[test]
fn live_ipc_focus_matches_sway_mru_arrays() {
    assert_focus_matches_fixture(&nested_fixture_tree(), &nested_live_tree(), "$tree");
}

#[test]
fn workspace_focus_spans_tiled_and_floating_children() {
    let expected = mixed_fixture_tree();
    let actual = mixed_live_tree();
    assert_focus_matches_fixture(&expected, &actual, "$tree");

    let workspace = &actual["nodes"][1]["nodes"][0];
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(workspace["floating_nodes"].as_array().unwrap().len(), 1);
    assert_eq!(workspace["focus"].as_array().unwrap().len(), 2);
}

fn floating_order(tree: &Value) -> (Vec<&str>, Vec<&str>) {
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] != "__i3")
        .unwrap()["nodes"][0]
        .as_object()
        .unwrap();
    let floating = workspace["floating_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let focus = workspace["focus"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|id| {
            workspace["floating_nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["id"] == *id)
                .and_then(|node| node["app_id"].as_str())
        })
        .collect::<Vec<_>>();
    (floating, focus)
}

#[test]
fn runtime_command_refusals_are_sway_shaped() {
    let mut f = Fixture::new();
    for (command, error) in [
        (
            "opacity 0.5",
            "opacity requires mutable per-container opacity support",
        ),
        (
            "inhibit_idle visible",
            "inhibit_idle requires user inhibitor policy support",
        ),
        (
            "urgent allow",
            "urgent allow|deny requires client urgency-request policy support",
        ),
    ] {
        assert_eq!(
            crate::command::execute(f.niri_state(), command),
            [swayward_ipc::CommandOutcome {
                success: false,
                error: Some(error.into()),
                parse_error: Some(true),
            }]
        );
    }
}

#[test]
fn shortcuts_inhibitor_disable_sets_future_policy_and_deactivates_current() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let current = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert_eq!(f.client(client).state.shortcut_inhibitor_events, [true]);

    let outcome = crate::command::execute(f.niri_state(), "shortcuts_inhibitor disable");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert_eq!(
        f.client(client).state.shortcut_inhibitor_events,
        [true, false]
    );

    current.destroy();
    f.double_roundtrip(client);
    f.client(client).state.shortcut_inhibitor_events.clear();
    let future = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert!(f.client(client).state.shortcut_inhibitor_events.is_empty());

    let outcome = crate::command::execute(f.niri_state(), "shortcuts_inhibitor enable");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert!(f.client(client).state.shortcut_inhibitor_events.is_empty());

    future.destroy();
    f.double_roundtrip(client);
    let _enabled_future = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert_eq!(f.client(client).state.shortcut_inhibitor_events, [true]);
}

#[test]
fn runtime_presentation_command_refusals_are_explicit() {
    let mut f = Fixture::new();
    for (command, error) in [
        (
            "allow_tearing yes",
            "allow_tearing requires immediate presentation support",
        ),
        (
            "max_render_time 1",
            "max_render_time requires per-view render deadline support",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].error.as_deref(), Some(error), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
    }
    assert_eq!(
        crate::command::execute(f.niri_state(), "max_render_time")[0]
            .error
            .as_deref(),
        Some("Missing max render time argument.")
    );
}

#[test]
fn create_output_adds_a_headless_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = f
        .swayward()
        .global_space
        .outputs()
        .map(smithay::output::Output::name)
        .collect::<Vec<_>>();
    assert_eq!(outputs, ["headless-1", "headless-2"]);
    assert_eq!(
        f.niri_output(2).current_mode().unwrap().size,
        (1920, 1080).into()
    );

    let removed = f.niri_output(2);
    f.swayward().remove_output(&removed);
    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = f
        .swayward()
        .global_space
        .outputs()
        .map(smithay::output::Output::name)
        .collect::<Vec<_>>();
    assert_eq!(outputs, ["headless-1", "headless-3"]);
}

#[test]
fn urgent_command_updates_a_hidden_scratchpad_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden-urgent".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let other = f.client(client).create_window();
    other.commit();
    let other_surface = other.surface.clone();
    f.roundtrip(client);
    let other = f.client(client).window(&other_surface);
    other.attach_new_buffer();
    other.ack_last_and_commit();
    f.double_roundtrip(client);
    let outcome = crate::command::execute(f.niri_state(), "[app_id=hidden-urgent] urgent enable");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(test_window_is_urgent(&mut f, "hidden-urgent"));
}

#[test]
fn urgent_command_changes_only_an_unfocused_selected_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    for app_id in ["target", "focused"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let urgent = |f: &mut Fixture| {
        f.swayward()
            .layout
            .windows()
            .find(|(_, window)| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.as_deref() == Some("target")
                })
            })
            .unwrap()
            .1
            .is_urgent()
    };

    for (command, expected) in [
        ("[app_id=target] urgent enable", true),
        ("[app_id=target] urgent toggle", false),
        ("[app_id=target] urgent toggle", true),
        ("[app_id=target] urgent disable", false),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(urgent(&mut f), expected, "{command}");
    }
}

#[test]
fn scratchpad_show_remaps_floating_center_between_asymmetric_outputs() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (683, 768), Some((0, 0)));
    f.add_named_output_at("right".into(), (1024, 768), Some((683, 0)));
    assert!(
        crate::command::execute(f.niri_state(), "focus output left, workspace left")[0].success
    );
    assert!(
        crate::command::execute(f.niri_state(), "focus output right, workspace right")[0].success
    );
    assert!(crate::command::execute(f.niri_state(), "workspace left")[0].success);

    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", true).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move position 40 px 100 px")[0].success);
    let left = rect(&mut f);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let right = rect(&mut f);
    assert_eq!(right["x"], 743, "left={left} right={right}");
    assert_ne!(right["x"], left["x"]);

    assert!(crate::command::execute(f.niri_state(), "move position 600 px 100 px")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let left_again = rect(&mut f);
    assert_eq!(left_again["x"], 400);
    assert_ne!(left_again["x"], right["x"]);
}

#[test]
fn initially_floating_window_uses_and_clamps_client_size() {
    fn mapped_rect(requested: (u16, u16), honor_requested_size: bool) -> Value {
        let mut config = swayward_config::Config::default();
        config.animations.off = true;
        config.window_rules.push(swayward_config::WindowRule {
            open_floating: Some(true),
            ..Default::default()
        });
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1280, 800));
        let client = f.add_client();
        let window = f.client(client).create_window();
        window.set_size(requested.0, requested.1);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(
            if honor_requested_size {
                requested.0
            } else {
                1280
            },
            if honor_requested_size {
                requested.1
            } else {
                800
            },
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let requested = f
            .swayward()
            .layout
            .focus()
            .unwrap()
            .expected_size()
            .unwrap();
        let window = f.client(client).window(&surface);
        window.set_size(
            requested.w.try_into().unwrap(),
            requested.h.try_into().unwrap(),
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false)
            .unwrap()
            .clone()
    }

    let requested = mapped_rect((400, 150), true);
    assert_eq!(requested["geometry"]["width"], 400);
    assert_eq!(requested["geometry"]["height"], 150);
    assert_eq!(requested["rect"]["width"], 400);
    assert_eq!(requested["rect"]["height"], 150);

    let clamped = mapped_rect((1600, 1000), true);
    assert_eq!(clamped["geometry"]["width"], 1600);
    assert_eq!(clamped["geometry"]["height"], 1000);
    assert_eq!(clamped["rect"]["width"], 1280);
    assert_eq!(clamped["rect"]["height"], 800);

    assert_ne!(
        mapped_rect((400, 150), false)["geometry"],
        requested["geometry"]
    );
}

#[test]
fn tiled_grow_at_workspace_edge_reports_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "resize grow right 10 px");

    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot resize any further")
    );
}

#[test]
fn floating_grow_edges_change_origin_and_size_like_sway() {
    let config = swayward_config::Config::parse_mem("animations { off; }").unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output_at(1, (1280, 800), Some((100, 50)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "resize set 400 px 300 px")[0].success);
    let requested = f
        .swayward()
        .layout
        .focus()
        .unwrap()
        .expected_size()
        .unwrap();
    let window = f.client(client).window(&surface);
    window.set_size(
        requested.w.try_into().unwrap(),
        requested.h.try_into().unwrap(),
    );
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let rect = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    for (direction, delta) in [
        ("left", (-10, 0, 10, 0)),
        ("right", (0, 0, 10, 0)),
        ("up", (0, -10, 0, 10)),
        ("down", (0, 0, 0, 10)),
    ] {
        let before = rect(&mut f);
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!("resize grow {direction} 10 px or 25 ppt"),
        );
        assert!(outcome[0].success, "{direction}: {outcome:?}");
        let requested = f
            .swayward()
            .layout
            .focus()
            .unwrap()
            .expected_size()
            .unwrap();
        let window = f.client(client).window(&surface);
        window.set_size(
            requested.w.try_into().unwrap(),
            requested.h.try_into().unwrap(),
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let after = rect(&mut f);
        assert_eq!(
            after["x"].as_i64(),
            before["x"].as_i64().map(|x| x + delta.0)
        );
        assert_eq!(
            after["y"].as_i64(),
            before["y"].as_i64().map(|y| y + delta.1)
        );
        assert_eq!(
            after["width"].as_i64(),
            before["width"].as_i64().map(|width| width + delta.2)
        );
        assert_eq!(
            after["height"].as_i64(),
            before["height"].as_i64().map(|height| height + delta.3)
        );
    }

    assert!(crate::command::execute(f.niri_state(), "resize set 1280 px 800 px")[0].success);
    let requested = f
        .swayward()
        .layout
        .focus()
        .unwrap()
        .expected_size()
        .unwrap();
    let window = f.client(client).window(&surface);
    window.set_size(
        requested.w.try_into().unwrap(),
        requested.h.try_into().unwrap(),
    );
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let before = rect(&mut f);
    assert_eq!(before["width"], 1280);
    assert_eq!(before["height"], 800);
    let outcome = crate::command::execute(f.niri_state(), "resize grow right 10 px or 25 ppt");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot resize any further")
    );
    assert_eq!(rect(&mut f), before);
}

#[test]
fn move_command_rejects_fullscreen_floating_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move left");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot move fullscreen floating container")
    );
}

#[test]
fn move_command_uses_sway_floating_pixel_distances() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };
    let before = rect(&mut f);

    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    let moved = rect(&mut f);
    assert_eq!(moved["x"].as_i64(), before["x"].as_i64().map(|x| x - 10));

    assert!(crate::command::execute(f.niri_state(), "move down 20 px")[0].success);
    let moved = rect(&mut f);
    assert_eq!(moved["y"].as_i64(), before["y"].as_i64().map(|y| y + 20));
}

#[test]
fn move_position_uses_workspace_coordinates_and_rejects_absolute_ppt() {
    let mut f = Fixture::new();
    f.add_output(1, (1000, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move position 5 px 15")[0].success);
    assert_eq!(rect(&mut f)["x"], 5);
    assert_eq!(rect(&mut f)["y"], 15);

    assert!(crate::command::execute(f.niri_state(), "move position 20 ppt 25 ppt")[0].success);
    assert_eq!(rect(&mut f)["x"], 200);
    assert_eq!(rect(&mut f)["y"], 200);

    for command in [
        "move absolute position 20 ppt 5 px",
        "move absolute position 5 px 20 ppt",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success);
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Cannot move to absolute positions by ppt")
        );
    }
}

#[test]
fn move_position_centers_on_root_and_pointer() {
    let mut f = Fixture::new();
    f.add_output_at(1, (1000, 800), Some((100, 50)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move position 150 75")[0].success);
    let relative = rect(&mut f);
    assert_eq!(relative["x"], 250);
    assert_eq!(relative["y"], 125);

    assert!(crate::command::execute(f.niri_state(), "move absolute position 150 75")[0].success);
    let absolute = rect(&mut f);
    assert_eq!(absolute["x"], 150);
    assert_eq!(absolute["y"], 75);

    assert!(crate::command::execute(f.niri_state(), "move absolute position center")[0].success);
    let centered = rect(&mut f);
    assert_eq!(centered["x"], 600);
    assert_eq!(centered["y"], 450);

    f.niri_state().move_cursor((300., 250.).into());
    assert!(crate::command::execute(f.niri_state(), "move position pointer")[0].success);
    let pointer = rect(&mut f);
    assert_eq!(pointer["x"], 300);
    assert_eq!(pointer["y"], 250);
}

#[test]
fn move_position_targets_floating_windows_by_criteria() {
    let mut f = Fixture::new();
    f.add_output(1, (1000, 800));
    let client = f.add_client();
    for app_id in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    }

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[app_id="first"] move position 25 px 30 px"#,
        )[0]
        .success
    );

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn find_first(value: &Value) -> Option<&Value> {
        if value["app_id"] == "first" {
            return Some(value);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| value[key].as_array()?.iter().find_map(find_first))
    }
    let first = find_first(&tree).unwrap();
    assert_eq!(first["rect"]["x"], 25);
    assert_eq!(first["rect"]["y"], 30);
}

#[test]
fn floating_ipc_rect_uses_final_position_during_animation() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("animated".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.swayward().layout.toggle_window_floating(None);
    f.swayward().layout.move_floating_window(
        None,
        swayward_ipc::PositionChange::SetFixed(100.),
        swayward_ipc::PositionChange::SetFixed(200.),
        true,
    );

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let node = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(node["rect"]["x"], 100);
    assert_eq!(node["rect"]["y"], 200);
}

#[test]
fn floating_input_region_holes_click_through_but_decorations_activate() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.border.off = false;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for app_id in ["bottom", "top"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        assert!(
            crate::command::execute(f.niri_state(), "resize set width 200 height 100")[0].success
        );
        let window = f.client(client).window(&surface);
        window.set_size(200, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let id = f.swayward().layout.focus().unwrap().window.clone();
        f.swayward().layout.move_floating_window(
            Some(&id),
            swayward_ipc::PositionChange::SetFixed(100.),
            swayward_ipc::PositionChange::SetFixed(100.),
            false,
        );
        surfaces.push(surface);
    }

    let focused_app_id = |f: &mut Fixture| {
        f.swayward().layout.focus().and_then(|window| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
        })
    };
    let (bottom, tile_pos, window_loc) = {
        let workspace = f.swayward().layout.active_workspace().unwrap();
        let bottom = workspace
            .windows()
            .find(|window| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.as_deref() == Some("bottom")
                })
            })
            .unwrap()
            .window
            .clone();
        let (tile, tile_pos, _) = workspace
            .tiles_with_render_positions()
            .find(|(tile, _, _)| {
                crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                    role.app_id.as_deref() == Some("top")
                })
            })
            .unwrap();
        (bottom, tile_pos, tile.window_loc())
    };
    let inside = tile_pos + window_loc + smithay::utils::Point::from((25., 25.));
    let outside = tile_pos + window_loc + smithay::utils::Point::from((175., 25.));
    let border_in_tile = smithay::utils::Point::from((window_loc.x / 2., window_loc.y + 10.));

    f.client(client)
        .set_input_region(&surfaces[1], Some((0, 0, 100, 100)));
    f.double_roundtrip(client);
    f.niri_state().move_cursor(inside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("top"));

    f.swayward().layout.activate_window_without_raising(&bottom);
    let output = f.niri_output(1);
    assert_eq!(
        f.swayward()
            .layout
            .window_under(&output, outside)
            .and_then(|(window, _)| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
            })
            .as_deref(),
        Some("bottom")
    );
    f.niri_state().move_cursor(outside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("bottom"));

    f.client(client).set_input_region(&surfaces[1], None);
    f.double_roundtrip(client);
    f.niri_state().move_cursor(inside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("bottom"));

    f.client(client).reset_input_region(&surfaces[1]);
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="top"] focus"#)[0].success);
    f.swayward().layout.activate_window_without_raising(&bottom);
    f.niri_state().move_cursor(outside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("top"));

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="top"] focus"#)[0].success);
    let workspace = f.swayward().layout.active_workspace().unwrap();
    let (tile, _, _) = workspace
        .tiles_with_render_positions()
        .find(|(tile, _, _)| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.as_deref() == Some("top")
            })
        })
        .unwrap();
    assert_eq!(
        tile.hit(border_in_tile),
        Some(crate::layout::HitType::Activate {
            is_tab_indicator: false
        })
    );
}

#[test]
fn floating_stacking_and_focus_match_sway_before_and_after_raise() {
    let two: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/two_floating.tree.json"
    ))
    .unwrap();
    assert_eq!(
        floating_order(&two),
        (
            vec!["fixture-1", "fixture-2"],
            vec!["fixture-2", "fixture-1"]
        )
    );

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for title in ["fixture-tiled", "fixture-1", "fixture-2", "fixture-3"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(title.into());
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if title != "fixture-tiled" {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }

    let describe = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap()
    };
    let before: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/three_floating_before_raise.tree.json"
    ))
    .unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&before));

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="^fixture-1$"] focus"#)[0].success);
    let after: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/three_floating_after_raise.tree.json"
    ))
    .unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&after));
}

#[test]
fn live_ipc_rectangle_roles_match_sway_relationships() {
    let expected = nested_fixture_tree();
    let actual = nested_live_tree();
    assert_rectangle_roles_match_fixture(&expected, &actual, "$tree");
}

#[test]
fn nested_tiling_rectangles_match_sway_roles() {
    let tree = nested_live_tree();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let top = &workspace["nodes"][0];
    let nested = &workspace["nodes"][1];
    let bottom_left = &nested["nodes"][0];
    let bottom_right = &nested["nodes"][1];

    assert_eq!(
        nested["rect"]["x"],
        bottom_left["rect"]["x"].as_i64().unwrap()
            - bottom_left["deco_rect"]["x"].as_i64().unwrap()
    );
    assert!(nested["rect"]["x"].as_i64().unwrap() > top["rect"]["x"].as_i64().unwrap());
    assert_eq!(
        nested["rect"]["y"],
        bottom_left["rect"]["y"].as_i64().unwrap()
            - bottom_left["deco_rect"]["height"].as_i64().unwrap()
    );
    assert_eq!(nested["rect"]["width"], bottom_left["rect"]["width"]);
    assert_eq!(
        nested["rect"]["height"],
        bottom_right["rect"]["y"].as_i64().unwrap()
            + bottom_right["rect"]["height"].as_i64().unwrap()
            - nested["rect"]["y"].as_i64().unwrap()
    );
    for window in [top, bottom_left, bottom_right] {
        assert!(window["deco_rect"]["height"].as_i64().unwrap() > 0);
        assert_eq!(window["current_border_width"], 4);
        assert_eq!(window["window_rect"]["x"], 4);
        assert_eq!(window["window_rect"]["y"], 0);
        assert_eq!(
            window["window_rect"]["width"].as_i64().unwrap(),
            window["rect"]["width"].as_i64().unwrap() - 8
        );
        assert_eq!(
            window["window_rect"]["height"].as_i64().unwrap(),
            window["rect"]["height"].as_i64().unwrap() - 4
        );
    }
}

#[test]
fn border_none_zeroes_deco_and_uses_the_whole_rect_for_window() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "border none")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let window = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(
        window["deco_rect"],
        serde_json::json!({"x": 0, "y": 0, "width": 0, "height": 0})
    );
    assert_eq!(window["window_rect"]["x"], 0);
    assert_eq!(window["window_rect"]["y"], 0);
    assert_eq!(window["window_rect"]["width"], window["rect"]["width"]);
    assert_eq!(window["window_rect"]["height"], window["rect"]["height"]);
}

#[test]
fn live_ipc_percent_matches_sway_parent_shares() {
    let expected = nested_fixture_tree();
    let representation = nested_representation_live_tree();
    assert_eq!(
        expected["nodes"][1]["nodes"][0]["representation"],
        representation["nodes"][1]["nodes"][0]["representation"],
        "workspace representation at $tree.nodes[1].nodes[0]"
    );
    let actual = nested_live_tree();
    assert_percent_value_matches_fixture(
        &expected["nodes"][1]["nodes"][0]["nodes"][1],
        &actual["nodes"][1]["nodes"][0]["nodes"][1],
        "$tree.nodes[1].nodes[0].nodes[1]",
    );
    assert_percent_matches_fixture(&expected, &actual, "$tree");
}

#[test]
fn root_focus_lists_outputs_once_in_global_mru_order() {
    let mut f = Fixture::new();
    for output in 1..=3 {
        f.add_output(output, (1280, 720));
    }
    let client = f.add_client();
    for output in [1, 2, 3] {
        f.niri_focus_output(output);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let root_focus = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .focus
    };
    let output_ids = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .nodes
        .into_iter()
        .filter(|node| node.name.as_deref() != Some("__i3"))
        .map(|node| node.id)
        .collect::<Vec<_>>()
    };

    let ids = output_ids(&mut f);
    assert_eq!(root_focus(&mut f), [ids[2], ids[1], ids[0]]);
    f.niri_focus_output(1);
    assert_eq!(root_focus(&mut f), [ids[0], ids[2], ids[1]]);
    let focus = root_focus(&mut f);
    assert_eq!(focus.len(), ids.len());
    assert_eq!(
        focus.iter().collect::<std::collections::HashSet<_>>().len(),
        ids.len()
    );
}

#[test]
fn criteria_focus_output_ignores_hidden_scratchpad_match_and_uses_seat_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left-head".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right-head".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let command = format!(r#"[con_id="{hidden}"] focus output right-head"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "left-head"
    );
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn criteria_focus_output_succeeds_without_an_output_and_keeps_scratchpad_hidden() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let output = f.swayward().layout.active_output().unwrap().clone();

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(f.swayward().layout.active_output(), Some(&output));
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn focus_output_prefers_a_name_over_a_direction_and_resolves_directions() {
    let mut f = Fixture::new();
    f.add_named_output_at("origin".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("left".into(), (1280, 720), Some((1280, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
    assert!(crate::command::execute(f.niri_state(), "focus output origin")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
}

#[test]
fn focus_output_uses_nearest_geometry_then_wraps_to_farthest_opposite() {
    let mut f = Fixture::new();
    f.add_named_output_at("west".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("center".into(), (800, 600), Some((800, 0)));
    f.add_named_output_at("east".into(), (800, 600), Some((1600, 0)));
    f.add_named_output_at("far-east".into(), (800, 600), Some((2400, 0)));
    let client = f.add_client();

    let mut ids = std::collections::HashMap::new();
    for output in ["west", "east", "far-east", "center"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("{output}-window"));
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        f.niri_state().update_keyboard_focus();
        ids.insert(output, f.swayward().layout.focus().unwrap().id());
    }

    assert!(crate::command::execute(f.niri_state(), "focus output east")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);
    let latest = f.client(client).create_window();
    latest.xdg_toplevel.set_app_id("east-latest".into());
    latest.commit();
    let latest_surface = latest.surface.clone();
    f.roundtrip(client);
    let latest = f.client(client).window(&latest_surface);
    latest.attach_new_buffer();
    latest.ack_last_and_commit();
    f.double_roundtrip(client);
    f.niri_state().update_keyboard_focus();
    ids.insert("east", f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "focus output center")[0].success);

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "east");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);

    assert!(crate::command::execute(f.niri_state(), "focus output far-east")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "west");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["west"]);
}

#[test]
fn directional_focus_prefers_an_adjacent_output_over_local_wrapping() {
    for (target_position, layout, command, wrapping, expected_output) in [
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (0, 600),
            "layout stacked",
            "focus down",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Force,
            "source",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::No,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Workspace,
            "source",
        ),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = wrapping;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("target".into(), (800, 600), Some(target_position));
        let client = f.add_client();

        for output in ["target", "source", "source"] {
            assert!(
                crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0]
                    .success
            );
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        assert!(crate::command::execute(f.niri_state(), layout)[0].success);

        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected_output
        );
    }
}

#[test]
fn no_wrapping_crosses_an_adjacent_output_but_does_not_wrap_outputs() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::No;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
}

#[test]
fn workspace_wrapping_uses_local_wrap_instead_of_an_adjacent_output() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let mut ids = Vec::new();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "source"
    );
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids[0]);
}

#[test]
fn workspace_wrapping_allows_output_focus_from_a_focused_workspace_node() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_workspace_focused());

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "target"
    );
}

#[test]
fn workspace_wrapping_respects_fullscreen_focus_barriers() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    for global in [false, true] {
        assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mode = if global { " global" } else { "" };
        assert!(
            crate::command::execute(f.niri_state(), &format!("fullscreen enable{mode}"))[0].success
        );

        assert!(crate::command::execute(f.niri_state(), "focus left")[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            "source"
        );
        assert!(crate::command::execute(f.niri_state(), "fullscreen disable")[0].success);
    }
}

#[test]
fn focus_output_reports_sway_errors() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output missing")[0]
            .error
            .as_deref(),
        Some("There is no output with that name.")
    );

    let mut f = Fixture::new();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output right")[0]
            .error
            .as_deref(),
        Some("No focused workspace to base directions off of.")
    );
}

#[test]
fn move_output_reports_the_missing_target() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(
        crate::command::execute(f.niri_state(), "move output missing")[0]
            .error
            .as_deref(),
        Some("Can't find output with name/direction 'missing'")
    );
}

#[test]
fn move_output_accepts_direction_name_and_workspace_forms() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let outputs = [f.niri_output(1).name(), f.niri_output(2).name()];
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move output right")[0].success);
    let focused = f.swayward().layout.focus().unwrap().id();
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == focused)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        &outputs[1]
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("move container to output {}", outputs[0])
        )[0]
        .success
    );
    assert!(crate::command::execute(f.niri_state(), "move workspace output right")[0].success);
}

#[test]
fn get_tree_hides_windows_on_background_workspaces() {
    // Sway reports `visible` per window, not per workspace: the captured
    // tests/fixtures/sway/two_workspaces.tree.json has visible:false on the
    // window sitting on the background workspace. Waybar's hasFlag recurses
    // into child nodes, so a window that always claims visibility lights up
    // every workspace button on the bar.
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));

    let client = fixture.add_client();
    for command in ["workspace 1", "workspace 2"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let mut stream = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);

    fn windows(node: &Value, workspace: Option<&str>, out: &mut Vec<(String, bool)>) {
        let workspace = if node["type"] == "workspace" {
            node["name"].as_str()
        } else {
            workspace
        };
        if node["type"] == "con" && node["nodes"].as_array().is_none_or(|n| n.is_empty()) {
            out.push((workspace.unwrap_or("?").to_owned(), node["visible"] == true));
        }
        for key in ["nodes", "floating_nodes"] {
            for child in node[key].as_array().into_iter().flatten() {
                windows(child, workspace, out);
            }
        }
    }

    let mut found = Vec::new();
    windows(&tree, None, &mut found);
    found.sort();
    assert_eq!(
        found,
        [("1".to_owned(), false), ("2".to_owned(), true)],
        "only the active workspace's window is visible"
    );
}

#[test]
fn overview_keys_work_with_num_lock_on() {
    // Num Lock is a state, not a chord. hardcoded_overview_bind used to
    // require the modifier set to be completely empty, so a keyboard with Num
    // Lock on -- which `input { keyboard { numlock } }` makes the default --
    // rejected every overview key while the mouse still worked.
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { numlock; }; }
workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 1", "workspace 2"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    assert!(fixture.swayward().keyboard_focus.is_overview());

    // Assert directly on the predicate: the harness does not latch Num Lock
    // from a keycode, and going through key_event would silently test the
    // unlocked path instead.
    let locked = smithay::input::keyboard::ModifiersState {
        num_lock: true,
        ..Default::default()
    };
    assert!(
        crate::input::hardcoded_overview_bind(smithay::input::keyboard::Keysym::Up, locked)
            .is_some(),
        "a bare Up was rejected while Num Lock was on"
    );

    let before = active_workspace_idx(&mut fixture);
    key_event(&mut fixture, 111, true);
    key_event(&mut fixture, 111, false);
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.swayward().layout.advance_animations();
    fixture.swayward().clock.set_complete_instantly(false);

    assert_ne!(
        active_workspace_idx(&mut fixture),
        before,
        "an overview arrow was rejected while Num Lock was on"
    );
}

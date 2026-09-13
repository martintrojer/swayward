//! IPC conformance tests. The empirical coverage boundary and known gaps are
//! recorded in `docs/IPC_ORACLE_COVERAGE.md`.

use std::collections::BTreeSet;
use std::ffi::OsStr;
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
    let swayward = f.swayward();
    serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap()
}

fn nested_fixture_tree() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/nested_h_in_v.tree.json"
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
    stream.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut response = Vec::new();
    loop {
        fixture.dispatch();
        let mut buf = [0; 4096];
        match stream.read(&mut buf) {
            Ok(0) => panic!("IPC connection closed before a reply"),
            Ok(len) => response.extend_from_slice(&buf[..len]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("error reading IPC reply: {error}"),
        }
        if response.len() >= crate::ipc::wire::HEADER_SIZE {
            let payload_len = u32::from_ne_bytes(response[6..10].try_into().unwrap()) as usize;
            if response.len() >= crate::ipc::wire::HEADER_SIZE + payload_len {
                let msg_type = u32::from_ne_bytes(response[10..14].try_into().unwrap());
                let payload = String::from_utf8(
                    response[crate::ipc::wire::HEADER_SIZE..][..payload_len].to_vec(),
                )
                .unwrap();
                return (msg_type, payload);
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for IPC reply");
    }
}

fn query_ipc(fixture: &mut Fixture, stream: &mut UnixStream, message_type: MessageType) -> Value {
    stream
        .write_all(&crate::ipc::wire::encode(message_type, ""))
        .unwrap();
    let (reply_type, payload) = read_ipc_reply(fixture, stream);
    assert_eq!(reply_type, message_type as u32);
    serde_json::from_str(&payload).unwrap()
}

fn ipc_fixture() -> (Fixture, std::path::PathBuf) {
    static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

    let mut fixture = Fixture::new();
    let handle = fixture.swayward().event_loop.clone();
    let socket_name = format!("test-{}", NEXT_SOCKET.fetch_add(1, Ordering::Relaxed));
    let ipc_server =
        crate::ipc::server::IpcServer::start(&handle, Some(OsStr::new(&socket_name))).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    (fixture, socket)
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
fn event_subscription_does_not_block_a_concurrent_query() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    let mut query = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&crate::ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    query
        .write_all(&crate::ipc::wire::encode(MessageType::GetVersion, ""))
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
        .write_all(&crate::ipc::wire::encode(
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
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::WindowFocusChanged { id: focused_id });
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
fn workspace_rename_event_matches_sway_shape() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(MessageType::RunCommand, "nop"))
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
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(MessageType::GetMarks, ""))
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
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(MessageType::GetMarks, ""))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([])
    );
}

#[derive(Debug)]
struct TestInput;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TestDevice;

impl crate::input::backend_ext::NiriInputDevice for TestDevice {
    fn output(&self, _state: &crate::swayward::State) -> Option<smithay::output::Output> {
        None
    }
}

impl smithay::backend::input::Device for TestDevice {
    fn id(&self) -> String {
        "test-keyboard".into()
    }

    fn name(&self) -> String {
        "test keyboard".into()
    }

    fn has_capability(&self, capability: smithay::backend::input::DeviceCapability) -> bool {
        capability == smithay::backend::input::DeviceCapability::Keyboard
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[derive(Debug)]
struct TestKeyEvent {
    key: u32,
    count: u32,
}

impl smithay::backend::input::Event<TestInput> for TestKeyEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        TestDevice
    }
}

impl smithay::backend::input::KeyboardKeyEvent<TestInput> for TestKeyEvent {
    fn key_code(&self) -> smithay::backend::input::Keycode {
        self.key.into()
    }

    fn state(&self) -> smithay::backend::input::KeyState {
        smithay::backend::input::KeyState::Pressed
    }

    fn count(&self) -> u32 {
        self.count
    }
}

impl smithay::backend::input::InputBackend for TestInput {
    type Device = TestDevice;
    type KeyboardKeyEvent = TestKeyEvent;
    type PointerAxisEvent = smithay::backend::input::UnusedEvent;
    type PointerButtonEvent = smithay::backend::input::UnusedEvent;
    type PointerMotionEvent = smithay::backend::input::UnusedEvent;
    type PointerMotionAbsoluteEvent = smithay::backend::input::UnusedEvent;
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

#[test]
fn binding_modes_switch_binds_emit_events_and_list_over_ipc() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+R { command "mode resize"; }; }
        mode "resize" {
            code:10 { command "workspace 7"; };
            Escape { command "mode default"; };
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&crate::ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode"]"#,
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

    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::Keyboard {
            event: TestKeyEvent { key: 2, count: 1 },
        },
    );
    let swayward = fixture.swayward();
    assert_eq!(
        describe_workspaces(&swayward.layout, &swayward.global_space)[0].num,
        7
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
        .write_all(&crate::ipc::wire::encode(MessageType::GetBindingModes, ""))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "resize"])
    );
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
            event: TestKeyEvent { key: 133, count: 1 },
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
            event: TestKeyEvent { key: 10, count: 2 },
        },
    );

    let swayward = fixture.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(workspaces[0].num, 7);
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
            .write_all(&crate::ipc::wire::encode(MessageType::RunCommand, command))
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

#[test]
fn for_window_applies_matching_command_when_window_maps() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&crate::ipc::wire::encode(
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
fn live_ipc_descriptions_match_sway_schema() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    let window = f.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.set_title("fixture-1");
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let swayward = f.swayward();
    let layout = &swayward.layout;
    let ours = serde_json::to_value(describe_tree(
        layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.tree.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_fixture_string_values(&fixture, &ours, "$tree");
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

    let ours = serde_json::to_value(describe_workspaces(layout, &swayward.global_space)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.workspaces.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$workspaces");

    let ours = serde_json::to_value(describe_outputs(layout, &swayward.global_space)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.outputs.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$outputs");

    let output_name = f.niri_output(1).name();
    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(workspaces.as_array().unwrap().len(), 1);
    assert_eq!(workspaces[0]["num"], 1);
    assert_eq!(workspaces[0]["name"], "1");
    assert_eq!(workspaces[0]["output"], output_name);

    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(outputs.as_array().unwrap().len(), 1);
    assert_eq!(outputs[0]["name"], output_name);

    stream
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(
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
        [(7, "7: web"), (9, "9 web")]
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
    assert_eq!(numbers, [7, 9]);
}

#[test]
fn closing_last_window_removes_inactive_named_workspace_from_ipc() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start(&handle, Some(OsStr::new("cleanup"))).unwrap();
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
        .write_all(&crate::ipc::wire::encode(
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
fn named_workspace_has_no_number_and_active_empty_workspace_remains_visible() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "workspace mail")[0].success);
    f.niri_state().ipc_refresh_layout();

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].name, "mail");
    assert_eq!(workspaces[0].num, -1);
    assert!(workspaces[0].visible);
    assert!(workspaces[0].focused);
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
        [("7: numbered", 7), ("inbox", -1), ("-42: negative", -1)]
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
    assert_eq!(workspaces[0].num, 7);
    assert_eq!(workspaces[0].focus.len(), 1);
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
        focus: vec![NodeId(1)],
        focused: false,
        children: vec![IpcNode::Leaf {
            id: NodeId(1),
            window: (),
            percent: Some(1.),
            focused: false,
            rect: Default::default(),
            deco_rect: None,
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
fn live_ipc_percent_matches_sway_parent_shares() {
    let expected = nested_fixture_tree();
    let actual = nested_live_tree();
    assert_eq!(
        expected["nodes"][1]["nodes"][0]["representation"],
        actual["nodes"][1]["nodes"][0]["representation"],
        "workspace representation at $tree.nodes[1].nodes[0]"
    );
    assert_percent_value_matches_fixture(
        &expected["nodes"][1]["nodes"][0]["nodes"][1],
        &actual["nodes"][1]["nodes"][0]["nodes"][1],
        "$tree.nodes[1].nodes[0].nodes[1]",
    );
    assert_percent_matches_fixture(&expected, &actual, "$tree");
}

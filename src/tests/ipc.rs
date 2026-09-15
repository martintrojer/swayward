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
fn non_reading_event_subscriber_is_disconnected_without_blocking_ipc() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(
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
        .write_all(&crate::ipc::wire::encode(
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TestDevice(&'static str);

impl crate::input::backend_ext::NiriInputDevice for TestDevice {
    fn output(&self, _state: &crate::swayward::State) -> Option<smithay::output::Output> {
        None
    }
}

impl smithay::backend::input::Device for TestDevice {
    fn id(&self) -> String {
        self.0.into()
    }

    fn name(&self) -> String {
        self.0.into()
    }

    fn has_capability(&self, capability: smithay::backend::input::DeviceCapability) -> bool {
        matches!(
            capability,
            smithay::backend::input::DeviceCapability::Keyboard
                | smithay::backend::input::DeviceCapability::Pointer
        )
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

fn active_workspace_name(fixture: &mut Fixture) -> Option<String> {
    fixture
        .swayward()
        .layout
        .active_workspace()
        .and_then(|workspace| workspace.name().cloned())
}

pub(super) fn pointer_button(fixture: &mut Fixture, button: u32, pressed: bool) {
    pointer_button_from(fixture, TestDevice("test keyboard"), button, pressed);
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

pub(super) fn pointer_axis(fixture: &mut Fixture, horizontal_v120: f64, vertical_v120: f64) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::PointerAxis {
            event: TestAxisEvent {
                device: TestDevice("test keyboard"),
                horizontal_v120,
                vertical_v120,
            },
        },
    );
}

pub(super) fn key_event(fixture: &mut Fixture, key: u32, pressed: bool) {
    key_event_from(fixture, TestDevice("test keyboard"), key, pressed);
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
        pointer_button_from(&mut fixture, TestDevice("first mouse"), 0x110, pressed);
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(&mut fixture, TestDevice("second mouse"), 0x111, pressed);
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(&mut fixture, TestDevice("second mouse"), 0x110, pressed);
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard-mouse")
    );
}

#[test]
fn device_identifier_matches_sways_libinput_format() {
    use crate::input::backend_ext::NiriInputDevice as _;

    let device = TestDevice("  keyboard with spaces  ");
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
        key_event_from(&mut fixture, TestDevice("first keyboard"), 53, pressed);
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact")
    );

    for pressed in [true, false] {
        key_event_from(&mut fixture, TestDevice("second keyboard"), 52, pressed);
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong")
    );

    for pressed in [true, false] {
        key_event_from(&mut fixture, TestDevice("second keyboard"), 53, pressed);
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
        .write_all(&crate::ipc::wire::encode(
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

    query
        .write_all(&crate::ipc::wire::encode(MessageType::GetBindingState, ""))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"name": "default"})
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    query
        .write_all(&crate::ipc::wire::encode(MessageType::GetBindingState, ""))
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
                device: TestDevice("test keyboard"),
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
                device: TestDevice("test keyboard"),
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
        .write_all(&crate::ipc::wire::encode(
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
fn malformed_sway_criteria_reload_keeps_the_compositor_responsive() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let malformed =
        swayward_config::Config::parse_mem(r#"binds { Mod+H { command "[con_id=nope] nop"; }; }"#)
            .map_err(|error| {
                assert!(format!("{error:?}")
                    .contains("The value for 'con_id' should be '__focused__' or numeric"));
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
        r#"for_window [class="special"] mark reloaded"#,
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
        .write_all(&crate::ipc::wire::encode(
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
    f.add_named_output_at("left".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("right".into(), (100, 100), Some((200, 0)));
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
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);

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
    assert_eq!(workspace_output(&mut f), "right");

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
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
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
    ] {
        let config = swayward_config::Config::parse_mem(config).unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1920, 1080));
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
fn workspace_back_and_forth_without_history_uses_sway_error() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&crate::ipc::wire::encode(
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
            .write_all(&crate::ipc::wire::encode(MessageType::RunCommand, command))
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
        .write_all(&crate::ipc::wire::encode(
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
        border: (swayward_ipc::command::BorderStyle::Normal, 2),
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
    assert_eq!(
        centered["x"].as_i64(),
        Some((100. + (1000. - centered["width"].as_f64().unwrap()) / 2.).round() as i64)
    );
    assert_eq!(
        centered["y"].as_i64(),
        Some((50. + (800. - centered["height"].as_f64().unwrap()) / 2.).round() as i64)
    );

    f.niri_state().move_cursor((300., 250.).into());
    assert!(crate::command::execute(f.niri_state(), "move position pointer")[0].success);
    let pointer = rect(&mut f);
    assert_eq!(
        pointer["x"].as_i64(),
        Some((300. - pointer["width"].as_f64().unwrap() / 2.).round() as i64)
    );
    assert_eq!(
        pointer["y"].as_i64(),
        Some((250. - pointer["height"].as_f64().unwrap() / 2.).round() as i64)
    );
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
        assert_eq!(window["window_rect"]["x"], 2);
        assert_eq!(window["window_rect"]["y"], 0);
        assert_eq!(
            window["window_rect"]["width"].as_i64().unwrap(),
            window["rect"]["width"].as_i64().unwrap() - 4
        );
        assert_eq!(
            window["window_rect"]["height"].as_i64().unwrap(),
            window["rect"]["height"].as_i64().unwrap() - 2
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

    let command = format!(r#"[id="{hidden}"] focus output right-head"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );

    let command = format!(r#"[id="{hidden}"] focus output left"#);
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

    let command = format!(r#"[id="{hidden}"] focus output left"#);
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

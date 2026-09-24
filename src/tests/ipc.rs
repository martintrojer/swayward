//! IPC conformance tests. The empirical coverage boundary and known gaps are
//! recorded in `docs/IPC_ORACLE_COVERAGE.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use swayward_config::OutputName;
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

const NORMALIZED_FIXTURE_VALUE: &str = "__swayward_normalized_fixture_value__";

fn normalize_fixture_value(value: &mut Value, path: &str) {
    match value {
        Value::Object(object) => {
            let is_output = object.get("type").and_then(Value::as_str) == Some("output");
            for (key, child) in object {
                let child_path = format!("{path}.{key}");
                // IDs and process metadata are allocated independently by each run.
                if matches!(key.as_str(), "id" | "pid" | "foreign_toplevel_identifier")
                    // Focus arrays contain those dynamic node IDs. Their membership
                    // and order have a separate position-mapped comparison.
                    || key == "focus"
                    // Rectangles vary with font metrics. Dedicated comparisons
                    // retain a 10 px tolerance and verify rectangle roles.
                    || matches!(
                        key.as_str(),
                        "rect" | "deco_rect" | "window_rect" | "geometry"
                    )
                    // Output identity, modes, refresh, and adaptive-sync support
                    // depend on the nested Wayland versus headless test backend.
                    || (key == "name" && is_output)
                    || key == "output"
                    || matches!(
                        key.as_str(),
                        "make" | "model" | "serial" | "adaptive_sync_status" | "modes"
                    )
                    || child_path.ends_with(".current_mode.refresh")
                    || child_path.ends_with(".features.adaptive_sync")
                {
                    *child = Value::String(NORMALIZED_FIXTURE_VALUE.into());
                } else {
                    normalize_fixture_value(child, &child_path);
                }
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter_mut().enumerate() {
                normalize_fixture_value(child, &format!("{path}[{index}]"));
            }
        }
        _ => {}
    }
}

fn normalized_fixture_values(expected: &Value, actual: &Value, path: &str) -> (Value, Value) {
    let mut expected = expected.clone();
    let mut actual = actual.clone();
    normalize_fixture_value(&mut expected, path);
    normalize_fixture_value(&mut actual, path);
    (expected, actual)
}

fn assert_same_values(expected: &Value, actual: &Value, path: &str) {
    let (expected, actual) = normalized_fixture_values(expected, actual, path);
    assert_eq!(expected, actual, "values at {path}");
}

fn checked_scalar_paths(value: &Value, path: &str, paths: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                checked_scalar_paths(child, &format!("{path}/{key}"), paths);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                checked_scalar_paths(child, &format!("{path}/{index}"), paths);
            }
        }
        Value::String(value) if value == NORMALIZED_FIXTURE_VALUE => {}
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            paths.push(path.into())
        }
    }
}

fn checked_array_paths(value: &Value, path: &str, paths: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                checked_array_paths(child, &format!("{path}/{key}"), paths);
            }
        }
        Value::Array(values) => {
            if path.ends_with("/modes") {
                return;
            }
            paths.push(path.into());
            for (index, child) in values.iter().enumerate() {
                checked_array_paths(child, &format!("{path}/{index}"), paths);
            }
        }
        _ => {}
    }
}

fn mutate_scalar(value: &mut Value) {
    match value {
        Value::Bool(value) => *value = !*value,
        Value::Number(value) => {
            *value = serde_json::Number::from_f64(value.as_f64().unwrap() + 1.).unwrap()
        }
        Value::String(value) => value.push_str("-wrong"),
        Value::Null => *value = Value::Bool(true),
        _ => panic!("not a scalar"),
    }
}

#[test]
fn normalized_fixture_comparison_rejects_every_retained_value() {
    for (path, fixture, expected_scalars, expected_arrays) in [
        (
            "$tree",
            include_str!("../../tests/fixtures/sway/one_window.tree.json"),
            111,
            18,
        ),
        (
            "$workspaces",
            include_str!("../../tests/fixtures/sway/one_window.workspaces.json"),
            17,
            4,
        ),
        (
            "$outputs",
            include_str!("../../tests/fixtures/sway/one_window.outputs.json"),
            29,
            4,
        ),
    ] {
        let original: Value = serde_json::from_str(fixture).unwrap();
        let (normalized, _) = normalized_fixture_values(&original, &original, path);
        let mut paths = Vec::new();
        checked_scalar_paths(&normalized, "", &mut paths);
        let checked = paths.len();
        for pointer in paths {
            let mut mutated = original.clone();
            mutate_scalar(mutated.pointer_mut(&pointer).unwrap());
            let (expected, actual) = normalized_fixture_values(&original, &mutated, path);
            assert_ne!(
                expected, actual,
                "mutation survived normalization at {pointer}"
            );
        }
        assert_eq!(checked, expected_scalars, "checked scalar count at {path}");

        let mut array_paths = Vec::new();
        checked_array_paths(&normalized, "", &mut array_paths);
        assert_eq!(
            array_paths.len(),
            expected_arrays,
            "checked array count at {path}"
        );
        for pointer in array_paths {
            let mut mutated = original.clone();
            let array = mutated
                .pointer_mut(&pointer)
                .unwrap()
                .as_array_mut()
                .unwrap();
            if array.is_empty() {
                array.push(Value::Null);
            } else {
                array.pop();
            }
            let (expected, actual) = normalized_fixture_values(&original, &mutated, path);
            assert_ne!(
                expected, actual,
                "array length mutation survived normalization at {pointer}"
            );
        }
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
            // Mode enumeration is backend-dependent: sway's nested Wayland
            // output advertises none, while the headless Smithay output
            // advertises its synthetic current mode.
            if !path.ends_with(".modes") {
                assert_eq!(expected.len(), actual.len(), "array length at {path}");
                for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
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
            let mut expected_keys = expected.keys().collect::<BTreeSet<_>>();
            // The individual event fixtures predate sway 1.12. Its `tag`
            // addition is pinned by the recaptured window-map sequences and
            // checked against sway's serializer by check-sway-fixture-schema.
            let tag = String::from("tag");
            if expected.contains_key("app_id") {
                expected_keys.insert(&tag);
                assert!(actual["tag"].is_null(), "tag at {path}");
            }
            assert_eq!(
                expected_keys,
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

fn assert_tree_rectangles_match_fixture(expected: &Value, actual: &Value, path: &str) {
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
    for child_key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[child_key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[child_key].as_array().unwrap())
            .enumerate()
        {
            assert_tree_rectangles_match_fixture(
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

fn find_json_parent_of_app_id<'a>(value: &'a Value, app_id: &str) -> Option<&'a Value> {
    for key in ["nodes", "floating_nodes"] {
        let children = value[key].as_array()?;
        if children.iter().any(|child| child["app_id"] == app_id) {
            return Some(value);
        }
        if let Some(parent) = children
            .iter()
            .find_map(|child| find_json_parent_of_app_id(child, app_id))
        {
            return Some(parent);
        }
    }
    None
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
fn exec_no_startup_id_suppresses_only_the_desktop_token() {
    let (mut fixture, _socket) = ipc_fixture();
    let directory = std::env::temp_dir();
    let suffix = std::process::id();
    let plain = directory.join(format!("swayward-exec-env-plain-{suffix}"));
    let suppressed = directory.join(format!("swayward-exec-env-suppressed-{suffix}"));
    let plain_pending = plain.with_extension("pending");
    let suppressed_pending = suppressed.with_extension("pending");
    for path in [&plain, &suppressed, &plain_pending, &suppressed_pending] {
        let _ = std::fs::remove_file(path);
    }

    // Redirect creates its target before `env` writes anything. Write to a
    // private path and rename it last, so the observed path means that the
    // child is done rather than merely started.
    for (command, output) in [
        (
            format!(
                "exec env > {} && mv {} {}",
                plain_pending.display(),
                plain_pending.display(),
                plain.display()
            ),
            &plain,
        ),
        (
            format!(
                "exec --no-startup-id env > {} && mv {} {}",
                suppressed_pending.display(),
                suppressed_pending.display(),
                suppressed.display()
            ),
            &suppressed,
        ),
    ] {
        assert!(crate::command::execute(fixture.niri_state(), &command)[0].success);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !output.exists() {
            assert!(
                Instant::now() < deadline,
                "{command} did not write its environment"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let read = |path: &std::path::Path| {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter_map(|line| {
                line.split_once('=')
                    .map(|(name, value)| (name.to_owned(), value.to_owned()))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let plain_env = read(&plain);
    let suppressed_env = read(&suppressed);
    std::fs::remove_file(plain).unwrap();
    std::fs::remove_file(suppressed).unwrap();

    let plain_xdg = plain_env.get("XDG_ACTIVATION_TOKEN").unwrap();
    assert!(!plain_xdg.is_empty());
    assert_eq!(plain_env.get("DESKTOP_STARTUP_ID"), Some(plain_xdg));
    assert!(!suppressed_env["XDG_ACTIVATION_TOKEN"].is_empty());
    assert!(!suppressed_env.contains_key("DESKTOP_STARTUP_ID"));
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

/// Sway writes this reply as a C string literal rather than serialising it
/// (`sway/sway/ipc-server.c:870`), so it carries spaces a JSON encoder would
/// not produce. The comparison above parses both sides and so cannot see
/// that; SWAY_COMPATIBILITY.md nonetheless called the reply byte-identical,
/// while swayward was sending the compact form.
#[test]
fn get_bar_config_unknown_id_is_byte_identical_to_sway() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBarConfig,
            "bar-0",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        reply,
        r#"{ "success": false, "error": "No bar with that ID" }"#
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
fn malformed_frames_do_not_hang_or_wedge_the_server() {
    // "It never hangs" is the half of the wire invariant most likely to fail,
    // and these are the frames a buggy client actually sends. Each case uses a
    // fresh connection: the server is entitled to drop a client that sends a
    // malformed frame, but it must not stop serving anyone else.
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();

    let cases: [(&str, Vec<u8>); 4] = [
        ("truncated header", b"i3-ipc\x00\x00".to_vec()),
        ("bad magic", {
            let mut f = swayward_ipc::wire::encode_raw(7, "");
            f[0] = b'X';
            f
        }),
        ("length longer than payload", {
            let mut f = swayward_ipc::wire::encode_raw(0, "");
            f[6..10].copy_from_slice(&64u32.to_ne_bytes());
            f
        }),
        ("payload that is not utf-8", {
            let mut f = swayward_ipc::wire::encode_raw(0, "");
            f[6..10].copy_from_slice(&2u32.to_ne_bytes());
            f.extend_from_slice(&[0xff, 0xfe]);
            f
        }),
    ];

    for (name, frame) in cases {
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream.write_all(&frame).unwrap();
        // Drive the loop. The server may reply or drop this client; either is
        // a valid answer to a frame it cannot parse. What it must not do is
        // block, which would show up as this never returning.
        fixture.dispatch();

        let mut healthy = UnixStream::connect(&socket).unwrap();
        let version = query_ipc(&mut fixture, &mut healthy, MessageType::GetVersion);
        assert_eq!(
            version["variant"], "swayward",
            "server stopped serving after a {name}"
        );
    }
}

#[test]
fn captured_workspace_event_sequences_pin_order_and_multiplicity() {
    for (fixture, expected) in [
        (
            include_str!("../../tests/fixtures/sway/events/workspace-switch-empty.sequence.json"),
            &["init", "focus", "focus", "focus", "empty"][..],
        ),
        (
            include_str!("../../tests/fixtures/sway/events/workspace-close-last.sequence.json"),
            &["close", "empty"][..],
        ),
        (
            include_str!("../../tests/fixtures/sway/events/workspace-rename.sequence.json"),
            &["rename"][..],
        ),
        (
            include_str!(
                "../../tests/fixtures/sway/events/workspace-move-right-empty-destination.sequence.json"
            ),
            &["move"][..],
        ),
        (
            include_str!(
                "../../tests/fixtures/sway/events/workspace-move-right-occupied-destination.sequence.json"
            ),
            &["move"][..],
        ),
        (
            include_str!(
                "../../tests/fixtures/sway/events/workspace-move-right-last-source.sequence.json"
            ),
            &["move"][..],
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
fn get_config_reports_not_implemented_rather_than_returning_kdl() {
    // Sway's GET_CONFIG returns the verbatim text of the sway config file
    // (`sway/sway/config.c:734-773` reads it byte for byte into
    // `config->current_config`; `sway/sway/ipc-server.c:908-917` returns it
    // unaltered). swayward's config is KDL, so there is nothing sway-shaped
    // to return.
    //
    // Serving KDL inside sway's single-field envelope was worse than serving
    // nothing: the reply is well-formed, so a client parses it as sway syntax
    // and fails with no error to attribute it to. A wire deviation is either
    // fully compliant or not implemented.
    //
    // `{"success": false}` is sway's own answer for a request it declines to
    // serve (`sway/sway/ipc-server.c:919-925`, IPC_SYNC).
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
    assert_eq!(reply, serde_json::json!({"success": false}));
    assert!(
        reply.get("config").is_none(),
        "must not leak KDL through sway's config field: {reply}"
    );
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
fn input_xkb_switch_layout_changes_get_inputs_and_emits_layout_events() {
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
    let ((_, payload), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(payload, r#"{"success": true}"#);

    let mut command = UnixStream::connect(&socket).unwrap();
    let mut query = UnixStream::connect(&socket).unwrap();
    for (input, expected_index, expected_name) in [
        ("input type:keyboard xkb_switch_layout next", 1, "Russian"),
        (
            "input 0:0:test_keyboard xkb_switch_layout prev",
            0,
            "English (US)",
        ),
        ("input type:keyboard xkb_switch_layout 1", 1, "Russian"),
    ] {
        let result =
            query_ipc_with_payload(&mut fixture, &mut command, MessageType::RunCommand, input);
        assert_eq!(result, serde_json::json!([{"success": true}]));

        let inputs = query_ipc(&mut fixture, &mut query, MessageType::GetInputs);
        assert_eq!(inputs[0]["xkb_active_layout_index"], expected_index);
        assert_eq!(inputs[0]["xkb_active_layout_name"], expected_name);

        let ((event_type, payload), next_remainder) =
            read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
        remainder = next_remainder;
        assert_eq!(event_type, (1 << 31) | 21);
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        assert_eq!(event["change"], "xkb_layout");
        assert_eq!(event["input"], inputs[0]);
    }
    assert!(remainder.is_empty());

    let refused = query_ipc_with_payload(
        &mut fixture,
        &mut command,
        MessageType::RunCommand,
        "input type:keyboard repeat_delay 300",
    );
    assert_eq!(refused[0]["success"], false);
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

    let outcome = crate::command::execute(fixture.niri_state(), "exit");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(fixture.swayward().shutdown_requested);
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
    // Sway sends this as a 17-byte C string literal, space included
    // (`sway/sway/ipc-server.c`, IPC_SEND_TICK).
    assert_eq!(payload, r#"{"success": true}"#);
    assert_eq!(payload.len(), 17, "sway writes exactly 17 bytes here");
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
fn subscribing_to_tick_on_an_existing_subscription_emits_first_tick() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["tick"]"#,
        ))
        .unwrap();
    let ((reply_type, reply), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(reply_type, MessageType::Subscribe as u32);
    assert_eq!(reply, r#"{"success": true}"#);
    let ((event_type, payload), _) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, (1 << 31) | 7);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"first": true, "payload": ""})
    );
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
fn partial_event_stream_header_survives_an_interleaved_event() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let request = swayward_ipc::wire::encode(MessageType::GetVersion, "");
    subscriber.write_all(&request[..7]).unwrap();
    fixture.dispatch();
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::WorkspaceReloaded);
    let (event_type, _) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);

    subscriber.write_all(&request[7..]).unwrap();
    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply_type, MessageType::GetVersion as u32);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap()["variant"],
        "swayward"
    );
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
fn get_tree_between_unmap_and_refresh_does_not_hide_window_close() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "close-after-query");
    fixture.niri_state().ipc_refresh_layout();
    let mapped = fixture
        .swayward()
        .layout
        .windows()
        .next()
        .unwrap()
        .1
        .window
        .clone();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    fixture
        .swayward()
        .layout
        .remove_window(&mapped, crate::utils::transaction::Transaction::new());
    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    assert!(find_json_node_with_app_id(&tree, "close-after-query").is_none());
    fixture.niri_state().ipc_refresh_layout();

    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let event: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(event["change"], "close");
    assert_eq!(event["container"]["app_id"], "close-after-query");
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

    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::WorkspaceReloaded);
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

    let mut subscriber = UnixStream::connect(&socket).unwrap();
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

    // Waybar responds to this event by querying GET_TREE. Before workspace
    // urgency was keyed by the layout id, applying the event looked up its
    // sway tree id instead and never updated the baseline. Every subsequent
    // compositor refresh emitted another urgency event.
    let mut query = UnixStream::connect(&socket).unwrap();
    for _ in 0..32 {
        let _ = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
        fixture.niri_state().ipc_refresh_layout();
    }
    subscriber.set_nonblocking(true).unwrap();
    fixture.dispatch();
    let mut byte = [0];
    assert!(matches!(
        subscriber.read(&mut byte),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
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
fn output_runtime_commands_apply_named_state_and_wildcard_fanout() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    fixture.add_named_output_at("right".into(), (1024, 768), Some((800, 0)));
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["output"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let command = "output left scale 1.5 transform 90 position 200 300 mode 1280x720@60Hz";
    assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
    fixture.niri_state().refresh_ipc_outputs();
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 1);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change": "unspecified"})
    );

    let left = fixture.niri_output(1);
    assert_eq!(left.current_scale().fractional_scale(), 1.5);
    assert_eq!(left.current_transform(), smithay::utils::Transform::_270);
    assert_eq!(
        fixture
            .swayward()
            .global_space
            .output_geometry(&left)
            .unwrap()
            .loc,
        (200, 300).into()
    );
    {
        let config = fixture.swayward().config.borrow();
        let left = config
            .outputs
            .0
            .iter()
            .find(|output| output.name == "left")
            .unwrap();
        let mode = left.mode.unwrap();
        assert_eq!((mode.mode.width, mode.mode.height), (1280, 720));
        assert_eq!(mode.mode.refresh, Some(60.));
    }

    assert!(crate::command::execute(fixture.niri_state(), "output * scale 2")[0].success);
    assert_eq!(
        fixture.niri_output(1).current_scale().fractional_scale(),
        2.
    );
    assert_eq!(
        fixture.niri_output(2).current_scale().fractional_scale(),
        2.
    );
    {
        let config = fixture.swayward().config.borrow();
        assert!(!config.outputs.0.iter().any(|output| output.name == "*"));
        assert_eq!(
            config
                .outputs
                .0
                .iter()
                .filter(|output| output.scale.map(|scale| scale.0) == Some(2.))
                .count(),
            2
        );
    }

    for command in [
        "output left disable scale 1.75",
        "output left enable",
        "output left mode --custom 640x480@75Hz",
        "output left modeline 25.175 640 656 752 800 480 490 492 525 -hsync -vsync",
        "output left adaptive_sync on",
        "output left render_bit_depth 10",
    ] {
        assert!(
            crate::command::execute(fixture.niri_state(), command)[0].success,
            "{command}"
        );
    }
    let config = fixture.swayward().config.borrow();
    let left = config
        .outputs
        .0
        .iter()
        .find(|output| output.name == "left")
        .unwrap();
    assert!(!left.off);
    assert_eq!(left.scale.unwrap().0, 1.75);
    assert!(left.mode.unwrap().custom);
    assert_eq!(left.mode.unwrap().mode.refresh, Some(75.));
    assert_eq!(left.modeline.unwrap().clock, 25.175);
    assert_eq!(
        left.variable_refresh_rate,
        Some(swayward_config::Vrr { on_demand: false })
    );
    assert_eq!(left.max_bpc.unwrap().0, swayward_ipc::MaxBpc::_10);
}

#[test]
fn output_power_commands_update_get_outputs_state() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    fixture.add_output(2, (1024, 768));
    let mut stream = UnixStream::connect(&socket).unwrap();
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["output"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let power_states = |fixture: &mut Fixture, stream: &mut UnixStream| {
        query_ipc(fixture, stream, MessageType::GetOutputs)
            .as_array()
            .unwrap()
            .iter()
            .map(|output| {
                (
                    output["name"].as_str().unwrap().to_owned(),
                    output["power"].as_bool().unwrap(),
                    output["dpms"].as_bool().unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };

    assert!(
        crate::command::execute(fixture.niri_state(), "output headless-1 power off")[0].success
    );
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 1);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change": "unspecified"})
    );
    assert_eq!(
        power_states(&mut fixture, &mut stream),
        [
            ("headless-1".into(), false, false),
            ("headless-2".into(), true, true)
        ]
    );

    assert!(
        crate::command::execute(fixture.niri_state(), "output headless-1 dpms toggle")[0].success
    );
    assert_eq!(
        power_states(&mut fixture, &mut stream),
        [
            ("headless-1".into(), true, true),
            ("headless-2".into(), true, true)
        ]
    );

    assert!(crate::command::execute(fixture.niri_state(), "output * dpms off")[0].success);
    assert_eq!(
        power_states(&mut fixture, &mut stream),
        [
            ("headless-1".into(), false, false),
            ("headless-2".into(), false, false)
        ]
    );
    assert!(crate::command::execute(fixture.niri_state(), "output * dpms on")[0].success);
    assert_eq!(
        power_states(&mut fixture, &mut stream),
        [
            ("headless-1".into(), true, true),
            ("headless-2".into(), true, true)
        ]
    );

    assert!(crate::command::execute(fixture.niri_state(), "output * power off")[0].success);
    assert!(!crate::command::execute(fixture.niri_state(), "output * power toggle")[0].success);

    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert_eq!(
        power_states(&mut fixture, &mut stream),
        [
            ("headless-1".into(), true, true),
            ("headless-2".into(), true, true)
        ]
    );
}

fn drain_workspace_window_events(
    fixture: &mut Fixture,
    subscriber: &mut UnixStream,
    mut remainder: Vec<u8>,
) -> (Vec<Value>, Vec<u8>) {
    fixture
        .swayward()
        .ipc_server
        .as_ref()
        .unwrap()
        .send_event(swayward_ipc::legacy::Event::Tick {
            payload: "barrier".into(),
            first: false,
        });
    let mut events = Vec::new();
    loop {
        let ((event_type, payload), next) =
            read_ipc_reply_with_remainder(fixture, subscriber, remainder);
        remainder = next;
        if event_type == (1 << 31) | 7 {
            break;
        }
        assert!(event_type == 1 << 31 || event_type == (1 << 31) | 3);
        events.push(serde_json::from_str(&payload).unwrap());
    }
    (events, remainder)
}

/// Sway's session-lock implementation changes seat focus but does not call
/// `ipc_event_workspace` or `ipc_event_window` (`sway/desktop/session_lock.c`).
/// Output power and idle wake likewise have no workspace/window event. A real
/// connector replug moves the affected workspace, and focus commands retain
/// their ordinary one-event-per-transition behavior while locked.
#[test]
fn lock_power_idle_and_hotplug_have_bounded_workspace_window_events() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    fixture.add_output(2, (1024, 768));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "event-burst-probe");
    let window_surface_id = fixture
        .client(client)
        .state
        .windows
        .last()
        .unwrap()
        .surface
        .id()
        .protocol_id();
    fixture.niri_state().refresh_and_flush_clients();

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace","window","tick"]"#,
        ))
        .unwrap();
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);

    let lock = {
        let client = fixture.client(client);
        client
            .state
            .session_lock_manager
            .as_ref()
            .unwrap()
            .lock(&client.qh, ())
    };
    let outputs = fixture
        .client(client)
        .state
        .outputs
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for output in outputs {
        fixture
            .client(client)
            .state
            .create_lock_surface(&lock, &output);
    }
    fixture.roundtrip(client);
    {
        let client = fixture.client(client);
        let qh = client.qh.clone();
        let spbm = client.state.spbm.clone().unwrap();
        for surface in &client.state.lock_surfaces {
            surface.ack_and_map(&qh, &spbm);
        }
    }
    fixture.roundtrip(client);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fixture.client(client).state.session_locked && Instant::now() < deadline {
        fixture
            .state
            .server
            .event_loop
            .dispatch(Duration::from_millis(10), &mut fixture.state.server.state)
            .unwrap();
        fixture.state.server.state.refresh_and_flush_clients();
        fixture.dispatch();
    }
    assert!(fixture.client(client).state.session_locked);
    let (events, next) = drain_workspace_window_events(&mut fixture, &mut subscriber, remainder);
    assert!(events.is_empty(), "locking emitted {events:?}");
    remainder = next;

    lock.unlock_and_destroy();
    fixture.roundtrip(client);
    fixture.niri_state().refresh_and_flush_clients();
    let keyboard_focus_id = fixture
        .swayward()
        .seat
        .get_keyboard()
        .unwrap()
        .current_focus()
        .map(|surface| surface.id().protocol_id());
    assert_eq!(keyboard_focus_id, Some(window_surface_id));
    let (events, next) = drain_workspace_window_events(&mut fixture, &mut subscriber, remainder);
    assert!(events.is_empty(), "unlocking emitted {events:?}");
    remainder = next;

    for command in ["output * power off", "output * power on"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        fixture.niri_state().refresh_and_flush_clients();
        let (events, next) =
            drain_workspace_window_events(&mut fixture, &mut subscriber, remainder);
        assert!(events.is_empty(), "{command} emitted {events:?}");
        remainder = next;
    }

    {
        let state = fixture.niri_state();
        state.swayward.deactivate_monitors(&mut state.backend);
        state.swayward.activate_monitors(&mut state.backend);
    }
    fixture.niri_state().refresh_and_flush_clients();
    let (events, next) = drain_workspace_window_events(&mut fixture, &mut subscriber, remainder);
    assert!(events.is_empty(), "idle wake emitted {events:?}");
    remainder = next;

    let removed = fixture.niri_output(1);
    fixture.swayward().remove_output(&removed);
    fixture.niri_state().refresh_and_flush_clients();
    fixture.add_output(1, (800, 600));
    fixture.niri_state().refresh_and_flush_clients();
    let (events, next) = drain_workspace_window_events(&mut fixture, &mut subscriber, remainder);
    assert_eq!(
        events
            .iter()
            .map(|event| event["change"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["move", "focus", "move"]
    );
    remainder = next;

    for command in ["workspace 2", "workspace 1"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
    }
    fixture.niri_state().refresh_and_flush_clients();
    let (events, _) = drain_workspace_window_events(&mut fixture, &mut subscriber, remainder);
    assert_eq!(
        events
            .iter()
            .map(|event| event["change"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["focus", "focus"]
    );
}

#[test]
fn a_powered_off_output_does_not_block_session_lock_confirmation() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    assert!(
        crate::command::execute(fixture.niri_state(), "output headless-1 power off")[0].success
    );

    let client = fixture.add_client();
    let lock = {
        let client = fixture.client(client);
        client
            .state
            .session_lock_manager
            .as_ref()
            .unwrap()
            .lock(&client.qh, ())
    };
    fixture.roundtrip(client);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fixture.client(client).state.session_locked && Instant::now() < deadline {
        fixture
            .state
            .server
            .event_loop
            .dispatch(Duration::from_millis(10), &mut fixture.state.server.state)
            .unwrap();
        fixture.state.server.state.refresh_and_flush_clients();
        fixture.dispatch();
    }

    assert!(fixture.client(client).state.session_locked);
    lock.unlock_and_destroy();
}

#[test]
fn an_ipc_event_burst_does_not_delay_session_lock_confirmation() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["tick"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    for sequence in 0..1_000 {
        fixture
            .niri_state()
            .swayward
            .ipc_server
            .as_ref()
            .unwrap()
            .send_event(swayward_ipc::legacy::Event::Tick {
                first: false,
                payload: sequence.to_string(),
            });
    }

    let client = fixture.add_client();
    let lock = {
        let client = fixture.client(client);
        client
            .state
            .session_lock_manager
            .as_ref()
            .unwrap()
            .lock(&client.qh, ())
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while !fixture.client(client).state.session_locked && Instant::now() < deadline {
        fixture
            .state
            .server
            .event_loop
            .dispatch(Duration::from_millis(10), &mut fixture.state.server.state)
            .unwrap();
        fixture.state.server.state.refresh_and_flush_clients();
        fixture.dispatch();
    }

    assert!(fixture.client(client).state.session_locked);
    lock.unlock_and_destroy();
}

#[test]
fn output_power_survives_replug_and_idle_wake() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let mut stream = UnixStream::connect(socket).unwrap();

    assert!(
        crate::command::execute(fixture.niri_state(), "output headless-1 power off")[0].success
    );
    let removed = fixture.niri_output(1);
    fixture.swayward().remove_output(&removed);
    fixture.add_output(1, (800, 600));

    {
        let state = fixture.niri_state();
        state.swayward.deactivate_monitors(&mut state.backend);
        state.swayward.activate_monitors(&mut state.backend);
    }

    let outputs = query_ipc(&mut fixture, &mut stream, MessageType::GetOutputs);
    assert_eq!(outputs[0]["name"], "headless-1");
    assert_eq!(outputs[0]["power"], false);
    assert_eq!(outputs[0]["dpms"], false);
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
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let window = fixture.client(client).window(&surface);
    window.attach_null();
    window.commit();
    fixture.double_roundtrip(client);
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

/// Sway's GET_MARKS walks the container tree and appends each container's
/// marks in the order it meets them (`sway/tree/root.c:246-260`,
/// `sway/ipc-server.c:604-610,825-834`). It never sorts, and it visits every
/// container, not only the ones holding a view.
///
/// Swayward sorted the list and read only the per-window map, so a mark set
/// on a split container was reported by GET_TREE and missing from GET_MARKS.
#[test]
fn get_marks_reports_container_marks_in_tree_order_like_sway() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let id = fixture.add_client();

    // Two windows, so there is a split container to mark.
    for index in 0..2 {
        let window = fixture.client(id).create_window();
        window.xdg_toplevel.set_app_id(format!("fixture-{index}"));
        window.set_title(&format!("fixture-{index}"));
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(id);
        let window = fixture.client(id).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(id);
    }

    let mut stream = UnixStream::connect(&socket).unwrap();
    let run = |fixture: &mut Fixture, stream: &mut UnixStream, command: &str| {
        stream
            .write_all(&swayward_ipc::wire::encode(
                MessageType::RunCommand,
                command,
            ))
            .unwrap();
        let (_, reply) = read_ipc_reply(fixture, stream);
        reply
    };

    // "zeta" is marked first but sorts last, so a sorted reply reorders it.
    // Split the second window vertically and add a third, so the tree holds a
    // real split container below the workspace. `focus parent` from a leaf of
    // that split reaches the container, not the workspace: sway rejects `mark`
    // on a workspace with "Only containers can have marks"
    // (`sway/commands/mark.c:20-23`).
    run(&mut fixture, &mut stream, "splitv");
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-2".into());
    window.set_title("fixture-2");
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    // "zeta" is marked first but sorts last, so a sorted reply reorders it.
    run(&mut fixture, &mut stream, "mark zeta");
    run(&mut fixture, &mut stream, "focus parent");
    run(&mut fixture, &mut stream, "mark alpha");

    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::GetMarks, ""))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    let marks: Vec<String> = serde_json::from_str(&reply).unwrap();

    assert!(
        marks.contains(&"alpha".to_string()),
        "a mark on a split container must appear in GET_MARKS, as sway walks \
         every container and not only views: {marks:?}"
    );
    assert_eq!(
        marks,
        vec!["alpha".to_string(), "zeta".to_string()],
        "GET_MARKS must follow sway's tree walk, which reaches the parent \
         before its children, rather than sorting: {marks:?}"
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

#[derive(Debug)]
struct TestSwitchEvent {
    device: TestDevice,
    switch: smithay::backend::input::Switch,
    state: smithay::backend::input::SwitchState,
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

impl smithay::backend::input::Event<TestInput> for TestSwitchEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::SwitchToggleEvent<TestInput> for TestSwitchEvent {
    fn switch(&self) -> Option<smithay::backend::input::Switch> {
        Some(self.switch)
    }

    fn state(&self) -> smithay::backend::input::SwitchState {
        self.state
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
    type SwitchToggleEvent = TestSwitchEvent;
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

/// Press and release `Mod4+a` as a real key sequence.
fn press_mod_a(fixture: &mut Fixture) {
    key_event(fixture, 133, true);
    key_event(fixture, 38, true);
    key_event(fixture, 38, false);
    key_event(fixture, 133, false);
}

fn switch_event(
    fixture: &mut Fixture,
    switch: smithay::backend::input::Switch,
    state: smithay::backend::input::SwitchState,
) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::SwitchToggle {
            event: TestSwitchEvent {
                device: TestDevice::keyboard("test switch"),
                switch,
                state,
            },
        },
    );
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

/// Runtime key binding commands must mutate the exact table the keyboard path
/// reads: add must fire, unbind must stop firing, and reload must restore the
/// file-backed table rather than retaining runtime mutations.
#[test]
fn runtime_bindsym_fires_unbinds_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let added = crate::command::execute(
        fixture.niri_state(),
        "bindsym Mod4+a workspace runtime-bound",
    );
    assert!(added[0].success, "{added:?}");
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("runtime-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace unbound-check")[0].success);
    let removed = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+a");
    assert!(removed[0].success, "{removed:?}");
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("unbound-check")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace should-not-survive",
        )[0]
        .success
    );
    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-check")[0].success);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("reload-check")
    );
}

#[test]
fn runtime_bindcode_fires_and_unbindcode_stops_it() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(fixture.niri_state(), "bindcode 39 workspace code-bound",)[0]
            .success
    );
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("code-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "unbindcode 39")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace code-unbound")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("code-unbound")
    );
}

#[test]
fn runtime_bindsym_release_fires_only_on_key_release() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym --release a workspace released",
        )[0]
        .success
    );

    key_event(&mut fixture, 38, true);
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("released")
    );
    key_event(&mut fixture, 38, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("released")
    );
}

/// Sway replaces an equal binding rather than appending a competing one
/// (`binding_upsert`, sway/sway/commands/bind.c:260-278).
#[test]
fn runtime_bindsym_duplicate_overwrites_the_old_command() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace first-command",
        )[0]
        .success
    );
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace replacement-command",
        )[0]
        .success
    );
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);

    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("replacement-command")
    );
    assert_eq!(fixture.swayward().config.borrow().binds.0.len(), 1);
}

/// `unbind*` reports failure when no binding has the same key and flags
/// (`sway/sway/commands/bind.c:302-320`) and leaves the table alone.
#[test]
fn runtime_unbindsym_missing_binding_fails_without_mutation() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let before = fixture.swayward().config.borrow().binds.0.clone();

    let outcome = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+a");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Could not find binding `Mod4+a` for the given flags")
    );
    assert_eq!(fixture.swayward().config.borrow().binds.0, before);
}

/// Top-level runtime binds target sway's current mode, not always the default
/// (`sway/sway/commands/bind.c:291-298`).
#[test]
fn runtime_bindsym_mutates_the_active_binding_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds { Mod4+a { command "workspace default-mode"; }; }
mode "resize" {
    Mod4+b { command "nop"; };
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+a workspace resize-mode",)[0]
            .success
    );

    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("resize-mode")
    );

    // The default table was not overwritten.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-mode")
    );
}

/// Runtime variable substitution happens before cmd_bindsym stores its command,
/// so redefining the variable later does not rewrite the captured binding.
#[test]
fn runtime_bindsym_captures_the_current_variable_value() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "set $dest captured")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+a workspace $dest",)[0].success
    );
    assert!(crate::command::execute(fixture.niri_state(), "set $dest later")[0].success);

    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("captured")
    );
}

/// A binding may mutate the binding table while it is itself being dispatched.
/// Sway does this safely because the table is a plain list; swayward holds it
/// behind a `RefCell`, so a live borrow across dispatch would panic rather than
/// misbehave. Drive the reentrant case through real key input to prove the
/// borrow is released before the command runs.
#[test]
fn a_binding_may_rebind_and_unbind_itself_while_dispatching() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Mod4+a { command "bindsym Mod4+b workspace chained"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    // Mod4+a adds Mod4+b from inside its own dispatch.
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 56, true);
    key_event(&mut fixture, 56, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("chained")
    );

    // A binding that removes itself takes effect from the next press onwards.
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+c unbindsym Mod4+c",)[0]
            .success
    );
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 54, true);
    key_event(&mut fixture, 54, false);
    key_event(&mut fixture, 133, false);
    let outcome = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+c");
    assert!(
        !outcome[0].success,
        "the self-unbinding bind should already be gone: {outcome:?}"
    );
}

#[test]
fn runtime_bindswitch_fires_unbinds_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch lid:on workspace switch-bound",
        )[0]
        .success
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "unbindswitch lid:on")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace switch-unbound")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-unbound")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch lid:on workspace should-not-survive",
        )[0]
        .success
    );
    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(crate::command::execute(fixture.niri_state(), "workspace switch-reload")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-reload")
    );
}

#[test]
fn runtime_bindswitch_respects_mode_and_toggle_trigger() {
    let config =
        swayward_config::Config::parse_mem(r#"mode "switch-mode" { x { command "nop"; }; }"#)
            .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode switch-mode")[0].success);
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch tablet:toggle workspace toggled",
        )[0]
        .success
    );

    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("toggled")
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace before-off")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::Off,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("toggled")
    );

    // The mode-local binding is inactive in default mode.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "workspace default-switch-mode")[0].success
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-switch-mode")
    );
}

#[test]
fn runtime_bindswitch_refuses_to_shadow_a_narrower_kdl_switch_event() {
    let config = swayward_config::Config::parse_mem(
        r#"
switch-events {
    lid-close { spawn "true"; }
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "bindswitch lid:on workspace would-shadow",
    );
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("runtime switch binding conflicts with a narrower KDL switch-event binding")
    );
    assert!(fixture.swayward().runtime_switch_bindings.is_empty());
}

/// Gesture binds remain honest failures: gesture events have no sway command
/// binding table or matching path.
#[test]
fn unsupported_runtime_gesture_binds_do_not_mutate_key_table() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let before = fixture.swayward().config.borrow().binds.0.clone();

    for command in ["bindgesture swipe:3:left nop", "unbindgesture swipe:3:left"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(!outcome[0].success, "{command}: {outcome:?}");
    }
    assert_eq!(fixture.swayward().config.borrow().binds.0, before);
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

/// Waybar tracks the focused workspace from the `workspace` event stream, not
/// by polling GET_WORKSPACES. Jumping to a workspace from the overview changes
/// the active workspace through `toggle_overview_to_workspace`, which is not a
/// command dispatch, so nothing on that path told the event stream anything
/// had happened and every bar kept highlighting the workspace the user left.
#[test]
fn overview_workspace_jump_emits_a_workspace_focus_event() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));

    for command in ["workspace 2", "workspace 1"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }

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

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let before = active_idx(&mut fixture);

    // Exactly what a click on another workspace in the overview does.
    assert!(fixture.swayward().layout.open_overview());
    fixture
        .swayward()
        .layout
        .toggle_overview_to_workspace(before + 1);
    fixture.niri_state().refresh_and_flush_clients();

    let after = active_idx(&mut fixture);
    assert_eq!(
        after,
        before + 1,
        "the overview jump must change the active workspace"
    );

    let ((event_type, payload), _) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, 1 << 31, "expected a workspace event");
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(
        event["change"], "focus",
        "a bar learns the workspace changed only from this event: {event}"
    );
}

/// Waybar answers a workspace event by immediately re-reading GET_TREE and
/// rendering whatever that reply says
/// (Waybar/src/modules/sway/workspaces.cpp:107-113,146-172). So the tree the
/// server is holding at the moment it emits the event is the tree the bar
/// draws.
///
/// ipc_refresh_layout emits from ipc_refresh_workspaces first and only then
/// calls refresh_query_state, so for that window query_state.tree still
/// describes the workspace the user left.
#[test]
fn query_state_tree_is_current_when_a_workspace_event_is_emitted() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));
    for command in ["workspace 2", "workspace 1"] {
        crate::command::execute(fixture.niri_state(), command);
    }
    fixture.niri_state().refresh_and_flush_clients();

    // Subscribe first: this client is the bar, and it must not see a stale
    // tree after being told the workspace changed.
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let before = fixture
        .swayward()
        .layout
        .monitor_for_output(&output)
        .unwrap()
        .active_workspace_idx();

    assert!(fixture.swayward().layout.open_overview());
    fixture
        .swayward()
        .layout
        .toggle_overview_to_workspace(before + 1);

    fixture.niri_state().ipc_refresh_layout();

    // Wait for the event, then query exactly as waybar does on receiving it.
    let ((_, payload), _) = read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "focus", "expected a focus event: {event}");

    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut workspaces);
    let focused = workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "the cached GET_TREE served to a bar that just saw the event still \
         names the old workspace: {tree:#}"
    );
}

/// Arrows inside the overview change the active workspace while the overview
/// is still open. A bar must track that immediately, not only once the
/// overview closes.
#[test]
fn overview_arrow_emits_focus_event_before_the_overview_closes() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));
    for command in ["workspace 2", "workspace 1"] {
        crate::command::execute(fixture.niri_state(), command);
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    key_event(&mut fixture, 116, true);
    key_event(&mut fixture, 116, false);
    fixture.niri_state().refresh_and_flush_clients();

    let mut changes = Vec::new();
    while let Some(((_, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        changes.push((
            event["change"].as_str().unwrap_or_default().to_owned(),
            event["current"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    assert!(
        changes
            .iter()
            .any(|(change, name)| change == "focus" && name == "2"),
        "the bar must learn about the new workspace while the overview is \
         still open, got {changes:?}"
    );

    // And GET_TREE, which is what waybar actually renders, must agree.
    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut workspaces);
    let focused = workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "GET_TREE must mark the arrowed-to workspace focused while the \
         overview is open: {tree:#}"
    );
}

/// The keyboard route into the same jump: open the overview, arrow to another
/// workspace, then Escape to leave. A bar must end up highlighting the
/// workspace the user landed on.
#[test]
fn overview_arrow_then_escape_emits_workspace_focus_events() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));

    for command in ["workspace 2", "workspace 1"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let before = active_idx(&mut fixture);

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();

    // Down arrow, then Escape to close the overview.
    for key in [116, 1] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
    }
    fixture.niri_state().refresh_and_flush_clients();

    assert_eq!(
        active_idx(&mut fixture),
        before + 1,
        "arrow then escape must leave the new workspace active"
    );

    let mut changes = Vec::new();
    while let Some(((event_type, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        assert_eq!(event_type, 1 << 31);
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        changes.push((
            event["change"].as_str().unwrap_or_default().to_owned(),
            event["current"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    assert!(
        changes
            .iter()
            .any(|(change, name)| change == "focus" && name == "2"),
        "a bar must be told workspace 2 is focused, got {changes:?}"
    );

    // Waybar ignores the event payload entirely and re-reads GET_TREE, then
    // reads `focused` and `visible` off the workspace nodes
    // (Waybar/src/modules/sway/workspaces.cpp:88-113,146-172,307-321,363-371).
    // The event is only the trigger; GET_TREE is what the bar renders.
    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut tree_workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut tree_workspaces);
    let tree_focused = tree_workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        tree_focused,
        vec!["2".to_string()],
        "GET_TREE is what waybar renders, and it must mark the new workspace \
         focused: {tree:#}"
    );

    let mut query = UnixStream::connect(&socket).unwrap();
    let workspaces = query_ipc(&mut fixture, &mut query, MessageType::GetWorkspaces);
    let focused = workspaces
        .as_array()
        .unwrap()
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "GET_WORKSPACES must agree with the focus event: {workspaces}"
    );
    let visible = workspaces
        .as_array()
        .unwrap()
        .iter()
        .filter(|ws| ws["visible"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        vec!["2".to_string()],
        "the workspace the user landed on must be the visible one: {workspaces}"
    );
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
fn runtime_mode_definition_with_set_creates_a_switchable_pango_mode() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode --pango_markup created set $destination workspace-7",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "default");
    assert_eq!(
        fixture.swayward().sway_variables,
        [("$destination".into(), "workspace-7".into())]
    );
    let config = fixture.swayward().config.borrow();
    let created = config
        .binding_modes
        .iter()
        .find(|mode| mode.name == "created")
        .unwrap();
    assert!(created.pango_markup);
    assert!(created.binds.0.is_empty());
    drop(config);

    let mut query = UnixStream::connect(&socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "created"])
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode created");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change":"created","pango_markup":true})
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace $destination")[0].success);
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-7".into())
    );

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode inline set $next workspace-8; workspace $next",
    );
    assert!(outcome.iter().all(|result| result.success), "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-8".into())
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode missing");
    assert!(!outcome[0].success);
    assert_eq!(outcome[0].error.as_deref(), Some("Unknown mode `missing'"));
}

/// Gesture binds are the only nested subcommands still refused, top level or
/// nested: swayward has no gesture command-binding table. The refusal happens
/// at parse time, so the named mode is not created either.
#[test]
fn runtime_mode_definition_rejects_only_gesture_binding_subcommands() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for subcommand in ["bindgesture swipe:3:left nop", "unbindgesture swipe:3:left"] {
        let command = format!("mode blocked {subcommand}");
        let outcome = crate::command::execute(fixture.niri_state(), &command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("gesture events have no sway command-binding model")
        );
    }
    assert!(fixture
        .swayward()
        .config
        .borrow()
        .binding_modes
        .iter()
        .all(|mode| mode.name != "blocked"));
}

/// `mode <name> bindsym` inserts into the named mode without switching to it
/// (`sway/sway/commands/mode.c:69-84`). The binding must fire once that mode
/// is active, stay silent in the default mode, and be discarded by reload.
#[test]
fn runtime_nested_mode_bindsym_fires_only_in_that_mode_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let added = crate::command::execute(
        fixture.niri_state(),
        "mode nested bindsym Mod4+a workspace nested-bound",
    );
    assert!(added[0].success, "{added:?}");
    // The nested form defines the mode but does not enter it.
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Inactive in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace still-default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("still-default")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-bound")
    );

    // Unbinding through the nested form removes it again.
    let removed = crate::command::execute(fixture.niri_state(), "mode nested unbindsym Mod4+a");
    assert!(removed[0].success, "{removed:?}");
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "mode nested bindsym Mod4+a workspace should-not-survive",
        )[0]
        .success
    );
    let config =
        swayward_config::Config::parse_mem(r#"mode "nested" { x { command "nop"; }; }"#).unwrap();
    fixture.niri_state().reload_config(Ok(config));
    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-check")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("reload-check")
    );
}

/// The keycode and switch nested forms reach the same tables as their
/// top-level counterparts, and target the named mode rather than the active
/// one.
#[test]
fn runtime_nested_mode_bindcode_and_bindswitch_target_the_named_mode() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    for command in [
        "mode nested bindcode 39 workspace nested-code",
        "mode nested bindswitch lid:on workspace nested-switch",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Neither fires in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace default-still")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-still")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-code")
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-switch")
    );

    for command in [
        "mode nested unbindcode 39",
        "mode nested unbindswitch lid:on",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );
}

/// A nested bind aimed at the mode the compositor is already in must not
/// leak into the default table, and a nested bind issued while a mode is
/// active must land in the named mode rather than the active one.
#[test]
fn runtime_nested_mode_bindsym_ignores_the_active_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds { Mod4+a { command "workspace default-mode"; }; }
mode "other" { Mod4+b { command "nop"; }; }
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode elsewhere bindsym Mod4+a workspace elsewhere-bound",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "other");

    // The active mode did not receive the binding.
    assert!(crate::command::execute(fixture.niri_state(), "workspace untouched")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("untouched")
    );

    // Neither did the default table.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-mode")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode elsewhere")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("elsewhere-bound")
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
fn portable_security_context_criteria_match_exactly_the_intended_windows() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let sandboxed =
        fixture.add_client_with_security_context(Some(crate::swayward::SecurityContextMetadata {
            sandbox_engine: Some("flatpak".into()),
            app_id: Some("org.example.Sandbox".into()),
            instance_id: Some("instance-1".into()),
        }));
    let other =
        fixture.add_client_with_security_context(Some(crate::swayward::SecurityContextMetadata {
            sandbox_engine: Some("snap".into()),
            app_id: Some("org.example.Other".into()),
            instance_id: Some("instance-2".into()),
        }));
    let unrestricted = fixture.add_client();
    map_test_window(&mut fixture, sandboxed, "same-app-id");
    map_test_window(&mut fixture, other, "same-app-id");
    map_test_window(&mut fixture, unrestricted, "same-app-id");

    for (criterion, expected_marks) in [
        (r#"sandbox_engine="^flatpak$""#, 1),
        (r#"sandbox_app_id="^org\.example\.Sandbox$""#, 1),
        (r#"sandbox_instance_id="^instance-1$""#, 1),
        // Missing metadata is not coerced to an empty string: this must not
        // match the unrestricted window.
        (r#"sandbox_engine="^$""#, 0),
    ] {
        let command = format!(r#"[{criterion}] mark --add portable"#);
        let outcome = crate::command::execute(fixture.niri_state(), &command);
        if expected_marks == 0 {
            assert!(!outcome[0].success, "{criterion}: {outcome:?}");
            assert_eq!(outcome[0].error.as_deref(), Some("No matching node."));
        } else {
            assert!(outcome[0].success, "{criterion}: {outcome:?}");
        }
        assert_eq!(
            fixture
                .swayward()
                .marks_by_window
                .values()
                .filter(|marks| marks.iter().any(|mark| mark == "portable"))
                .count(),
            expected_marks,
            "{criterion}"
        );
        fixture.swayward().marks_by_window.clear();
    }
}

#[test]
fn xdg_toplevel_tags_match_only_the_intended_windows() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let tagged = fixture.add_client();
    let other = fixture.add_client();

    let (tagged_surface, tagged_toplevel) = {
        let window = fixture.client(tagged).create_window();
        window.xdg_toplevel.set_app_id("same-app-id".into());
        (window.surface.clone(), window.xdg_toplevel.clone())
    };
    fixture
        .client(tagged)
        .set_toplevel_tag(&tagged_toplevel, "Editor");
    fixture.client(tagged).window(&tagged_surface).commit();
    fixture.roundtrip(tagged);
    let window = fixture.client(tagged).window(&tagged_surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(tagged);
    map_test_window(&mut fixture, other, "same-app-id");
    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"for_window [tag="^Other$"] mark --add updated-tag"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let other_toplevel = fixture.client(other).state.windows[0].xdg_toplevel.clone();
    fixture
        .client(other)
        .set_toplevel_tag(&other_toplevel, "Other");
    fixture.roundtrip(other);
    assert_eq!(
        fixture
            .swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "updated-tag"))
            .count(),
        1
    );

    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[tag="^Editor$"] mark --add tagged"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        fixture
            .swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "tagged"))
            .count(),
        1
    );

    for criterion in [r#"tag="^Missing$""#, r#"tag="^$""#] {
        let outcome = crate::command::execute(
            fixture.niri_state(),
            &format!(r#"[{criterion}] mark should-not-appear"#),
        );
        assert!(!outcome[0].success, "{criterion}: {outcome:?}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("No matching node."),
            "{criterion}"
        );
    }

    fixture.swayward().marks_by_window.clear();
    let tagged_window = fixture
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| mapped.tag().as_deref() == Some("Editor"))
        .unwrap()
        .1
        .window
        .clone();
    fixture.swayward().layout.activate_window(&tagged_window);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[tag="__focused__"] mark --add focused-tag"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        fixture
            .swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "focused-tag"))
            .count(),
        1
    );
}

#[test]
fn x11_only_criteria_fail_before_matching_wayland_windows() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "same-as-x11-class");

    for (criterion, error) in [
        (
            r#"class="same-as-x11-class""#,
            "X11-only criterion 'class' is unsupported",
        ),
        (
            r#"instance=".*""#,
            "X11-only criterion 'instance' is unsupported",
        ),
        ("id=1", "X11-only criterion 'id' is unsupported"),
        (
            r#"window_role=".*""#,
            "X11-only criterion 'window_role' is unsupported",
        ),
        (
            "window_type=normal",
            "X11-only criterion 'window_type' is unsupported",
        ),
    ] {
        let outcome = crate::command::execute(
            fixture.niri_state(),
            &format!(r#"[{criterion}] mark should-not-appear"#),
        );
        assert!(!outcome[0].success, "{criterion}: {outcome:?}");
        assert_eq!(outcome[0].parse_error, Some(true), "{criterion}");
        assert_eq!(outcome[0].error.as_deref(), Some(error), "{criterion}");
        assert!(fixture.swayward().marks_by_window.is_empty(), "{criterion}");
    }
}

#[test]
fn criteria_global_settings_require_matches_and_run_once_per_match() {
    use swayward_config::layout::{FocusWrapping, SmartBorders};
    use swayward_config::misc::PopupDuringFullscreen;

    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    let before = fixture.swayward().config.borrow().layout.focus_wrapping;
    assert_eq!(
        crate::command::execute(
            fixture.niri_state(),
            r#"[app_id="missing"] focus_wrapping toggle"#,
        ),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No matching node.".into()),
            parse_error: None,
        }]
    );
    assert_eq!(
        fixture.swayward().config.borrow().layout.focus_wrapping,
        before,
        "zero matches must not mutate global state"
    );

    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "matched");
    let command = concat!(
        r#"[app_id="matched"] floating_minimum_size 111 x 77, "#,
        "floating_maximum_size 999 x 777, focus_wrapping toggle, ",
        "force_focus_wrapping toggle, popup_during_fullscreen ignore, ",
        "smart_borders no_gaps, workspace_auto_back_and_forth toggle",
    );
    let outcomes = crate::command::execute(fixture.niri_state(), command);
    assert!(
        outcomes.iter().all(|outcome| outcome.success),
        "{outcomes:?}"
    );
    {
        let config = fixture.swayward().config.borrow();
        assert_eq!(config.layout.floating_minimum_size.width, 111);
        assert_eq!(config.layout.floating_minimum_size.height, 77);
        assert_eq!(config.layout.floating_maximum_size.width, 999);
        assert_eq!(config.layout.floating_maximum_size.height, 777);
        assert_eq!(config.layout.focus_wrapping, FocusWrapping::Force);
        assert_eq!(config.layout.smart_borders, SmartBorders::NoGaps);
        assert!(config.input.workspace_auto_back_and_forth);
        assert_eq!(
            config.popup_during_fullscreen,
            PopupDuringFullscreen::Ignore
        );
    }

    for _ in 0..2 {
        map_test_window(&mut fixture, client, "matched");
    }
    {
        let mut config = fixture.swayward().config.borrow_mut();
        config.layout.focus_wrapping = FocusWrapping::Yes;
        config.layout.smart_borders = SmartBorders::On;
        config.input.workspace_auto_back_and_forth = true;
    }
    crate::command::reset_global_setting_executions();
    for (index, (command, expected_wrapping, expected_smart_borders, expected_back_and_forth)) in [
        (
            r#"[app_id="matched"] focus_wrapping toggle"#,
            FocusWrapping::No,
            SmartBorders::On,
            true,
        ),
        (
            r#"[app_id="matched"] force_focus_wrapping toggle"#,
            FocusWrapping::Force,
            SmartBorders::On,
            true,
        ),
        (
            r#"[app_id="matched"] smart_borders toggle"#,
            FocusWrapping::Force,
            SmartBorders::Off,
            true,
        ),
        (
            r#"[app_id="matched"] workspace_auto_back_and_forth toggle"#,
            FocusWrapping::Force,
            SmartBorders::Off,
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let config = fixture.swayward().config.borrow();
        assert_eq!(config.layout.focus_wrapping, expected_wrapping, "{command}");
        assert_eq!(
            config.layout.smart_borders, expected_smart_borders,
            "{command}"
        );
        assert_eq!(
            config.input.workspace_auto_back_and_forth, expected_back_and_forth,
            "{command}"
        );
        assert_eq!(
            crate::command::global_setting_executions(),
            (index + 1) * 3,
            "{command} must execute once for each of the three retained targets"
        );
    }
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

#[test]
fn criteria_lifecycle_commands_fail_without_changing_state() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    add_two_tiled_windows(&mut fixture);
    let before = fixture.swayward().config.borrow().layout.clone();
    let for_window = fixture.swayward().for_window.len();

    for (criteria, matches) in [
        (r#"[app_id="missing"]"#, 0),
        (r#"[app_id="left"]"#, 1),
        ("[all]", 2),
    ] {
        for command in ["exit", "reload"] {
            let input = format!("{criteria} {command}");
            let outcome = crate::command::execute(fixture.niri_state(), &input);
            let expected = if matches == 0 {
                "No matching node.".to_owned()
            } else {
                format!("criteria are not supported for {command}")
            };
            assert_eq!(outcome.len(), 1, "{input}: {outcome:?}");
            assert_eq!(
                outcome[0].error.as_deref(),
                Some(expected.as_str()),
                "{input}"
            );
            assert_eq!(outcome[0].parse_error, None, "{input}");
            assert!(!fixture.swayward().shutdown_requested, "{input}");
            assert_eq!(fixture.swayward().config.borrow().layout, before, "{input}");
            assert_eq!(fixture.swayward().for_window.len(), for_window, "{input}");
        }
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

/// Tiled window rects on one named workspace.
///
/// [`tiled_window_rects`] flattens every workspace, so a test that creates a
/// second workspace cannot use it: the two sets interleave under the sort.
fn tiled_window_rects_on(fixture: &mut Fixture, workspace: &str) -> Vec<Value> {
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
    fn find_ws<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        if value["type"] == "workspace" && value["name"] == name {
            return Some(value);
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    if let Some(found) = find_ws(child, name) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    let ws = find_ws(&tree, workspace).expect("workspace not in tree");
    let mut rects = Vec::new();
    collect(ws, &mut rects);
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

/// Sway's two-argument form sets the default for workspaces created later and
/// leaves existing ones alone (`sway/sway/commands/gaps.c:48-91`), because a
/// live workspace reads its own gaps, not the global default
/// (`sway/sway/tree/workspace.c:224-225`).
#[test]
fn gaps_defaults_form_does_not_disturb_an_existing_workspace() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.outer_gaps_configured = true;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 40")[0].success);

    // The default moved...
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 40.);
    // ...and the existing workspace did not.
    assert_eq!(tiled_window_rects(&mut fixture), before);
}

/// The converse, and the other half of the "must not fight" requirement: the
/// runtime form mutates the live workspace and must leave the default alone, so
/// a workspace created afterwards still inherits the configured default.
#[test]
fn runtime_gaps_form_does_not_overwrite_the_defaults() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 30")[0].success);

    // The live workspace moved.
    let after = tiled_window_rects(&mut fixture);
    assert_eq!(after[0]["x"], 30);
    // The default did not, so a later workspace still gets 10.
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 10.);

    // Prove it by creating one and reading its gaps back.
    assert!(crate::command::execute(fixture.niri_state(), "workspace fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "fresh")[0]["x"], 10);
}

/// Both directions in one session: setting the default, then a runtime change,
/// then another default write, must not let either clobber the other.
#[test]
fn gaps_defaults_and_runtime_forms_hold_separate_state() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 25")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 5")[0].success);

    // Runtime change won for the live workspace; the default is still 25.
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 25.);

    // A further default write still does not touch the live workspace.
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 50")[0].success);
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 50.);
}

/// `workspace <name> gaps <kind> <px>` is a per-workspace-name default applied
/// at creation (`sway/sway/tree/workspace.c:226-243`), so it must affect a
/// workspace of that name created afterwards and not the current one.
#[test]
fn workspace_gaps_apply_to_a_later_workspace_of_that_name() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(
        crate::command::execute(fixture.niri_state(), "workspace roomy gaps inner 45")[0].success
    );

    // The current workspace is untouched, as in sway.
    assert_eq!(tiled_window_rects(&mut fixture), before);
    // The global default is untouched too: this is per-name state.
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 10.);

    // A workspace with that name picks the value up.
    assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "roomy")[0]["x"], 45);

    // A differently named workspace still gets the global default.
    assert!(crate::command::execute(fixture.niri_state(), "workspace plain")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "plain")[0]["x"], 10);
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
    subscriber.set_nonblocking(true).unwrap();
    fixture.dispatch();
    let mut byte = [0];
    assert!(matches!(
        subscriber.read(&mut byte),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));

    std::fs::remove_file(path).unwrap();
}

#[test]
fn reload_reports_malformed_config_in_the_command_reply() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let mut fixture = Fixture::new();
    let path = std::env::temp_dir().join(format!(
        "swayward-bad-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, "binds { Mod+H { command; }; }").unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );

    let outcome = crate::command::execute(fixture.niri_state(), "reload");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Error(s) reloading config.".into()),
            parse_error: None,
        }]
    );

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
fn reload_updates_runtime_for_window_execution_state() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "for_window [con_mark=trigger] mark --add fired",
        )[0]
        .success
    );

    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let mapped = fixture.swayward().layout.focus().unwrap().id();

    fixture.swayward().executed_for_window.insert((
        mapped,
        "[con_mark=trigger]".into(),
        "mark --add fired".into(),
    ));
    fixture.niri_state().reload_config(Err(()));
    assert!(
        !fixture.swayward().executed_for_window.is_empty(),
        "a failed reload must preserve criteria from the active config"
    );
    fixture.swayward().executed_for_window.clear();

    assert!(crate::command::execute(fixture.niri_state(), "mark trigger")[0].success);
    assert!(fixture
        .swayward()
        .executed_for_window
        .iter()
        .any(|(window, criteria, _)| *window == mapped && criteria == "[con_mark=trigger]"));
    assert!(!fixture.swayward().runtime_for_window.is_empty());

    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(fixture.swayward().executed_for_window.is_empty());
    assert!(fixture.swayward().for_window.is_empty());
    assert!(fixture.swayward().runtime_for_window.is_empty());
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
        "before"
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
        "%app_id"
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
fn title_format_updates_a_split_container_representation() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for title in ["one", "two", "three"] {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("app-{title}"));
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    fixture.swayward().layout.consume_or_expel_window_left(None);
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "mark formatted-split")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[con_mark=formatted-split] title_format group: %title %app_id"#,
    );
    assert!(outcome[0].success, "{outcome:?}");

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    let split = find_json_node_with_mark(&tree, "formatted-split").unwrap();
    assert_eq!(split["name"], Value::Null);
    let workspace_node = tree["nodes"][1]["nodes"][0].as_object().unwrap();
    assert_eq!(
        workspace_node["representation"],
        "T[app-one V[app-three app-two]]"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert!(workspace
        .tiling()
        .titlebar_titles()
        .iter()
        .any(|title| title == "group: V[three two] %app_id"));
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
fn runtime_assign_applies_only_to_windows_mapped_after_registration() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    map_test_window(&mut f, client, "assigned-existing");
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"assign [app_id="^assigned-"] workspace 7: target"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    map_test_window(&mut f, client, "assigned-future");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace_for = |app_id: &str| {
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|output| output["nodes"].as_array().unwrap())
            .find(|workspace| find_json_node_with_app_id(workspace, app_id).is_some())
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(workspace_for("assigned-existing"), "1");
    assert_eq!(workspace_for("assigned-future"), "7: target");
}

#[test]
fn runtime_assign_uses_the_first_matching_rule() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [
        r#"assign [app_id="^assigned$"] workspace first"#,
        r#"assign [app_id="^assigned$"] workspace second"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "first");
}

#[test]
fn file_config_assignment_precedes_a_runtime_assignment() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            match app-id="^assigned$"
            open-on-workspace "configured"
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^assigned$"] workspace runtime"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "configured");
}

#[test]
fn runtime_assign_supports_workspace_numbers() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^numbered$"] workspace number 7: target"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "numbered");

    assert!(f
        .swayward()
        .layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("7: target")));
}

#[test]
fn runtime_assign_skips_a_missing_output_and_uses_the_next_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [
        r#"assign [app_id="^assigned$"] output missing"#,
        r#"assign [app_id="^assigned$"] workspace fallback"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "fallback");
}

#[test]
fn runtime_assign_supports_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(1);
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^output$"] output headless-2"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "output");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let output = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| find_json_node_with_app_id(output, "output").is_some())
        .unwrap();
    assert_eq!(output["name"], "headless-2");
}

#[test]
fn successful_reload_clears_runtime_assign_and_no_focus_rules() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for command in [
        r#"assign [app_id="^future$"] workspace target"#,
        r#"no_focus [app_id="^future$"]"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    assert_eq!(f.swayward().runtime_window_rules.len(), 2);

    f.niri_state()
        .reload_config(Err::<swayward_config::Config, _>(()));
    assert_eq!(f.swayward().runtime_window_rules.len(), 2);

    f.niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(f.swayward().runtime_window_rules.is_empty());
}

#[test]
fn runtime_no_focus_does_not_leave_the_first_window_unfocused() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), r#"no_focus [app_id="^first$"]"#)[0].success);
    map_test_window(&mut f, client, "first");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["app_id"],
        "first"
    );
}

#[test]
fn runtime_no_focus_applies_only_to_windows_mapped_after_registration() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["existing", "already-mapped"] {
        map_test_window(&mut f, client, app_id);
    }
    let focused_app_id = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node(&tree, "con", true).unwrap()["app_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(focused_app_id(&mut f), "already-mapped");
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"no_focus [app_id="^(already-mapped|future)$"]"#,
        )[0]
        .success
    );
    assert_eq!(
        focused_app_id(&mut f),
        "already-mapped",
        "registering no_focus must not change an already mapped view"
    );

    map_test_window(&mut f, client, "future");
    assert_eq!(
        focused_app_id(&mut f),
        "already-mapped",
        "a future no_focus match must not steal focus"
    );
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
fn marking_a_mapped_window_runs_each_newly_matching_for_window_rule_once() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(
        crate::command::execute(
            f.niri_state(),
            "for_window [con_mark=trigger] sticky toggle",
        )[0]
        .success
    );

    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("first".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    let mapped = f.swayward().layout.focus().unwrap().id();
    let sticky = |f: &mut Fixture, app_id: &str| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node_with_app_id(&tree, app_id).unwrap()["sticky"] == true
    };
    assert!(!sticky(&mut f, "first"));

    assert!(crate::command::execute(f.niri_state(), "mark trigger")[0].success);
    assert!(
        sticky(&mut f, "first"),
        "the mark-dependent action must run"
    );

    let second = f.client(client).create_window();
    second.xdg_toplevel.set_app_id("second".into());
    second.commit();
    let surface = second.surface.clone();
    f.roundtrip(client);
    let second = f.client(client).window(&surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark trigger")[0].success);
    assert!(sticky(&mut f, "second"));

    let con_id = crate::ipc::tree::window_id(mapped);
    assert!(
        crate::command::execute(f.niri_state(), &format!("[con_id={con_id}] mark trigger"),)[0]
            .success
    );
    assert!(
        sticky(&mut f, "first"),
        "moving a global mark away and back must not rerun the first view's rule"
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
    assert_same_values(&fixture, &ours, "$tree");
    assert_tree_rectangles_match_fixture(&fixture, &ours, "$tree");
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

    let window = f.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-2".into());
    window.set_title("fixture-2");
    window.set_size(696, 491);
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_floating.tree.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_rectangle_roles_match_fixture(&fixture, &ours, "$tree");
    assert_focus_matches_fixture(&fixture, &ours, "$tree");
    let expected = find_json_node(&fixture, "floating_con", false).unwrap();
    let actual_percent = ours["nodes"][1]["nodes"][0]["floating_nodes"][0]["percent"]
        .as_f64()
        .unwrap();
    let expected_percent = expected["percent"].as_f64().unwrap();
    assert!(
        (actual_percent - expected_percent).abs() < 0.02,
        "floating percent: expected {expected_percent}, got {actual_percent}"
    );
    let floating = find_json_node(&ours, "floating_con", false).unwrap();
    assert_eq!(
        floating["deco_rect"]["height"].as_i64().unwrap() > 0,
        expected["deco_rect"]["height"].as_i64().unwrap() > 0
    );
    assert_eq!(floating["deco_rect"]["x"], floating["rect"]["x"]);
    assert_eq!(floating["deco_rect"]["width"], floating["rect"]["width"]);
    assert_eq!(floating["deco_rect"]["y"], floating["rect"]["y"]);

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
        "../../tests/fixtures/sway/one_floating.workspaces.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$workspaces");
    let expected_focus = fixture[0]["focus"].as_array().unwrap();
    let actual_focus = ours[0]["focus"].as_array().unwrap();
    assert_eq!(expected_focus.len(), actual_focus.len());
    assert_eq!(ours[0]["floating_nodes"].as_array().unwrap().len(), 1);
    assert_eq!(ours[0]["floating_nodes"][0]["app_id"], "fixture-2");
    assert_eq!(ours[0]["focused"], true);
    assert_eq!(ours[0]["representation"], fixture[0]["representation"]);

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.outputs.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$outputs");
    assert_same_values(&fixture, &ours, "$outputs");

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
fn criteria_split_commands_apply_to_every_matched_window() {
    for (command, expected_layout) in [
        ("split vertical", "splitv"),
        ("splitv", "splitv"),
        ("splith", "splith"),
        ("splitt", "splitv"),
    ] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        for app_id in ["matched-1", "other", "matched-2", "matched-3"] {
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

        let outcome =
            crate::command::execute(f.niri_state(), &format!("[app_id=matched-] {command}"));
        assert!(outcome[0].success, "{command}: {outcome:?}");

        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        for app_id in ["matched-1", "matched-2", "matched-3"] {
            let parent = find_json_parent_of_app_id(&tree, app_id).unwrap();
            assert_eq!(parent["layout"], expected_layout, "{command}: {app_id}");
            assert_eq!(
                parent["nodes"].as_array().unwrap().len(),
                1,
                "{command}: {app_id}"
            );
        }
        let other_parent = find_json_parent_of_app_id(&tree, "other").unwrap();
        assert_eq!(other_parent["layout"], "splith", "{command}: non-match");
        assert_eq!(
            other_parent["nodes"].as_array().unwrap().len(),
            4,
            "{command}: non-match"
        );
    }
}

#[test]
fn split_none_flattens_only_a_singleton_parent_and_preserves_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    map_test_window(&mut f, client, "second");
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let before = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .ipc_tiling_tree();
    assert_eq!(before.nodes().len(), 4);

    let outcome = crate::command::execute(f.niri_state(), "split none");
    assert!(outcome[0].success, "{outcome:?}");
    {
        let workspace = f.swayward().layout.active_workspace().unwrap();
        assert_eq!(workspace.ipc_tiling_tree().nodes().len(), 3);
        workspace.verify_invariants(None);
    }
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);

    let before = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .ipc_tiling_tree();
    let outcome = crate::command::execute(f.niri_state(), "split none");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Can only flatten a child container with no siblings".into()),
            parse_error: None,
        }]
    );
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.ipc_tiling_tree(), before);
    workspace.verify_invariants(None);
}

#[test]
fn criteria_split_none_flattens_the_matched_singleton_parent() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    map_test_window(&mut f, client, "matched");
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "[app_id=matched] split none");
    assert!(outcome[0].success, "{outcome:?}");
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.ipc_tiling_tree().nodes().len(), 3);
    workspace.verify_invariants(None);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
}

#[test]
fn criteria_split_command_applies_to_a_matched_split_container() {
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
    assert!(crate::command::execute(f.niri_state(), "mark split-target")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "[con_mark=split-target] split vertical");
    assert!(outcome[0].success, "{outcome:?}");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let marked = find_json_node_with_mark(&tree, "split-target").unwrap();
    fn find_parent(value: &Value, id: i64) -> Option<&Value> {
        for key in ["nodes", "floating_nodes"] {
            let children = value[key].as_array()?;
            if children.iter().any(|child| child["id"] == id) {
                return Some(value);
            }
            if let Some(parent) = children.iter().find_map(|child| find_parent(child, id)) {
                return Some(parent);
            }
        }
        None
    }
    let parent = find_parent(&tree, marked["id"].as_i64().unwrap()).unwrap();
    assert_eq!(parent["layout"], "splitv");
    assert_eq!(parent["nodes"].as_array().unwrap().len(), 1);
}

#[test]
fn criteria_layout_applies_to_every_matched_windows_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for workspace in ["one", "two"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for app_id in ["matched-layout", "other"] {
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
    }

    for (command, expected) in [
        ("layout tabbed", TreeLayout::Tabbed),
        ("layout default", TreeLayout::SplitH),
        ("layout toggle all", TreeLayout::SplitV),
    ] {
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!("[app_id=matched-layout] {command}"),
        );
        assert!(outcome[0].success, "{command}: {outcome:?}");
        for workspace in ["one", "two"] {
            let tree = f
                .swayward()
                .layout
                .workspaces()
                .find(|(_, _, candidate)| candidate.sway_name().as_deref() == Some(workspace))
                .unwrap()
                .2
                .ipc_tiling_tree();
            assert!(
                matches!(
                    tree,
                    IpcNode::Split {
                        layout,
                        ..
                    } if layout == expected
                ),
                "{command}: {workspace}"
            );
        }
    }
}

#[test]
fn criteria_fullscreen_applies_to_every_matched_split_and_its_descendants() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut windows = Vec::new();
    for workspace in ["one", "two"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for app_id in ["outside", "inside-1", "inside-2"] {
            map_test_window(&mut f, client, app_id);
            windows.push((
                workspace,
                app_id,
                f.swayward().layout.focus().unwrap().window.clone(),
            ));
        }
        f.swayward().layout.consume_or_expel_window_left(None);
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!("mark fullscreen-group-{workspace}")
            )[0]
            .success
        );
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        "[con_mark=fullscreen-group-] fullscreen enable",
    );
    assert!(outcome[0].success, "{outcome:?}");
    for (workspace, app_id, window) in &windows {
        assert_eq!(
            f.swayward().layout.fullscreen_mode(window),
            (*app_id != "outside").then_some(crate::layout::tiling_tree::FullscreenMode::Workspace),
            "{workspace}: {app_id}"
        );
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        "[con_mark=fullscreen-group-] fullscreen disable",
    );
    assert!(outcome[0].success, "{outcome:?}");
    for (workspace, app_id, window) in windows {
        assert_eq!(
            f.swayward().layout.fullscreen_mode(&window),
            None,
            "{workspace}: {app_id}"
        );
    }
}

#[test]
fn criteria_kill_closes_every_descendant_of_every_matched_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for workspace in ["one", "two"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for app_id in ["outside", "inside-1", "inside-2"] {
            let window = f.client(client).create_window();
            window.xdg_toplevel.set_app_id(app_id.into());
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
            surfaces.push((app_id, surface));
        }
        f.swayward().layout.consume_or_expel_window_left(None);
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(
            crate::command::execute(f.niri_state(), &format!("mark kill-group-{workspace}"))[0]
                .success
        );
    }

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=kill-group-] kill");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    for (app_id, surface) in surfaces {
        assert_eq!(
            f.client(client).window(&surface).close_requested,
            app_id != "outside",
            "{app_id}"
        );
    }
}

#[test]
fn kill_closes_every_descendant_of_the_focused_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
    }
    f.swayward().layout.consume_or_expel_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "kill");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert!(!f.client(client).window(&surfaces[0]).close_requested);
    assert!(f.client(client).window(&surfaces[1]).close_requested);
    assert!(f.client(client).window(&surfaces[2]).close_requested);
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
fn mark_on_empty_workspace_fails_without_removing_existing_mark() {
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

    assert!(crate::command::execute(f.niri_state(), "mark keepme")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "mark keepme");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Only containers can have marks")
    );
    assert_eq!(
        f.swayward().marks_by_window.values().next().unwrap(),
        &["keepme".to_owned()]
    );
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
    let unsupported_id =
        crate::command::execute(f.niri_state(), &format!("swap container with id {x11_id}"));
    assert_eq!(unsupported_id[0].parse_error, Some(true));
    assert_eq!(
        unsupported_id[0].error.as_deref(),
        Some("swap container with id is unsupported because X11 window IDs are unavailable")
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
    let outcome = crate::command::execute(f.niri_state(), "mark unique");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(mark_count(f.niri_state(), "unique"), 1);
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
    assert_eq!(tree.nodes[0].nodes[0].fullscreen_mode, 1);
    assert_eq!(
        tree.nodes[0].nodes[0].focus,
        tree.nodes[0].nodes[0]
            .floating_nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>()
    );

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
    let mut events = Vec::new();
    for _ in 0..2 {
        let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
        assert_eq!(event_type, (1 << 31) | 3);
        events.push(serde_json::from_str::<Value>(&payload).unwrap());
    }
    assert_eq!(events[0]["change"], "move");
    assert_eq!(
        events[0]["container"]["id"],
        crate::ipc::tree::window_id(scratchpad_id)
    );
    assert_eq!(events[0]["container"]["type"], "floating_con");
    assert_eq!(events[1]["change"], "focus");
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
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["fullscreen_mode"],
        2
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
        [(9, "9 web")]
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
fn default_border_changes_only_windows_mapped_after_the_command() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();

    for app_id in ["existing", "explicit"] {
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
        crate::command::execute(f.niri_state(), r#"[app_id="explicit"] border pixel 7"#,)[0]
            .success
    );
    assert!(crate::command::execute(f.niri_state(), "default_border pixel 3")[0].success);

    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("new".into());
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
    let existing = find_json_node_with_app_id(&tree, "existing").unwrap();
    assert_eq!(existing["border"], "normal");
    assert_eq!(existing["current_border_width"], 4);
    let explicit = find_json_node_with_app_id(&tree, "explicit").unwrap();
    assert_eq!(explicit["border"], "pixel");
    assert_eq!(explicit["current_border_width"], 7);
    let new = find_json_node_with_app_id(&tree, "new").unwrap();
    assert_eq!(new["border"], "pixel");
    assert_eq!(new["current_border_width"], 3);
}

#[test]
fn default_floating_border_changes_only_windows_mapped_after_the_command() {
    let config = swayward_config::Config::parse_mem("window-rule { open-floating true; }").unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();

    for app_id in ["existing-float", "new-float"] {
        if app_id == "new-float" {
            assert!(
                crate::command::execute(f.niri_state(), "default_floating_border pixel 3",)[0]
                    .success
            );
        }
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
    let existing = find_json_node_with_app_id(&tree, "existing-float").unwrap();
    assert_eq!(existing["border"], "normal");
    assert_eq!(existing["current_border_width"], 4);
    let new = find_json_node_with_app_id(&tree, "new-float").unwrap();
    assert_eq!(new["border"], "pixel");
    assert_eq!(new["current_border_width"], 3);
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
    let wrapped = crate::command::execute(f.niri_state(), "move workspace to output right");
    assert!(wrapped[0].success, "{wrapped:?}");
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "middle"
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

/// Focus a second window by command with the pointer parked over the first,
/// then re-run the focus-follows-mouse hook without moving the pointer.
///
/// Sway's `yes` leaves focus alone because the hovered window did not change,
/// while `always` pulls focus back under the pointer
/// (`sway/sway/input/seatop_default.c:590-598`). Returns whether focus
/// returned to the hovered window.
fn focus_returns_under_stationary_pointer(
    mode: swayward_config::input::FocusFollowsMouseMode,
) -> bool {
    let mut config = swayward_config::Config::default();
    config.input.focus_follows_mouse = Some(swayward_config::input::FocusFollowsMouse {
        mode,
        max_scroll_amount: None,
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    // Two tiled windows side by side, so a point exists over each.
    let mut ids = Vec::new();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        // Commit the size the compositor asked for, so the surface has a
        // real hit-testable region.
        let size = window.configures_received.last().unwrap().1.size;
        window.set_size(size.0 as u16, size.1 as u16);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let focus = f.swayward().layout.focus().unwrap();
        ids.push((focus.id(), focus.window.clone()));
    }
    let (first, first_window) = ids[0].clone();
    let (second, second_window) = ids[1].clone();

    // Find a point the compositor itself reports as over the first window,
    // rather than assuming a tiling geometry.
    let location = (0..128)
        .map(|step| {
            smithay::utils::Point::<f64, smithay::utils::Logical>::from((
                f64::from(step) * 10. + 5.,
                400.,
            ))
        })
        .find(|&point| {
            f.swayward()
                .contents_under(point)
                .window
                .is_some_and(|(window, _)| window == first_window)
        })
        .expect("no point over the first window");

    // Park the pointer over the first window; it becomes the hovered node.
    f.niri_state().move_cursor(location);

    // Move focus to the other window for a reason unrelated to the pointer,
    // which stays exactly where it is. This is sway's "focus got moved due to,
    // say, a workspace switch" case.
    f.swayward().layout.activate_window(&second_window);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), second);

    // Re-run the hook with the pointer still over the first window. The
    // hovered node did not change, so only `always` acts.
    let under = f.swayward().contents_under(location);
    f.swayward().handle_focus_follows_mouse(&under);
    f.swayward().layout.focus().unwrap().id() == first
}

#[test]
fn focus_follows_mouse_always_refocuses_the_still_hovered_window() {
    use swayward_config::input::FocusFollowsMouseMode;

    // The two modes must disagree here. If they agreed, the assertion would
    // also pass against an implementation that stored `always` as `yes`.
    assert!(
        !focus_returns_under_stationary_pointer(FocusFollowsMouseMode::Yes),
        "`yes` must not re-focus a window the pointer never left"
    );
    assert!(
        focus_returns_under_stationary_pointer(FocusFollowsMouseMode::Always),
        "`always` must re-focus the hovered window after focus moved away"
    );
}

/// Warp policy applied to a focus change within the output the pointer is
/// already on.
///
/// `WARP_OUTPUT` returns early when the pointer already sits on the focused
/// output, so a same-output focus change does not move it. `WARP_CONTAINER`
/// warps to the focused container regardless
/// (`sway/sway/input/seat.c:1526-1547`). Returns whether the pointer moved.
fn pointer_moves_on_same_output_focus(mode: swayward_config::input::MouseWarping) -> bool {
    let mut config = swayward_config::Config::default();
    config.input.mouse_warping = mode;
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    for _ in 0..2 {
        let window = f.client(client).create_window();
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        let size = window.configures_received.last().unwrap().1.size;
        window.set_size(size.0 as u16, size.1 as u16);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    // Put the pointer somewhere on the focused output that is not the centre
    // of the focused window.
    let start = smithay::utils::Point::<f64, smithay::utils::Logical>::from((5., 5.));
    f.niri_state().move_cursor(start);
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        start
    );

    // A focus change that stays on this one output.
    assert!(crate::command::execute(f.niri_state(), "focus left")[0].success);
    f.niri_state().maybe_warp_cursor_to_focus();

    f.swayward().seat.get_pointer().unwrap().current_location() != start
}

/// What a modifier-held click on the given button starts.
///
/// Sway derives the move and resize buttons from the inverse bit, so `normal`
/// is left-move/right-resize and `inverse` swaps them
/// (`sway/sway/input/seatop_default.c:359-363`).
#[derive(Debug, PartialEq, Eq)]
enum DragKind {
    Nothing,
    Move,
    Resize,
}

fn drag_started_by(inverse: bool, button: u32) -> DragKind {
    let mut config = swayward_config::Config::default();
    config.input.floating_modifier = Some(swayward_config::input::FloatingModifier {
        modifier: swayward_config::input::ModKey::Super,
        inverse,
    });
    // Float the window: a lone tiled window has no neighbour to take space
    // from, so a tiling resize would refuse and hide the button mapping.
    config.window_rules.push(swayward_config::WindowRule {
        open_floating: Some(true),
        ..Default::default()
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    // A floating window is configured with a zero size, meaning the client
    // picks, so ask for a concrete one.
    window.set_size(600, 400);
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(600, 400);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    // A resize needs a resize edge under the cursor, so pick a point that is
    // both over a window and on one of its edges. Then the same position can
    // start either gesture and only the button decides which.
    let output = f.niri_output(1);
    let location = (0..1280)
        .map(|step| {
            smithay::utils::Point::<f64, smithay::utils::Logical>::from((f64::from(step), 400.))
        })
        .find(|&point| {
            f.niri_state().move_cursor(point);
            let over_window = f.swayward().window_under_cursor().is_some();
            let on_edge = f
                .swayward()
                .global_space
                .output_geometry(&output)
                .and_then(|geo| {
                    f.swayward()
                        .layout
                        .resize_edges_under(&output, point - geo.loc.to_f64())
                })
                .is_some_and(|edges| !edges.is_empty());
            over_window && on_edge
        })
        .expect("no point both over a window and on a resize edge");

    pointer_motion_absolute(&mut f, location.x, location.y);

    // Hold the floating modifier, then press the button.
    key_event(&mut f, 133, true);
    pointer_button(&mut f, button, true);

    // Inspect the concrete grab the press installed, which names the gesture
    // exactly rather than inferring it.
    let pointer = f.swayward().seat.get_pointer().unwrap();
    let kind = pointer
        .with_grab(|_, grab| {
            if grab.is::<crate::input::move_grab::MoveGrab>() {
                DragKind::Move
            } else if grab.is::<crate::input::resize_grab::ResizeGrab>() {
                DragKind::Resize
            } else {
                DragKind::Nothing
            }
        })
        .unwrap_or(DragKind::Nothing);

    pointer_button(&mut f, button, false);
    key_event(&mut f, 133, false);
    kind
}

#[test]
fn floating_modifier_inverse_swaps_the_move_and_resize_buttons() {
    const LEFT: u32 = 0x110;
    const RIGHT: u32 = 0x111;

    // normal: left moves, right resizes.
    assert_eq!(drag_started_by(false, LEFT), DragKind::Move);
    assert_eq!(drag_started_by(false, RIGHT), DragKind::Resize);
    // inverse: the two swap. If the inverse bit were dropped, these two
    // assertions would match the normal case above and fail.
    assert_eq!(drag_started_by(true, LEFT), DragKind::Resize);
    assert_eq!(drag_started_by(true, RIGHT), DragKind::Move);
}

fn tiled_drag_fixture() -> Fixture {
    let mut config = swayward_config::Config::default();
    config.window_rules.push(swayward_config::WindowRule {
        sway_border: Some(swayward_config::SwayWindowBorderStyle::Normal),
        ..Default::default()
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    pointer_motion_absolute(&mut f, 640., 400.);
    f
}

fn tiled_titlebar_point(f: &mut Fixture) -> smithay::utils::Point<f64, smithay::utils::Logical> {
    let rect = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiling()
        .titlebar_rects()[0]
        .1;
    smithay::utils::Point::from((rect.loc.x + rect.size.w / 2., rect.loc.y + rect.size.h / 2.))
}

fn move_grab_state(f: &mut Fixture) -> Option<bool> {
    f.swayward()
        .seat
        .get_pointer()
        .unwrap()
        .with_grab(|_, grab| {
            grab.downcast_ref::<crate::input::move_grab::MoveGrab>()
                .map(|grab| grab.is_move())
        })
        .flatten()
}

#[test]
fn tiling_drag_disabled_does_not_start_a_modifier_drag() {
    let mut f = tiled_drag_fixture();
    assert!(crate::command::execute(f.niri_state(), "tiling_drag no")[0].success);

    key_event(&mut f, 133, true);
    pointer_button(&mut f, 0x110, true);

    assert_eq!(move_grab_state(&mut f), None);
}

#[test]
fn tiling_drag_threshold_delays_a_titlebar_move() {
    let mut f = tiled_drag_fixture();
    assert!(crate::command::execute(f.niri_state(), "tiling_drag_threshold 100")[0].success);
    let start = tiled_titlebar_point(&mut f);
    pointer_motion_absolute(&mut f, start.x, start.y);
    pointer_button(&mut f, 0x110, true);
    pointer_motion_absolute(&mut f, start.x + 100., start.y);
    assert_eq!(move_grab_state(&mut f), Some(false));

    pointer_motion_absolute(&mut f, start.x + 101., start.y);
    assert_eq!(move_grab_state(&mut f), Some(true));
}

#[test]
fn tiling_drag_defaults_remain_enabled_with_a_nine_pixel_threshold() {
    let config = swayward_config::Config::default();
    assert!(config.input.tiling_drag);
    assert_eq!(config.input.tiling_drag_threshold, 9);

    let mut f = tiled_drag_fixture();
    let start = tiled_titlebar_point(&mut f);
    pointer_motion_absolute(&mut f, start.x, start.y);
    pointer_button(&mut f, 0x110, true);
    pointer_motion_absolute(&mut f, start.x + 9., start.y);
    assert_eq!(move_grab_state(&mut f), Some(false));
    pointer_motion_absolute(&mut f, start.x + 10., start.y);
    assert_eq!(move_grab_state(&mut f), Some(true));
}

#[test]
fn floating_modifier_none_disables_the_drag() {
    let mut config = swayward_config::Config::default();
    config.input.floating_modifier = Some(swayward_config::input::FloatingModifier {
        modifier: swayward_config::input::ModKey::None,
        inverse: false,
    });
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    pointer_motion_absolute(&mut f, 640., 400.);
    // Super is held, but `none` means no modifier can arm the drag.
    key_event(&mut f, 133, true);
    pointer_button(&mut f, 0x110, true);
    let pointer = f.swayward().seat.get_pointer().unwrap();
    let dragging = pointer
        .with_grab(|_, grab| {
            grab.is::<crate::input::move_grab::MoveGrab>()
                || grab.is::<crate::input::resize_grab::ResizeGrab>()
        })
        .unwrap_or(false);
    assert!(!dragging, "`floating_modifier none` must not start a drag");
    pointer_button(&mut f, 0x110, false);
    key_event(&mut f, 133, false);
}

#[test]
fn mouse_warping_output_warps_when_focus_crosses_outputs() {
    // The other half of `output`: the pointer is not on the newly focused
    // output, so the early return does not apply and it warps.
    let mut config = swayward_config::Config::default();
    config.input.mouse_warping = swayward_config::input::MouseWarping::Output;
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    f.add_output(2, (1280, 800));
    let client = f.add_client();

    // A window on the second output, with the pointer left on the first.
    f.niri_focus_output(2);
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let start = smithay::utils::Point::<f64, smithay::utils::Logical>::from((5., 5.));
    f.niri_state().move_cursor(start);
    let first_output = f.niri_output(1);
    let second_output = f.niri_output(2);
    let output_at = |f: &mut Fixture, point| {
        f.swayward()
            .global_space
            .output_under(point)
            .next()
            .cloned()
    };
    assert_eq!(
        output_at(&mut f, start),
        Some(first_output),
        "the pointer should start on the output that is not focused"
    );

    f.niri_state().maybe_warp_cursor_to_focus();

    let moved = f.swayward().seat.get_pointer().unwrap().current_location();
    assert_ne!(moved, start, "`output` must warp across an output boundary");
    assert_eq!(
        output_at(&mut f, moved),
        Some(second_output),
        "the warp must land on the focused output"
    );
}

#[test]
fn mouse_warping_container_warps_within_an_output_and_output_does_not() {
    use swayward_config::input::MouseWarping;

    // The two modes must disagree here, otherwise the assertion would also
    // pass against an implementation that collapsed them to one boolean.
    assert!(
        !pointer_moves_on_same_output_focus(MouseWarping::Output),
        "`output` must leave the pointer alone while it is already on the focused output"
    );
    assert!(
        pointer_moves_on_same_output_focus(MouseWarping::Container),
        "`container` must warp to the focused container on a same-output focus change"
    );
    assert!(
        !pointer_moves_on_same_output_focus(MouseWarping::No),
        "`none` must never warp"
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
    assert!(outcome.success, "{outcome:?}");
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == window_id)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        "right"
    );
}

#[test]
fn move_split_container_to_output_preserves_the_subtree() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
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
        if app_id == "second" {
            assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move container to output right");
    assert!(outcome[0].success, "{outcome:?}");
    let counts = f.swayward().layout.windows().fold(
        std::collections::HashMap::<_, usize>::new(),
        |mut counts, (monitor, _)| {
            *counts
                .entry(monitor.unwrap().output_name().clone())
                .or_default() += 1;
            counts
        },
    );
    assert_eq!(counts.get("right"), Some(&2));
    assert_eq!(counts.get("left"), Some(&1));
}

#[test]
fn move_split_container_direction_crosses_output_as_a_subtree() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
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
        if app_id == "second" {
            assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move right");
    assert!(outcome[0].success, "{outcome:?}");
    let counts = f.swayward().layout.windows().fold(
        std::collections::HashMap::<_, usize>::new(),
        |mut counts, (monitor, _)| {
            *counts
                .entry(monitor.unwrap().output_name().clone())
                .or_default() += 1;
            counts
        },
    );
    assert_eq!(counts.get("right"), Some(&2));
    assert_eq!(counts.get("left"), Some(&1));
}

#[test]
fn unscoped_move_output_wraps_from_the_edge() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(
        crate::command::execute(f.niri_state(), "focus output right, workspace right-ws")[0]
            .success
    );
    assert!(
        crate::command::execute(f.niri_state(), "focus output left, workspace left-ws")[0].success
    );
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    for expected in ["right", "left"] {
        assert!(
            crate::command::execute(f.niri_state(), "move container to output right")[0].success
        );
        assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .next()
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected
        );
    }
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
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move output current")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move output right")[0].success);
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
    let closed = f
        .client(client)
        .state
        .windows
        .iter()
        .filter(|window| window.close_requested)
        .map(|window| window.surface.clone())
        .collect::<Vec<_>>();
    for surface in closed {
        let window = f.client(client).window(&surface);
        window.attach_null();
        window.commit();
    }
    f.double_roundtrip(client);
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
    let window = f.client(client).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

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
    let mut tree_workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut tree_workspaces);
    let names = tree_workspaces
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["__i3_scratch", "active"]);

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
    assert_eq!(workspaces.len(), 1);
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
        [("7: numbered", 7)]
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

fn set_test_window_urgent_at(f: &mut Fixture, app_id: &str, now: Duration) {
    f.swayward().layout.with_windows_mut(|window, _| {
        if crate::utils::with_toplevel_role(window.toplevel(), |role| {
            role.app_id.as_deref() == Some(app_id)
        }) {
            window.set_urgent_for_test(true, now);
        }
    });
}

fn set_test_window_urgent(f: &mut Fixture, app_id: &str) {
    set_test_window_urgent_at(f, app_id, crate::utils::get_monotonic_time());
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
fn xdg_activation_policy_matches_sway_for_tokens_and_visibility() {
    use swayward_config::FocusOnWindowActivation::{Focus, None, Smart, Urgent};

    for (policy, focused_token, visible, expect_focus, expect_urgent) in [
        (Smart, true, true, true, false),
        (Smart, true, false, false, true),
        (Urgent, true, true, false, true),
        (Focus, true, false, true, false),
        (None, true, true, false, false),
        (Smart, false, true, false, true),
        (Urgent, false, true, false, true),
        (Focus, false, true, false, true),
        (None, false, true, false, false),
    ] {
        let config = swayward_config::Config {
            focus_on_window_activation: policy,
            ..Default::default()
        };
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        map_test_window(&mut f, client, "target");
        let target = f.swayward().layout.focus().unwrap().id();
        let target_surface = f.client(client).state.windows[0].surface.clone();
        if visible {
            map_test_window(&mut f, client, "other");
        } else {
            assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
            map_test_window(&mut f, client, "other");
        }
        let requesting_surface = focused_token.then_some(target_surface.clone());
        let token = f
            .client(client)
            .request_activation_token_with_focus(requesting_surface.as_ref());
        f.double_roundtrip(client);
        let token = token.lock().unwrap().take().unwrap();
        f.client(client).activate(token, &target_surface);
        f.double_roundtrip(client);

        assert_eq!(
            f.swayward().layout.focus().map(|w| w.id()) == Some(target),
            expect_focus,
            "policy={policy:?} focused_token={focused_token} visible={visible}"
        );
        assert_eq!(
            test_window_is_urgent(&mut f, "target"),
            expect_urgent,
            "policy={policy:?} focused_token={focused_token} visible={visible}"
        );
    }

    // Focus mode reveals a hidden scratchpad target before focusing it.
    let config = swayward_config::Config {
        focus_on_window_activation: Focus,
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    let target = f.swayward().layout.focus().unwrap().id();
    let target_window = f.swayward().layout.focus().unwrap().window.clone();
    let surface = f.client(client).state.windows[0].surface.clone();
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let token = f
        .client(client)
        .request_activation_token_with_focus(Some(&surface));
    f.double_roundtrip(client);
    let token = token.lock().unwrap().take().unwrap();
    f.client(client).activate(token, &surface);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().layout.focus().map(|w| w.id()), Some(target));
    assert!(!f.swayward().layout.is_scratchpad_hidden(&target_window));
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
    set_test_window_urgent_at(&mut f, "oldest", Duration::from_millis(1));
    set_test_window_urgent_at(&mut f, "latest", Duration::from_millis(2));

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
        urgent_timeout_ms: 60_000,
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
    let target_id = f
        .swayward()
        .layout
        .windows()
        .find_map(|(_, window)| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| {
                (role.app_id.as_deref() == Some("target")).then_some(window.id())
            })
        })
        .unwrap();
    let first_timer = *f.swayward().urgency_timers.get(&target_id).unwrap();
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert_eq!(
        f.swayward().urgency_timers.get(&target_id),
        Some(&first_timer)
    );
    assert!(f.swayward().fire_urgency_timer_for_test(target_id));
    assert!(!test_window_is_urgent(&mut f, "target"));
}

#[test]
fn closing_a_window_cancels_its_pending_urgency_timer() {
    let config = swayward_config::Config {
        urgent_timeout_ms: 60_000,
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
fn move_workspace_focuses_the_moved_window_in_the_destination_reply() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "destination-window");
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "moved-window");

    let mut stream = UnixStream::connect(socket).unwrap();
    assert_eq!(
        query_ipc_with_payload(
            &mut f,
            &mut stream,
            MessageType::RunCommand,
            "move workspace 1",
        ),
        serde_json::json!([{"success": true}])
    );
    assert_eq!(
        query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, "workspace 1",),
        serde_json::json!([{"success": true}])
    );
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);

    assert_eq!(
        find_json_node_with_app_id(&tree, "moved-window").unwrap()["focused"],
        true
    );
    assert_eq!(
        find_json_node_with_app_id(&tree, "destination-window").unwrap()["focused"],
        false
    );
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

/// Sway accepts a LIST of outputs and uses the first that resolves
/// (`sway/sway/commands/workspace.c:153-155`;
/// `sway/sway/tree/workspace.c:244-250`). The list must not be joined into one
/// name, which is what a single-output parser would do.
#[test]
fn workspace_output_assignment_uses_the_first_available_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let present = f.niri_output(2).name();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    // First two names do not resolve, so the third wins.
    let command = format!("workspace 7 output missing-a missing-b {present}");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    let swayward = f.swayward();
    let workspace = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.output, present);
}

/// A list naming only absent outputs resolves to nothing and must fail rather
/// than silently doing nothing.
#[test]
fn workspace_output_assignment_fails_when_no_output_resolves() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);

    let result =
        &crate::command::execute(f.niri_state(), "workspace 7 output missing-a missing-b")[0];
    assert!(!result.success);
}

#[test]
fn ipc_output_reports_sways_clockwise_transform_names() {
    for (transform, expected) in [
        (smithay::utils::Transform::Normal, "normal"),
        (smithay::utils::Transform::_90, "270"),
        (smithay::utils::Transform::_180, "180"),
        (smithay::utils::Transform::_270, "90"),
        (smithay::utils::Transform::Flipped, "flipped"),
        (smithay::utils::Transform::Flipped90, "flipped-270"),
        (smithay::utils::Transform::Flipped180, "flipped-180"),
        (smithay::utils::Transform::Flipped270, "flipped-90"),
    ] {
        assert_eq!(crate::ipc::tree::sway_transform(transform), expected);
    }
}

#[test]
fn get_outputs_reads_the_live_transform_and_subpixel_layout() {
    let (mut f, socket) = ipc_fixture();
    let output = smithay::output::Output::new(
        "test-output".into(),
        smithay::output::PhysicalProperties {
            size: (0, 0).into(),
            subpixel: smithay::output::Subpixel::HorizontalRgb,
            make: "test".into(),
            model: "test".into(),
            serial_number: "test".into(),
        },
    );
    let mode = smithay::output::Mode {
        size: (1280, 720).into(),
        refresh: 60_000,
    };
    output.change_current_state(Some(mode), None, None, None);
    output.set_preferred(mode);
    output.user_data().insert_if_missing(|| OutputName {
        connector: "test-output".into(),
        make: Some("test".into()),
        model: Some("test".into()),
        serial: Some("test".into()),
    });
    f.swayward().add_output(output.clone(), None, false);
    output.change_current_state(None, Some(smithay::utils::Transform::_90), None, None);
    f.niri_state().ipc_refresh_layout();

    let mut stream = UnixStream::connect(socket).unwrap();
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(outputs[0]["transform"], "270");
    assert_eq!(outputs[0]["subpixel_hinting"], "rgb");
}

#[test]
fn get_tree_ids_are_unique_across_workspaces() {
    fn collect_ids(node: &swayward_ipc::Node, ids: &mut Vec<i64>) {
        ids.push(node.id);
        for child in node.nodes.iter().chain(&node.floating_nodes) {
            collect_ids(child, ids);
        }
    }

    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for index in 0..3 {
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
            if index == 1 {
                assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
            }
        }
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut ids = Vec::new();
    collect_ids(&tree, &mut ids);
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

#[test]
fn ipc_output_reports_sways_subpixel_names() {
    for (subpixel, expected) in [
        (smithay::output::Subpixel::Unknown, "unknown"),
        (smithay::output::Subpixel::None, "none"),
        (smithay::output::Subpixel::HorizontalRgb, "rgb"),
        (smithay::output::Subpixel::HorizontalBgr, "bgr"),
        (smithay::output::Subpixel::VerticalRgb, "vrgb"),
        (smithay::output::Subpixel::VerticalBgr, "vbgr"),
    ] {
        assert_eq!(crate::ipc::tree::sway_subpixel_hinting(subpixel), expected);
    }
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
    assert_eq!(
        root.nodes[1].percent,
        Some((1280 * 720) as f64 / (3200 * 1080) as f64)
    );
    assert_eq!(root.nodes[2].percent, Some(1920. / 3200.));
}

#[test]
fn stale_tree_leaf_is_omitted_without_panicking() {
    let tree = IpcNode::Leaf {
        id: NodeId(1),
        window: (),
        percent: Some(1.),
        focused: false,
        fullscreen_mode: 0,
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
        title: None,
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
            fullscreen_mode: 0,
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
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let mut stream = UnixStream::connect(socket).unwrap();

    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(
        outputs
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["headless-1", "headless-2"]
    );
    assert_eq!(
        f.niri_output(2).current_mode().unwrap().size,
        (1920, 1080).into()
    );

    let removed = f.niri_output(2);
    f.swayward().remove_output(&removed);
    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(
        outputs
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["headless-1", "headless-3"]
    );
}

#[test]
fn create_output_rejects_unsupported_backends_without_changing_output_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let snapshot = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::json!({
            "outputs": describe_outputs(&swayward.layout, &swayward.global_space),
            "workspaces": describe_workspaces(&swayward.layout, &swayward.global_space),
        })
    };
    let before = snapshot(&mut f);

    let state = f.niri_state();
    let outcome = crate::command::create_output(None, &mut state.swayward);
    assert_eq!(
        outcome,
        swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Can only create outputs for Wayland, X11 or headless backends".into()),
            parse_error: None,
        }
    );
    assert_eq!(snapshot(&mut f), before);
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
fn focused_split_rejects_border_and_resizes_as_one_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1200, 800));
    let client = f.add_client();
    for app_id in ["outside", "upper", "lower"] {
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
    assert!(crate::command::execute(f.niri_state(), "move down")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let border = crate::command::execute(f.niri_state(), "border pixel 7");
    assert!(!border[0].success, "{border:?}");
    assert_eq!(
        border[0].error.as_deref(),
        Some("Only views can have borders")
    );

    let widths = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        ["outside", "upper", "lower"].map(|app_id| {
            find_json_node_with_app_id(&tree, app_id).unwrap()["rect"]["width"]
                .as_i64()
                .unwrap()
        })
    };
    let before = widths(&mut f);
    let resize = crate::command::execute(f.niri_state(), "resize grow width 10 ppt");
    assert!(resize[0].success, "{resize:?}");
    let after = widths(&mut f);
    assert!(after[0] < before[0], "before={before:?} after={after:?}");
    assert!(after[1] > before[1], "before={before:?} after={after:?}");
    assert!(after[2] > before[2], "before={before:?} after={after:?}");

    assert!(crate::command::execute(f.niri_state(), "mark group")[0].success);
    let resize =
        crate::command::execute(f.niri_state(), "[con_mark=group] resize shrink width 5 ppt");
    assert!(resize[0].success, "{resize:?}");
    let shrunk = widths(&mut f);
    assert!(shrunk[0] > after[0], "after={after:?} shrunk={shrunk:?}");
    assert!(shrunk[1] < after[1], "after={after:?} shrunk={shrunk:?}");
    assert!(shrunk[2] < after[2], "after={after:?} shrunk={shrunk:?}");
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
fn move_absolute_position_is_verbatim_under_a_bar_and_gaps() {
    // `move absolute position` must use the requested coordinate unchanged.
    // Sway applies the workspace origin only on the relative form
    // (`sway/sway/commands/move.c:913-916`) and then calls
    // container_floating_move_to, which performs no bounds check
    // (`sway/sway/tree/container.c:1127-1159`).
    //
    // swayward routed the coordinate through floating_pos, whose setter adds
    // working_area.loc back (`src/layout/floating.rs:120-129`), so a request
    // that already accounted for the bar gained the bar and the gaps a second
    // time. Only a preset with no slack revealed it: a full-workspace-height
    // window at the workspace origin overflowed the bottom edge, while shorter
    // presets absorbed the shift invisibly.
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.gaps = 4.;
    config.layout.outer_gaps_configured = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();

    // A top bar, so the workspace origin is not the output origin.
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

    let layer = f.client(client).create_layer(None, Layer::Top, "bar");
    layer.set_configure_props(crate::tests::client::LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 20)),
        exclusive_zone: Some(20),
        ..Default::default()
    });
    let layer_surface = layer.surface.clone();
    layer.commit();
    f.double_roundtrip(client);
    let layer = f.client(client).layer(&layer_surface);
    let size = layer.configures_received.last().unwrap().1.size;
    layer.attach_new_buffer();
    layer.set_size(size.0 as u16, size.1 as u16);
    layer.ack_last_and_commit();
    f.double_roundtrip(client);

    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    // find_json_node would return the hidden __i3 scratch workspace, whose
    // rect is all zeroes.
    fn visible<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
        if node["type"] == kind && node["name"] != "__i3" && node["name"] != "__i3_scratch" {
            return Some(node);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| node[key].as_array()?.iter().find_map(|c| visible(c, kind)))
    }

    let node = |f: &mut Fixture, kind: &str| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        visible(&tree, kind).unwrap()["rect"].clone()
    };

    let ws = node(&mut f, "workspace");
    let (ws_x, ws_y) = (ws["x"].as_i64().unwrap(), ws["y"].as_i64().unwrap());
    let (ws_w, ws_h) = (
        ws["width"].as_i64().unwrap(),
        ws["height"].as_i64().unwrap(),
    );
    assert_eq!(ws_y, 20, "workspace rect starts below the bar");

    // The tallCenter preset from a real script: full workspace height at the
    // workspace origin, computed from the IPC workspace rect. Zero slack, so
    // any displacement overflows.
    let command =
        format!("resize set {ws_w} px {ws_h} px, move absolute position {ws_x} px {ws_y} px");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    f.double_roundtrip(client);

    let rect = node(&mut f, "floating_con");
    assert_eq!(
        rect["y"], ws_y,
        "absolute y must be used verbatim, not offset by the bar and gaps"
    );
    assert_eq!(rect["x"], ws_x, "absolute x must be used verbatim");

    let bottom = rect["y"].as_i64().unwrap() + rect["height"].as_i64().unwrap();
    assert!(
        bottom <= ws_y + ws_h,
        "a full-height window overflowed the workspace: bottom {bottom} > {}",
        ws_y + ws_h
    );
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
fn global_fullscreen_blocks_directional_output_focus_but_workspace_fullscreen_does_not() {
    for (global, direction, source, expected) in [
        (false, "right", "left", "right"),
        (false, "left", "right", "left"),
        (true, "right", "left", "left"),
        (true, "left", "right", "right"),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {source}"))[0].success
        );
        let client = f.add_client();
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

        assert!(crate::command::execute(f.niri_state(), &format!("focus {direction}"))[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected,
            "global={global} direction={direction}"
        );
    }
}

#[test]
fn focus_output_reports_sway_errors() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let output = f.swayward().layout.active_output().unwrap().clone();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output missing"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("There is no output with that name.".into()),
            parse_error: Some(true),
        }]
    );
    assert_eq!(f.swayward().layout.active_output(), Some(&output));

    let mut f = Fixture::new();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output right"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No focused workspace to base directions off of.".into()),
            parse_error: None,
        }]
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
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move output right")[0].success);
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

#[test]
fn every_message_type_replies_and_leaves_the_connection_usable() {
    // AGENTS.md: every SWAYSOCK reply is sway-shaped or a structured failure,
    // and it never hangs. docs/IPC_ORACLE_COVERAGE.md recorded that this was
    // asserted but not enumerated, so nothing proved it for the numbers a
    // buggy or future client actually sends.
    //
    // Sway's own range is 0..=12 plus 100 and 101 (sway/include/ipc.h). 11 is
    // IPC_SYNC, which sway declines with {"success": false} rather than
    // closing the socket (sway/sway/ipc-server.c:919-925).
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();
    let mut stream = UnixStream::connect(&socket).unwrap();

    let types: Vec<u32> = (0..=13)
        .chain([99, 100, 101, 102, 1000, u32::MAX])
        .collect();
    for raw_type in types {
        // SUBSCRIBE needs a JSON array; anything else would be a parse failure
        // rather than a test of the type dispatch.
        let payload = if raw_type == 2 { "[]" } else { "" };
        stream
            .write_all(&swayward_ipc::wire::encode_raw(raw_type, payload))
            .unwrap();
        let (reply_type, reply) = read_ipc_reply(&mut fixture, &mut stream);

        assert_eq!(
            reply_type, raw_type,
            "reply must echo the request type for {raw_type}"
        );
        let value: Value = serde_json::from_str(&reply)
            .unwrap_or_else(|e| panic!("type {raw_type} returned invalid JSON: {reply}: {e}"));

        // Either a sway-shaped payload or a structured failure. Never a bare
        // string, never empty, never a silent drop.
        assert!(
            value.is_object() || value.is_array(),
            "type {raw_type} must reply with an object or array, got {reply}"
        );
        if raw_type == 11 {
            assert_eq!(
                value,
                serde_json::json!({"success": false}),
                "IPC_SYNC must match sway's decline exactly"
            );
        }
        if !matches!(raw_type, 0..=10 | 12 | 100 | 101) {
            assert_eq!(
                value["success"], false,
                "unsupported type {raw_type} must report failure, got {reply}"
            );
        }
    }

    // The connection survived every unsupported type and still serves a real
    // request. A client that gets one bad reply and a dead socket is worse off
    // than one that gets an error.
    let version = query_ipc(&mut fixture, &mut stream, MessageType::GetVersion);
    assert_eq!(version["variant"], "swayward");
}

fn nested_split_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for _ in 0..3 {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    fixture.swayward().layout.consume_or_expel_window_left(None);
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(fixture
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .focused_container_node()
        .is_some());
    fixture
}

fn command_tree(fixture: &mut Fixture) -> Value {
    let swayward = fixture.swayward();
    serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap()
}

#[test]
fn floating_group_commands_fail_without_changing_state() {
    // Disabling floating on an already tiled split is an exact no-op and does
    // not need the missing model.
    let mut fixture = nested_split_fixture();
    let before = command_tree(&mut fixture);
    let outcome = crate::command::execute(fixture.niri_state(), "floating disable");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(command_tree(&mut fixture), before);

    // Sway applies these commands to the selected split container, and
    // floating/move-scratchpad create or preserve one floating subtree
    // (`commands/floating.c:23-55`, `commands/move.c:925-946`, and
    // `commands/sticky.c:20-42`). Swayward has no
    // floating-subtree representation, so it must refuse instead of reporting
    // success after acting on only the focused leaf.
    for command in [
        "floating enable",
        "floating toggle",
        "move scratchpad",
        "sticky enable",
    ] {
        let mut fixture = nested_split_fixture();
        let before_tree = command_tree(&mut fixture);
        let before_scratchpad = fixture.swayward().layout.scratchpad_windows().count();

        let outcomes = crate::command::execute(fixture.niri_state(), command);

        assert_eq!(outcomes.len(), 1, "{command}: {outcomes:?}");
        assert!(!outcomes[0].success, "{command}: {outcomes:?}");
        assert_eq!(
            outcomes[0].error.as_deref(),
            Some("floating container groups are not supported"),
            "{command}"
        );
        assert_eq!(
            command_tree(&mut fixture),
            before_tree,
            "{command} changed tree geometry, floating state or sticky state"
        );
        assert_eq!(
            fixture.swayward().layout.scratchpad_windows().count(),
            before_scratchpad,
            "{command} changed scratchpad membership"
        );
    }
}

#[test]
fn criteria_targeted_floating_group_commands_fail_without_changing_state() {
    for command in [
        "floating enable",
        "floating toggle",
        "move scratchpad",
        "sticky enable",
    ] {
        let mut fixture = nested_split_fixture();
        assert!(crate::command::execute(fixture.niri_state(), "mark floating-group")[0].success);
        assert!(crate::command::execute(fixture.niri_state(), "focus child")[0].success);
        let before_tree = command_tree(&mut fixture);
        let before_scratchpad = fixture.swayward().layout.scratchpad_windows().count();
        let command = format!(r#"[con_mark="floating-group"] {command}"#);

        let outcomes = crate::command::execute(fixture.niri_state(), &command);

        assert_eq!(outcomes.len(), 1, "{command}: {outcomes:?}");
        assert!(!outcomes[0].success, "{command}: {outcomes:?}");
        assert_eq!(
            outcomes[0].error.as_deref(),
            Some("floating container groups are not supported"),
            "{command}"
        );
        assert_eq!(
            command_tree(&mut fixture),
            before_tree,
            "{command} changed tree geometry, floating state, sticky state or focus"
        );
        assert_eq!(
            fixture.swayward().layout.scratchpad_windows().count(),
            before_scratchpad,
            "{command} changed scratchpad membership"
        );
    }
}

#[test]
fn floating_accepts_sways_boolean_vocabulary() {
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

    for (value, expected) in [
        ("1", true),
        ("yes", true),
        ("on", true),
        ("true", true),
        ("enable", true),
        ("enabled", true),
        ("active", true),
        ("no", false),
        ("off", false),
        ("false", false),
        ("disable", false),
        ("disabled", false),
        ("inactive", false),
        ("arbitrary", false),
    ] {
        let command = format!("floating {value}");
        let outcome = crate::command::execute(f.niri_state(), &command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let window = f.swayward().layout.focus().unwrap().window.clone();
        assert_eq!(
            f.swayward()
                .layout
                .workspaces()
                .any(|(_, _, workspace)| workspace.is_floating(&window)),
            expected,
            "{command}"
        );
    }
    for expected in [true, false] {
        let outcome = crate::command::execute(f.niri_state(), "floating toggle");
        assert!(outcome[0].success, "{outcome:?}");
        let window = f.swayward().layout.focus().unwrap().window.clone();
        assert_eq!(
            f.swayward()
                .layout
                .workspaces()
                .any(|(_, _, workspace)| workspace.is_floating(&window)),
            expected
        );
    }

    for (command, expected) in [
        (
            "floating",
            "Invalid floating command (expected 1 argument, got 0)",
        ),
        (
            "floating yes now",
            "Invalid floating command (expected 1 argument, got 2)",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true));
    }
}

#[test]
fn floating_sizes_accept_i32_values_and_reject_malformed_values() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));

    for (command, minimum, expected) in [
        ("floating_minimum_size -10 x +20", true, (-10, 20)),
        (
            "floating_maximum_size 2147483647 x -2147483648",
            false,
            (i32::MAX, i32::MIN),
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let layout = &f.swayward().config.borrow().layout;
        let size = if minimum {
            layout.floating_minimum_size
        } else {
            layout.floating_maximum_size
        };
        assert_eq!((size.width, size.height), expected, "{command}");
    }

    for (command, expected) in [
        (
            "floating_minimum_size 10px x 20",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 2147483648 x 20",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 10 by 20",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 10 x 20 extra",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size wide x 20",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size 10 x -2147483649",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size 10 X 20",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size 10 x",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
    ] {
        let before = f.swayward().config.borrow().layout.clone();
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
        assert_eq!(f.swayward().config.borrow().layout, before, "{command}");
    }
}

#[test]
fn layout_settings_apply_at_runtime_like_sway() {
    use swayward_config::layout::{
        DefaultOrientation, FocusWrapping, HideEdgeBorders, SmartBorders, WorkspaceLayout,
    };

    // Sway serves its config file and its IPC from one command table
    // (`sway/sway/commands.c:162-173`), so these directives are live commands
    // there. swayward keeps the setting in KDL; this asserts the command
    // reaches the same state, rather than merely returning success.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let layout = |f: &mut Fixture| f.swayward().config.borrow().layout.clone();
    let before = layout(&mut f);
    assert_ne!(before.workspace_layout, WorkspaceLayout::Tabbed);

    for (command, check) in [
        (
            "workspace_layout tabbed",
            &(|l: &swayward_config::Layout| l.workspace_layout == WorkspaceLayout::Tabbed)
                as &dyn Fn(&swayward_config::Layout) -> bool,
        ),
        ("focus_wrapping no", &|l| {
            l.focus_wrapping == FocusWrapping::No
        }),
        ("focus_wrapping force", &|l| {
            l.focus_wrapping == FocusWrapping::Force
        }),
        // Deprecated in sway, kept as a boolean alias selecting between
        // force and yes (`sway/sway/commands/force_focus_wrapping.c`).
        ("force_focus_wrapping no", &|l| {
            l.focus_wrapping == FocusWrapping::Yes
        }),
        ("force_focus_wrapping yes", &|l| {
            l.focus_wrapping == FocusWrapping::Force
        }),
        ("hide_edge_borders both", &|l| {
            l.hide_edge_borders == HideEdgeBorders::Both
        }),
        // sway folds smart and smart_no_gaps into the smart-border toggle
        // rather than treating them as edge-border values.
        ("hide_edge_borders smart", &|l| {
            l.smart_borders == SmartBorders::On
        }),
        ("smart_borders no_gaps", &|l| {
            l.smart_borders == SmartBorders::NoGaps
        }),
        ("default_orientation vertical", &|l| {
            l.default_orientation == DefaultOrientation::Vertical
        }),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command} failed: {outcome:?}");
        assert!(
            check(&layout(&mut f)),
            "{command} did not change the config"
        );
    }

    // Values sway rejects must fail here too, with sway's message.
    for (command, expected) in [
        (
            "workspace_layout sideways",
            "Expected 'workspace_layout <default|stacking|tabbed>'",
        ),
        (
            "focus_follows_mouse maybe",
            "Expected 'focus_follows_mouse no|yes|always'",
        ),
        (
            "default_orientation diagonal",
            "Expected 'orientation <horizontal|vertical|auto>'",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
    }

    assert!(crate::command::execute(f.niri_state(), "show_marks no")[0].success);
    assert!(!layout(&mut f).titlebar.show_marks);
    assert!(crate::command::execute(f.niri_state(), "title_align right")[0].success);
    assert_eq!(
        layout(&mut f).titlebar.alignment,
        swayward_config::TitleAlignment::Right
    );
    assert!(crate::command::execute(f.niri_state(), "smart_gaps inverse_outer")[0].success);
    assert_eq!(
        layout(&mut f).smart_gaps,
        swayward_config::SmartGaps::InverseOuter
    );
    assert!(crate::command::execute(f.niri_state(), "smart_gaps toggle")[0].success);
    assert_eq!(layout(&mut f).smart_gaps, swayward_config::SmartGaps::Off);

    assert!(crate::command::execute(f.niri_state(), "tiling_drag no")[0].success);
    assert!(!f.swayward().config.borrow().input.tiling_drag);
    assert!(crate::command::execute(f.niri_state(), "tiling_drag toggle")[0].success);
    assert!(f.swayward().config.borrow().input.tiling_drag);
    assert!(crate::command::execute(f.niri_state(), "tiling_drag_threshold 17")[0].success);
    assert_eq!(f.swayward().config.borrow().input.tiling_drag_threshold, 17);
    assert!(crate::command::execute(f.niri_state(), "force_display_urgency_hint 700ms")[0].success);
    assert_eq!(f.swayward().config.borrow().urgent_timeout_ms, 700);

    // The primary-selection manager is created at launch. Reasserting its
    // current value succeeds; changing it must fail instead of claiming a
    // live change that cannot affect the manager (`sway/server.c:783-785`).
    assert!(crate::command::execute(f.niri_state(), "primary_selection enabled")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "primary_selection disabled");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("primary_selection can only be enabled/disabled at launch")
    );
    assert!(!f.swayward().config.borrow().clipboard.disable_primary);

    assert!(crate::command::execute(f.niri_state(), "focus_on_window_activation none")[0].success);
    assert_eq!(
        f.swayward().config.borrow().focus_on_window_activation,
        swayward_config::FocusOnWindowActivation::None
    );

    // focus_follows_mouse and workspace_auto_back_and_forth live under input.
    // Sway keeps three distinct states, and `always` is not `yes`
    // (`sway/include/sway/config.h:458-462`), so assert the stored mode and
    // not merely that the setting is enabled.
    use swayward_config::input::FocusFollowsMouseMode;
    let ffm = |f: &mut Fixture| f.swayward().config.borrow().input.focus_follows_mouse;
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse yes")[0].success);
    assert_eq!(
        ffm(&mut f).map(|v| v.mode),
        Some(FocusFollowsMouseMode::Yes)
    );
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse always")[0].success);
    assert_eq!(
        ffm(&mut f).map(|v| v.mode),
        Some(FocusFollowsMouseMode::Always),
        "`always` must be stored distinctly from `yes`"
    );
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse no")[0].success);
    assert_eq!(
        ffm(&mut f),
        None,
        "sway's FOLLOWS_NO is the field's absence"
    );
    // Sway compares these three with strcmp, so they are case-sensitive
    // (`sway/sway/commands/focus_follows_mouse.c:9-18`).
    let outcome = crate::command::execute(f.niri_state(), "focus_follows_mouse ALWAYS");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'focus_follows_mouse no|yes|always'")
    );

    assert!(
        crate::command::execute(f.niri_state(), "workspace_auto_back_and_forth yes")[0].success
    );
    assert!(
        f.swayward()
            .config
            .borrow()
            .input
            .workspace_auto_back_and_forth
    );

    // sway parses these with strtol and requires a literal x between two
    // integers, rejecting a trailing suffix because it checks the remainder
    // (`sway/sway/commands/floating_minmax_size.c`).
    assert!(crate::command::execute(f.niri_state(), "floating_minimum_size 100 x 50")[0].success);
    assert_eq!(layout(&mut f).floating_minimum_size.width, 100);
    assert_eq!(layout(&mut f).floating_minimum_size.height, 50);
    assert!(crate::command::execute(f.niri_state(), "floating_maximum_size 800 x 600")[0].success);
    assert_eq!(layout(&mut f).floating_maximum_size.width, 800);
    assert_eq!(layout(&mut f).floating_maximum_size.height, 600);
    for bad in [
        "floating_minimum_size 100 50",
        "floating_minimum_size 100 x 50px",
        "floating_minimum_size 100 by 50",
    ] {
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Expected 'floating_minimum_size <width> x <height>'")
        );
    }

    // `sway/sway/commands/font.c` strips a leading pango: prefix and joins the
    // remaining words, so the family and size survive as one string.
    assert!(crate::command::execute(f.niri_state(), "font pango:monospace 11")[0].success);
    assert_eq!(layout(&mut f).titlebar.font, "monospace 11");
    assert!(layout(&mut f).titlebar.pango_markup);
    assert!(f.swayward().layout.options().layout.titlebar.pango_markup);
    assert!(crate::command::execute(f.niri_state(), "font Sans Bold 9")[0].success);
    assert_eq!(layout(&mut f).titlebar.font, "Sans Bold 9");
    assert!(!layout(&mut f).titlebar.pango_markup);
    assert!(!f.swayward().layout.options().layout.titlebar.pango_markup);
    for (command, expected) in [
        ("font 11", "Invalid font family."),
        ("font monospace", "Invalid font size."),
    ] {
        let before = layout(&mut f).titlebar.clone();
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
        assert_eq!(layout(&mut f).titlebar, before);
    }

    // One value sets both axes; two set horizontal then vertical. Sway requires
    // both padding axes to be at least the current border thickness, and a new
    // thickness may not exceed the current vertical padding.
    assert!(crate::command::execute(f.niri_state(), "titlebar_padding 7")[0].success);
    assert_eq!(layout(&mut f).titlebar.horizontal_padding, 7.);
    assert_eq!(layout(&mut f).titlebar.vertical_padding, 7.);
    assert!(crate::command::execute(f.niri_state(), "titlebar_border_thickness 6")[0].success);
    assert_eq!(layout(&mut f).titlebar.border_thickness, 6);
    for bad in [
        "titlebar_border_thickness 8",
        "titlebar_border_thickness -1",
        "titlebar_border_thickness wide",
        "titlebar_padding 5 7",
        "titlebar_padding 7 5",
        "titlebar_padding -1",
        "titlebar_padding wide",
    ] {
        let before = layout(&mut f).titlebar.clone();
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some("Invalid size specified"));
        assert_eq!(layout(&mut f).titlebar, before);
    }
    assert!(crate::command::execute(f.niri_state(), "titlebar_padding 9 6")[0].success);
    assert_eq!(layout(&mut f).titlebar.horizontal_padding, 9.);
    assert_eq!(layout(&mut f).titlebar.vertical_padding, 6.);

    // focused_tab_title has no effective indicator or child-border colours in
    // sway, so the titlebar ring makes that class complete. The other classes
    // still use those colours on window borders and remain fail-loud.
    assert!(
        crate::command::execute(
            f.niri_state(),
            "client.focused_tab_title #123456 #abcdef #fedcba #010203 #040506"
        )[0]
        .success
    );
    let focused_tab = layout(&mut f).titlebar.focused_tab_title;
    assert_eq!(
        focused_tab.border_color.to_array_unpremul(),
        [
            0x12 as f32 / 255.,
            0x34 as f32 / 255.,
            0x56 as f32 / 255.,
            1.
        ]
    );
    assert_eq!(
        focused_tab.background_color.to_array_unpremul(),
        [
            0xab as f32 / 255.,
            0xcd as f32 / 255.,
            0xef as f32 / 255.,
            1.
        ]
    );
    assert_eq!(
        focused_tab.text_color.to_array_unpremul(),
        [
            0xfe as f32 / 255.,
            0xdc as f32 / 255.,
            0xba as f32 / 255.,
            1.
        ]
    );

    let before = layout(&mut f).titlebar;
    for command in [
        "client.focused #102030 #405060 #708090",
        "client.focused_inactive #112233 #445566 #778899",
        "client.unfocused #203040 #506070 #8090a0",
        "client.urgent #304050 #607080 #90a0b0",
        "client.focused #010203 #11223380 #44556640 #778899 #aabbcc",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].parse_error, Some(true));
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("client colour commands are unsupported because sway window-border colours are not fully rendered")
        );
        assert_eq!(layout(&mut f).titlebar, before);
        assert_eq!(f.swayward().layout.options().layout.titlebar, before);
    }

    let before = layout(&mut f).titlebar.focused;
    for (command, expected) in [
        (
            "client.focused #000000 #111111",
            "Invalid client.focused command (expected at least 3 arguments, got 2)",
        ),
        (
            "client.focused #000000 #111111 #222222 #333333 #444444 #555555",
            "Invalid client.focused command (expected at most 5 arguments, got 6)",
        ),
        (
            "client.focused #000000 #111111 #222222 nope",
            "Invalid indicator color nope",
        ),
        (
            "client.focused #000000 #111111 #222222 #333333 nope",
            "Invalid child_border color nope",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
        assert_eq!(layout(&mut f).titlebar.focused, before);
    }

    // Xwayland's mode is fixed at launch. Sway accepts the command and
    // refuses only a change (`sway/sway/commands/xwayland.c:24-28`), so
    // asking for the value already in effect succeeds and flipping it fails
    // with sway's message. The default config enables it.
    // default_border governs the border a new tiled window gets; sway keeps
    // the previous width when the command omits one
    // (`sway/sway/commands/default_border.c:22-24`). new_window and new_float
    // are the older i3 spellings of the same two settings.
    use swayward_config::layout::SwayBorderStyle;
    assert!(crate::command::execute(f.niri_state(), "default_border pixel 3")[0].success);
    assert_eq!(layout(&mut f).default_border.style, SwayBorderStyle::Pixel);
    assert_eq!(layout(&mut f).default_border.width, Some(3));
    assert!(crate::command::execute(f.niri_state(), "default_border normal")[0].success);
    assert_eq!(layout(&mut f).default_border.style, SwayBorderStyle::Normal);
    assert_eq!(
        layout(&mut f).default_border.width,
        Some(3),
        "omitting the width must keep the previous one"
    );
    assert!(crate::command::execute(f.niri_state(), "new_float none")[0].success);
    assert_eq!(
        layout(&mut f).default_floating_border.style,
        SwayBorderStyle::None,
        "new_float is the deprecated spelling of default_floating_border"
    );
    for bad in ["default_border csd", "default_border pixel wide"] {
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Expected 'default_border <none|normal|pixel>' or 'default_border <normal|pixel> <px>'")
        );
    }

    // popup_during_fullscreen shares its accepted values and error string
    // with the KDL node, so IPC and the config file cannot drift.
    assert!(crate::command::execute(f.niri_state(), "popup_during_fullscreen ignore")[0].success);
    assert_eq!(
        f.swayward().config.borrow().popup_during_fullscreen,
        swayward_config::misc::PopupDuringFullscreen::Ignore
    );
    let outcome = crate::command::execute(f.niri_state(), "popup_during_fullscreen sometimes");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'popup_during_fullscreen smart|ignore|leave_fullscreen'")
    );

    // Sway spells the modifier Mod1..Mod5; swayward names them. The modifier
    // and the inverse bit are independent fields, and this is its own setting
    // rather than the compositor `mod_key`, which must not move
    // (`sway/include/sway/config.h:509-510`).
    use swayward_config::input::{FloatingModifier, ModKey};
    let floating = |f: &mut Fixture| f.swayward().config.borrow().input.floating_modifier;
    let mod_key_before = f.swayward().config.borrow().input.mod_key;
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: false
        })
    );
    assert_eq!(
        f.swayward().config.borrow().input.mod_key,
        mod_key_before,
        "floating_modifier must not move the compositor mod key"
    );
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Alt normal")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Alt,
            inverse: false
        })
    );
    // inverse is stored, not refused: it swaps the move and resize buttons.
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: true
        })
    );
    // `none` is a value, not a key name, and sway returns before reading the
    // second argument (`sway/sway/commands/floating_modifier.c:11-14`), so a
    // trailing word is ignored and the inverse bit resets.
    for command in ["floating_modifier none", "floating_modifier NONE inverse"] {
        assert!(
            crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success
        );
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command} failed"
        );
        assert_eq!(
            floating(&mut f),
            Some(FloatingModifier {
                modifier: ModKey::None,
                inverse: false
            }),
            "{command} must disable the drag rather than name a key"
        );
    }
    // Sway validates the modifier before the mode, so an invalid modifier
    // wins over an invalid trailing word.
    for (command, expected) in [
        ("floating_modifier Mod9", "Invalid modifier"),
        ("floating_modifier Mod9 sideways", "Invalid modifier"),
        (
            "floating_modifier Mod4 sideways",
            "Usage: floating_modifier <mod> [inverse|normal]",
        ),
        (
            "floating_modifier",
            "Invalid floating_modifier command (expected at least 1 argument, got 0)",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
    }

    // Sway keeps the three warping modes apart: `output` warps only across
    // outputs, `container` warps on every qualifying focus change
    // (`sway/sway/input/seat.c:1526-1547`). This is its own policy and must
    // not be folded into the inherited `warp-mouse-to-focus` centering mode.
    use swayward_config::input::MouseWarping;
    let warping = |f: &mut Fixture| f.swayward().config.borrow().input.mouse_warping;
    let warp_to_focus_before = f.swayward().config.borrow().input.warp_mouse_to_focus;
    assert!(crate::command::execute(f.niri_state(), "mouse_warping output")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Output);
    assert!(crate::command::execute(f.niri_state(), "mouse_warping container")[0].success);
    assert_eq!(
        warping(&mut f),
        MouseWarping::Container,
        "`container` must be stored distinctly from `output`"
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::No);
    assert_eq!(
        f.swayward().config.borrow().input.warp_mouse_to_focus,
        warp_to_focus_before,
        "mouse_warping must not overwrite the inherited centering option"
    );
    // strcasecmp, unlike focus_follows_mouse
    // (`sway/sway/commands/mouse_warping.c:9-16`).
    assert!(crate::command::execute(f.niri_state(), "mouse_warping CONTAINER")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Container);
    let outcome = crate::command::execute(f.niri_state(), "mouse_warping sideways");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'mouse_warping output|container|none'")
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);

    assert!(crate::command::execute(f.niri_state(), "xwayland enable")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "xwayland disable");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("xwayland can only be enabled/disabled at launch")
    );
}

/// A KDL `workspace "name" { layout { gaps N } }` must reach the workspace
/// however it comes to exist. Sway reads the workspace config inside
/// `workspace_create` (`sway/sway/tree/workspace.c:224-243`), so this holds for
/// a workspace created on demand, not only one created eagerly at startup.
///
/// Regression test: workspaces carrying an output assignment skip eager
/// creation (`src/swayward.rs:1549-1552`), and the lazy creation path used to
/// drop the per-name layout entirely.
#[test]
fn configured_workspace_layout_applies_however_the_workspace_is_created() {
    for assignment in ["", "sway-output-assignment \"fake-1\""] {
        let config = swayward_config::Config::parse_mem(&format!(
            r#"
layout {{
    gaps 10
    border {{ off; }}
}}
workspace "roomy" {{
    {assignment}
    layout {{ gaps 45; }}
}}
"#
        ))
        .unwrap();
        let mut fixture = Fixture::with_config(config);
        fixture.add_output(1, (1280, 800));
        assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
        add_two_tiled_windows(&mut fixture);
        assert_eq!(
            tiled_window_rects_on(&mut fixture, "roomy")[0]["x"],
            45,
            "assignment: {assignment:?}"
        );
    }
}

/// Workspace names in a stable order, for rename assertions.
fn workspace_names(fixture: &mut Fixture) -> Vec<String> {
    let swayward = fixture.swayward();
    let mut names = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .map(|workspace| workspace.name)
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Put one window with `app_id` on each named workspace, leaving the last
/// created workspace focused.
fn windows_on_workspaces(fixture: &mut Fixture, plan: &[(&str, &str)]) {
    let client = fixture.add_client();
    for (workspace, app_id) in plan {
        assert!(
            crate::command::execute(fixture.niri_state(), &format!("workspace {workspace}"))[0]
                .success
        );
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id((*app_id).into());
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}

/// Sway resolves `rename workspace to <new>` from the matched container's
/// workspace rather than from focus (`sway/sway/commands.c:181-202`;
/// `sway/sway/commands/rename.c:36-37`).
#[test]
fn criteria_rename_workspace_renames_the_matched_workspace_not_the_focused_one() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "target"), ("beta", "bystander")]);
    // Focus is on beta, but the criteria match is on alpha.
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("beta".to_owned())
    );

    let outcome = crate::command::execute(
        f.niri_state(),
        "[app_id=target] rename workspace to renamed",
    );
    assert!(outcome[0].success, "{outcome:?}");

    // alpha became renamed; beta, which had focus, is untouched.
    assert_eq!(
        workspace_names(&mut f),
        vec!["beta".to_owned(), "renamed".to_owned()]
    );
}

/// Sway runs the handler once per matched container
/// (`sway/sway/commands.c:305-323`), so two matches on ONE workspace rename it
/// once: the second pass finds the new name already taken by the same
/// workspace and returns success without renaming again
/// (`sway/sway/commands/rename.c:82-89`).
#[test]
fn criteria_rename_workspace_renames_one_workspace_once_for_two_matches() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "twin"), ("beta", "bystander")]);
    // Add a second matching window to alpha.
    assert!(crate::command::execute(f.niri_state(), "workspace alpha")[0].success);
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("twin".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "[app_id=twin] rename workspace to once");
    assert!(outcome[0].success, "{outcome:?}");

    // Renamed exactly once. A second rename would have failed with
    // "Workspace already exists" or produced a stray name.
    assert_eq!(
        workspace_names(&mut f),
        vec!["beta".to_owned(), "once".to_owned()]
    );
}

/// Two matches on DIFFERENT workspaces cannot both take one name. Sway renames
/// the first, then fails the second with `Workspace already exists`, and a
/// CMD_INVALID aborts the remaining targets (`sway/sway/commands.c:316-321`).
#[test]
fn criteria_rename_workspace_fails_when_two_matched_workspaces_want_one_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "spread"), ("beta", "spread")]);

    let outcome =
        crate::command::execute(f.niri_state(), "[app_id=spread] rename workspace to clash");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Workspace already exists")
    );

    // The first match was renamed before the clash, as in sway: the loop is not
    // transactional.
    assert_eq!(
        workspace_names(&mut f),
        vec!["beta".to_owned(), "clash".to_owned()]
    );
}

/// Zero matches must be a structured failure, not a success no-op
/// (`sway/sway/commands.c:301-303`).
#[test]
fn criteria_rename_workspace_reports_no_matching_node_for_zero_matches() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "present")]);
    let before = workspace_names(&mut f);

    let outcome =
        crate::command::execute(f.niri_state(), "[app_id=absent] rename workspace to nope");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error.as_deref(), Some("No matching node."));
    assert_eq!(workspace_names(&mut f), before);
}

/// Sway resolves the `<old>` and `number <n>` forms by name even under a
/// criteria prefix; only the bare `to` form reads the matched container
/// (`sway/sway/commands/rename.c:35-58`).
#[test]
fn criteria_rename_workspace_with_an_explicit_old_name_ignores_the_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "target"), ("beta", "bystander")]);

    let outcome = crate::command::execute(
        f.niri_state(),
        "[app_id=target] rename workspace beta to moved",
    );
    assert!(outcome[0].success, "{outcome:?}");

    // beta was renamed, even though the match was on alpha.
    assert_eq!(
        workspace_names(&mut f),
        vec!["alpha".to_owned(), "moved".to_owned()]
    );
}

/// The point of runtime `set`: a variable defined over IPC must change what a
/// LATER command does, not merely be stored. Sway substitutes at dispatch
/// (`sway/sway/commands.c:283-285`), so this is observable behaviour.
#[test]
fn runtime_set_variable_changes_a_subsequent_command() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $target chosen")[0].success);
    // The variable is only useful if it expands in the NEXT command.
    assert!(crate::command::execute(f.niri_state(), "workspace $target")[0].success);

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("chosen".to_owned())
    );
}

/// Sway's symbol table is global, not per-connection, so a variable set on one
/// IPC connection is visible on the next command from any source.
#[test]
fn runtime_set_variable_survives_across_ipc_connections() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));

    let mut setter = UnixStream::connect(&socket).unwrap();
    setter
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "set $ws first",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut f, &mut setter);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([{"success": true}])
    );
    drop(setter);

    // A different socket sees the compositor-global symbol table.
    let mut user = UnixStream::connect(&socket).unwrap();
    user.write_all(&swayward_ipc::wire::encode(
        MessageType::RunCommand,
        "workspace $ws",
    ))
    .unwrap();
    let (_, payload) = read_ipc_reply(&mut f, &mut user);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([{"success": true}])
    );
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("first".to_owned())
    );

    // Redefining on that second socket is visible on its next request too.
    user.write_all(&swayward_ipc::wire::encode(
        MessageType::RunCommand,
        "set $ws second",
    ))
    .unwrap();
    let _ = read_ipc_reply(&mut f, &mut user);
    user.write_all(&swayward_ipc::wire::encode(
        MessageType::RunCommand,
        "workspace $ws",
    ))
    .unwrap();
    let _ = read_ipc_reply(&mut f, &mut user);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("second".to_owned())
    );
}

/// Sway sorts symbols longest name first on insert
/// (`sway/sway/commands/set.c:13-15`), so a longer name is never shadowed by a
/// shorter one that prefixes it.
#[test]
fn runtime_set_prefers_the_longest_matching_variable_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    // Define the SHORT name first, so insertion order alone would mismatch.
    assert!(crate::command::execute(f.niri_state(), "set $ws short")[0].success);
    assert!(crate::command::execute(f.niri_state(), "set $ws2 long")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $ws2")[0].success);

    // Wrong answer here would be "short2".
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("long".to_owned())
    );
}

/// Sway exempts the name being defined from substitution, starting at argv[2]
/// for `set` (`sway/sway/commands.c:283`), so `set $a $b` assigns the VALUE of
/// `$b` to `$a` rather than expanding `$a` on the left.
#[test]
fn runtime_set_expands_the_value_but_not_the_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $source resolved")[0].success);
    assert!(crate::command::execute(f.niri_state(), "set $alias $source")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $alias")[0].success);

    // $alias holds "resolved", and the name $alias was not itself expanded.
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("resolved".to_owned())
    );
}

/// An unknown variable is left verbatim rather than becoming empty
/// (`sway/sway/config.c:935-937`).
#[test]
fn runtime_set_leaves_an_unknown_variable_verbatim() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $known value")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $unknown")[0].success);

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("$unknown".to_owned())
    );
}

/// Sway rejects a name without `$` and a command with too few arguments
/// (`sway/sway/commands/set.c:27-34`).
#[test]
fn runtime_set_rejects_sways_invalid_forms() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let bare = &crate::command::execute(f.niri_state(), "set novar value")[0];
    assert!(!bare.success);
    assert_eq!(
        bare.error.as_deref(),
        Some("variable 'novar' must start with $")
    );

    let short = &crate::command::execute(f.niri_state(), "set $onlyname")[0];
    assert!(!short.success);
    assert_eq!(
        short.error.as_deref(),
        Some("Invalid set command (expected at least 2 arguments, got 1)")
    );

    // Neither rejection may leave a variable behind.
    assert!(f.swayward().sway_variables.is_empty());
}

/// A key binding re-enters the command path at press time
/// (`sway/sway/commands/bind.c:635`), so a variable set at runtime expands for
/// a binding whose stored command still contains it.
#[test]
fn runtime_set_variable_expands_for_a_binding_at_press_time() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds {
    Mod+Shift+V { command "workspace $late"; }
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $late arrived")[0].success);
    let bound = f.swayward().config.borrow().binds.0[0].action.clone();
    let swayward_config::Action::SwayCommand(command) = bound else {
        panic!("expected a sway command binding");
    };
    // The binding still holds the unexpanded text; expansion happens on run.
    assert_eq!(command, "workspace $late");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("arrived".to_owned())
    );
}

/// Sway frees the symbol table on reload (`sway/sway/config.c:111-115`), so a
/// runtime variable does not survive one.
#[test]
fn runtime_set_variables_are_discarded_by_reload() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $gone value")[0].success);
    assert_eq!(f.swayward().sway_variables.len(), 1);

    f.niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(f.swayward().sway_variables.is_empty());

    // And the name no longer expands, so it is left verbatim.
    assert!(crate::command::execute(f.niri_state(), "workspace $gone")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("$gone".to_owned())
    );
}

/// Sway substitutes after command-list and argv splitting, so separators and
/// whitespace inside a variable value remain one argument rather than becoming
/// syntax (`sway/sway/commands.c:253-285`).
#[test]
fn runtime_set_value_cannot_inject_another_command_or_split_an_argument() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $ws a b")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $ws")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("a b".to_owned())
    );

    assert!(crate::command::execute(f.niri_state(), "set $literal \"semi;colon\"")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "workspace $literal");
    assert_eq!(
        outcome.len(),
        1,
        "value became a second command: {outcome:?}"
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("semi;colon".to_owned())
    );
}

/// Sway executes an IPC command list in order and substitutes immediately
/// before each dispatch, so a `set` at the front affects a later command in the
/// same payload (`sway/sway/commands.c:230-334`).
#[test]
fn runtime_set_affects_a_later_command_in_the_same_payload() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let outcome = crate::command::execute(f.niri_state(), "set $ws inline; workspace $ws");
    assert_eq!(outcome.len(), 2);
    assert!(outcome.iter().all(|result| result.success), "{outcome:?}");
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("inline".to_owned())
    );
}

/// Criteria-targeted `set` is still refused rather than pretending that the
/// parser's global state matches sway's per-match sequential command loop.
#[test]
fn runtime_set_with_criteria_fails_without_changing_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("matched".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let before = f.swayward().sway_variables.clone();
    let outcome = crate::command::execute(f.niri_state(), "[app_id=matched] set $ws wrong");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("criteria targets are not implemented for this command yet")
    );
    assert_eq!(f.swayward().sway_variables, before);
}

/// Sway substitutes a bindsym line while loading the config and stores the
/// expanded command (`sway/sway/commands.c:403`; `sway/sway/commands/bind.c:488`),
/// so redefining the variable later cannot rewrite a binding that already
/// captured the old value.
#[test]
fn runtime_set_does_not_rewrite_a_binding_that_captured_the_old_value() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds {
    Mod+Shift+V { command "workspace old"; }
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $ws new")[0].success);
    let bound = f.swayward().config.borrow().binds.0[0].action.clone();
    let swayward_config::Action::SwayCommand(command) = bound else {
        panic!("expected a sway command binding");
    };
    assert_eq!(command, "workspace old");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("old".to_owned())
    );
}

#[test]
fn get_tree_hides_windows_on_inactive_tabs_at_every_depth() {
    // Sway's view_is_visible walks up from a view and, at every tabbed or
    // stacked ancestor, requires the seat's active tiling child to be on its
    // path (sway/tree/view.c:1180-1193). Measured on headless sway 1.12 with
    // tabbed[A, tabbed[B, tabbed[C, D]]]: exactly the focused window is
    // visible, whichever depth it sits at. swayward reported all four.
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let run = |fixture: &mut Fixture, command: &str| {
        assert!(
            crate::command::execute(fixture.niri_state(), command)[0].success,
            "{command}"
        );
    };

    map_test_window(&mut fixture, client, "m-A");
    run(&mut fixture, "layout tabbed");
    map_test_window(&mut fixture, client, "m-B");
    run(&mut fixture, "split v");
    run(&mut fixture, "layout tabbed");
    map_test_window(&mut fixture, client, "m-C");
    run(&mut fixture, "split v");
    run(&mut fixture, "layout tabbed");
    map_test_window(&mut fixture, client, "m-D");

    fn visible(node: &Value, out: &mut Vec<(String, bool)>) {
        if let Some(app) = node["app_id"].as_str().filter(|app| app.starts_with("m-")) {
            out.push((app.to_owned(), node["visible"] == true));
        }
        for key in ["nodes", "floating_nodes"] {
            for child in node[key].as_array().into_iter().flatten() {
                visible(child, out);
            }
        }
    }

    let mut stream = UnixStream::connect(&socket).unwrap();
    for focused in ["m-D", "m-C", "m-B", "m-A"] {
        run(&mut fixture, &format!("[app_id=\"{focused}\"] focus"));
        let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
        let mut found = Vec::new();
        visible(&tree, &mut found);
        found.sort();
        let expected: Vec<_> = ["m-A", "m-B", "m-C", "m-D"]
            .into_iter()
            .map(|app| (app.to_owned(), app == focused))
            .collect();
        assert_eq!(found, expected, "focused {focused}: only it is visible");
    }
}

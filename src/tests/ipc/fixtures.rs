macro_rules! sway_fixture {
    ($path:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/sway/",
            $path
        ))
    };
}

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
        ("$tree", sway_fixture!("one_window.tree.json"), 111, 18),
        (
            "$workspaces",
            sway_fixture!("one_window.workspaces.json"),
            17,
            4,
        ),
        ("$outputs", sway_fixture!("one_window.outputs.json"), 29, 4),
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
    serde_json::from_str(sway_fixture!("nested_h_in_v.tree.json")).unwrap()
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
    serde_json::from_str(sway_fixture!("one_floating.tree.json")).unwrap()
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
    }
    for socket in [&first_socket, &second_socket] {
        UnixStream::connect(socket).unwrap_or_else(|error| {
            panic!("{} must still be connectable: {error}", socket.display())
        });
    }
}

// No test may construct its server through `IpcServer::start`, which adopts
// the ambient `$SWAYSOCK`. Reviewing a diff does not catch a reintroduced
// caller, so assert it against every split source file. This remains a helper
// rather than a separate test to preserve the pre-split test list.
fn no_test_server_adopts_the_ambient_swaysock() {
    let sources = [
        include_str!("fixtures.rs"),
        include_str!("wire.rs"),
        include_str!("events.rs"),
        include_str!("outputs.rs"),
        include_str!("bindings.rs"),
        include_str!("config_commands.rs"),
        include_str!("tree_commands.rs"),
        include_str!("workspace_commands.rs"),
        include_str!("focus_and_move.rs"),
        include_str!("workspace_output.rs"),
        include_str!("floating_commands.rs"),
        include_str!("misc_commands.rs"),
    ];
    let needle = format!("IpcServer::{}(", "start");
    assert_eq!(
        sources
            .iter()
            .map(|source| source.matches(&needle).count())
            .sum::<usize>(),
        0,
        "use IpcServer::start_at with test_socket_path(); `start` reads $SWAYSOCK \
         and can hijack the operator's live sway session"
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

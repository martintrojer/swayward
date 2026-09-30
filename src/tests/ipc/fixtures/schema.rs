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
    // Some workspace representations depend on the scenario's window layout.
    // The oracle compares their semantics in dedicated tree scenarios.
    if path.ends_with(".representation") {
        return;
    }
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
    // `geometry` gets the same tolerance: the pinned capture's client
    // geometry also moved with font metrics (c2fa773c), and callers assert
    // their exact requested geometry separately.
    for rectangle in ["rect", "deco_rect", "window_rect", "geometry"] {
        for key in ["x", "y", "width", "height"] {
            let expected = expected[rectangle][key].as_i64().unwrap();
            let actual = actual[rectangle][key].as_i64().unwrap();
            assert!(
                (expected - actual).abs() <= 10,
                "{rectangle}.{key} at {path}: expected {expected}, got {actual}"
            );
        }
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
    f.swayward().layout.nest_or_unnest_window_left(None);
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


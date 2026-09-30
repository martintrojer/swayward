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

#[test]
fn get_version_reports_swayward_version_independently_of_niri_base() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();

    let version = query_ipc(&mut fixture, &mut stream, MessageType::GetVersion);
    let human_readable = version["human_readable"].as_str().unwrap();

    assert!(human_readable.starts_with("swayward beta0-dev ("));
    assert!(human_readable.ends_with(')'));
    assert_eq!(version["major"], 1);
    assert_eq!(version["minor"], 0);
    assert_eq!(version["patch"], 0);
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
    fixture.swayward().layout.nest_or_unnest_window_left(None);
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
fn floating_group_command_serializes_one_recursive_root() {
    let mut fixture = nested_split_fixture();

    let outcomes = crate::command::execute(fixture.niri_state(), "floating enable");

    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(outcomes[0].success, "{outcomes:?}");
    let tree = command_tree(&mut fixture);
    fn visible_workspace(node: &Value) -> Option<&Value> {
        if node["type"] == "workspace" && node["name"] != "__i3_scratch" {
            return Some(node);
        }
        node["nodes"]
            .as_array()?
            .iter()
            .find_map(visible_workspace)
    }
    let workspace = visible_workspace(&tree).unwrap();
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1, "{tree:#}");
    let floating = &workspace["floating_nodes"];
    assert_eq!(floating.as_array().unwrap().len(), 1);
    assert_eq!(floating[0]["type"], "floating_con");
    assert_eq!(floating[0]["floating"], "user_on");
    fn leaf_count(node: &Value) -> usize {
        if node["type"] == "con" && node["nodes"].as_array().is_some_and(Vec::is_empty) {
            return 1;
        }
        node["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(leaf_count)
            .sum()
    }
    assert_eq!(leaf_count(&floating[0]), 2, "{tree:#}");
}

#[test]
fn move_position_moves_a_focused_floating_group() {
    let mut fixture = nested_split_fixture();
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);

    let outcomes = crate::command::execute(fixture.niri_state(), "move position 120 100");

    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(outcomes[0].success, "{outcomes:?}");
    let tree = command_tree(&mut fixture);
    let floating = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(floating["rect"]["x"], 120);
    assert_eq!(floating["rect"]["y"], 100);

    let id = floating["id"].as_u64().unwrap();
    let outcomes = crate::command::execute(
        fixture.niri_state(),
        &format!("[con_id={id}] move position 75 50"),
    );
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert!(outcomes[0].success, "{outcomes:?}");
    let tree = command_tree(&mut fixture);
    let floating = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(floating["rect"]["x"], 75);
    assert_eq!(floating["rect"]["y"], 50);

    let outcomes = crate::command::execute(fixture.niri_state(), "move position center");
    assert!(outcomes[0].success, "{outcomes:?}");
    let tree = command_tree(&mut fixture);
    let floating = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(floating["rect"]["x"], 200);
    assert_eq!(floating["rect"]["y"], 75);

    assert!(crate::command::execute(fixture.niri_state(), "focus child")[0].success);
    let outcomes = crate::command::execute(fixture.niri_state(), "move position 300 250");
    assert_eq!(
        outcomes[0].error.as_deref(),
        Some("Only floating containers can be moved to an absolute position"),
        "{outcomes:?}"
    );
    let tree = command_tree(&mut fixture);
    let floating = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(floating["rect"]["x"], 200);
    assert_eq!(floating["rect"]["y"], 75);
}

#[test]
fn criteria_targeted_floating_group_commands_operate_on_the_root() {
    let mut fixture = nested_split_fixture();
    assert!(crate::command::execute(fixture.niri_state(), "mark floating-group")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus child")[0].success);

    for command in [
        r#"[con_mark="floating-group"] floating enable"#,
        r#"[con_mark="floating-group"] sticky enable"#,
        r#"[con_mark="floating-group"] move scratchpad"#,
    ] {
        let outcomes = crate::command::execute(fixture.niri_state(), command);
        assert_eq!(outcomes.len(), 1, "{command}: {outcomes:?}");
        assert!(outcomes[0].success, "{command}: {outcomes:?}");
    }
    assert_eq!(fixture.swayward().layout.scratchpad_windows().count(), 2);
}

#[test]
fn sticky_on_a_focused_floating_group_marks_the_root() {
    // Oracle: grouped_sticky_events (grouped_sticky_after). Sway sets
    // is_sticky on the focused container, here the floating group itself
    // (sway/commands/sticky.c:20-26).
    let mut fixture = nested_split_fixture();
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "sticky enable")[0].success);

    let tree = command_tree(&mut fixture);
    let floating = &tree["nodes"][1]["nodes"][0]["floating_nodes"];
    assert_eq!(floating.as_array().map(Vec::len), Some(1), "{tree:#}");
    assert_eq!(floating[0]["sticky"], true, "{tree:#}");
    for child in floating[0]["nodes"].as_array().unwrap() {
        assert_eq!(child["sticky"], false, "{tree:#}");
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
            "Invalid floating_minimum_size command (expected 3 arguments, got 4)",
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
            "Invalid floating_maximum_size command (expected 3 arguments, got 2)",
        ),
    ] {
        let before = f.swayward().config.borrow().layout.clone();
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
        assert_eq!(f.swayward().config.borrow().layout, before, "{command}");
    }
}


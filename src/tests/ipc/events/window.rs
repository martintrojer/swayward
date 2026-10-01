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
    windows::map_window(
        fixture,
        client,
        windows::WindowSpec {
            app_id: Some(app_id),
            ..Default::default()
        },
    );
}

#[test]
fn floating_a_group_emits_one_recursive_floating_event() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "group-first");
    assert!(crate::command::execute(fixture.niri_state(), "splitv")[0].success);
    map_test_window(&mut fixture, client, "group-second");
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    fixture.niri_state().ipc_refresh_layout();

    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, EVENT_WINDOW);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "floating");
    assert_eq!(event["container"]["type"], "floating_con");
    assert_eq!(event["container"]["floating"], "user_on");
    assert_eq!(event["container"]["nodes"].as_array().unwrap().len(), 2);
    assert!(remainder.is_empty(), "unexpected leaf events were buffered");
    subscriber.set_nonblocking(true).unwrap();
    fixture.dispatch();
    let mut byte = [0];
    assert!(matches!(
        subscriber.read(&mut byte),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[test]
fn floating_events_report_user_requested_state() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "floating-event");
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    for (command, expected) in [
        ("floating enable", "user_on"),
        ("floating disable", "user_off"),
    ] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        fixture.niri_state().ipc_refresh_layout();
        let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
        assert_eq!(event_type, EVENT_WINDOW);
        let event: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(event["change"], "floating");
        assert_eq!(event["container"]["floating"], expected);
    }
}

#[test]
fn fullscreening_a_floating_group_emits_one_recursive_fullscreen_event() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "group-first");
    assert!(crate::command::execute(fixture.niri_state(), "splitv")[0].success);
    map_test_window(&mut fixture, client, "group-second");
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    assert!(fixture
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .focused_container_node()
        .is_some());
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    assert!(crate::command::execute(fixture.niri_state(), "fullscreen enable")[0].success);
    fixture.niri_state().ipc_refresh_layout();

    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, EVENT_WINDOW);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "fullscreen_mode");
    assert_eq!(event["container"]["type"], "floating_con");
    assert_eq!(event["container"]["fullscreen_mode"], 1);
    assert_eq!(event["container"]["nodes"].as_array().unwrap().len(), 2);
    assert!(remainder.is_empty(), "unexpected leaf events were buffered");
}

#[test]
fn moving_a_floating_group_to_scratchpad_emits_one_recursive_move_event() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "group-first");
    assert!(crate::command::execute(fixture.niri_state(), "splitv")[0].success);
    map_test_window(&mut fixture, client, "group-second");
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    assert!(crate::command::execute(fixture.niri_state(), "move scratchpad")[0].success);
    fixture.niri_state().ipc_refresh_layout();

    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, EVENT_WINDOW);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "move");
    assert_eq!(event["container"]["type"], "floating_con");
    assert_eq!(event["container"]["scratchpad_state"], "fresh");
    assert_eq!(event["container"]["nodes"].as_array().unwrap().len(), 2);
    assert!(remainder.is_empty(), "unexpected leaf events were buffered");

    assert!(crate::command::execute(fixture.niri_state(), "scratchpad show")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, EVENT_WINDOW);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "focus");
    assert_eq!(event["container"]["type"], "con");
    assert!(event["container"]["focused"].as_bool().unwrap());
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, EVENT_WINDOW);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "move");
    assert_eq!(event["container"]["type"], "floating_con");
    assert_eq!(event["container"]["scratchpad_state"], "fresh");
    assert_eq!(event["container"]["nodes"].as_array().unwrap().len(), 2);
    assert!(event["container"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["focused"] == true));
    assert!(remainder.is_empty(), "unexpected leaf events were buffered");
}

#[test]
fn criteria_can_focus_a_resident_floating_group_leaf() {
    let mut fixture = nested_split_fixture();
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);

    let target = {
        let workspace = fixture.swayward().layout.active_workspace().unwrap();
        workspace
            .windows()
            .find(|window| {
                workspace
                    .floating_tree_root_for_window(&window.window)
                    .is_some()
            })
            .unwrap()
            .id()
    };
    let outcome = crate::command::execute(
        fixture.niri_state(),
        &format!("[con_id={}] focus", crate::ipc::tree::window_id(target)),
    );

    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().layout.focus().unwrap().id(), target);
}

#[test]
fn focusing_a_resident_floating_group_leaf_emits_focus() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    for _ in 0..2 {
        map_test_window(&mut fixture, client, "group");
    }
    fixture.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    let target = {
        let workspace = fixture.swayward().layout.active_workspace().unwrap();
        workspace
            .windows()
            .find(|window| {
                workspace
                    .floating_tree_root_for_window(&window.window)
                    .is_some()
            })
            .unwrap()
            .id()
    };
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        &format!("[con_id={}] focus", crate::ipc::tree::window_id(target)),
    );
    assert!(outcome[0].success, "{outcome:?}");
    fixture.niri_state().ipc_refresh_layout();

    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, EVENT_WINDOW);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "focus");
    assert_eq!(
        event["container"]["id"],
        crate::ipc::tree::window_id(target)
    );
    assert!(remainder.is_empty(), "unexpected events were buffered");
}

#[test]
fn moving_a_floating_group_to_new_workspace_emits_empty_init() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "group-first");
    assert!(crate::command::execute(fixture.niri_state(), "splitv")[0].success);
    map_test_window(&mut fixture, client, "group-second");
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    fixture.niri_state().ipc_refresh_layout();

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    assert!(
        crate::command::execute(fixture.niri_state(), "move container to workspace 2",)[0].success
    );
    fixture.niri_state().ipc_refresh_layout();

    let ((event_type, payload), _) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, EVENT_WORKSPACE);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "init");
    assert!(event["current"]["nodes"].as_array().unwrap().is_empty());
    assert!(event["current"]["floating_nodes"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn captured_window_map_sequences_pin_focus_order_and_multiplicity() {
    for (fixture, expected) in [
        (
            sway_fixture!("events/window-map-focused.sequence.json"),
            &["new", "title", "focus"][..],
        ),
        (
            sway_fixture!("events/window-map-unfocused.sequence.json"),
            &["new", "title"][..],
        ),
    ] {
        let events: Vec<Value> = serde_json::from_str(&fixture).unwrap();
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
fn mapping_a_focused_window_emits_new_title_then_focus() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();
    let client = fixture.add_client();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("focused-map".into());
    window.set_title("focused-map");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    fixture.niri_state().update_keyboard_focus();
    assert!(fixture
        .swayward()
        .layout
        .windows()
        .any(|(_, mapped)| mapped.is_focused()));
    fixture.niri_state().ipc_refresh_layout();

    let mut remainder = Vec::new();
    let events = (0..3)
        .map(|_| {
            let ((event_type, payload), next) =
                read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone());
            remainder = next;
            assert_eq!(event_type, EVENT_WINDOW);
            serde_json::from_str::<Value>(&payload).unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events
            .iter()
            .map(|event| event["change"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["new", "title", "focus"]
    );
    for event in &events[..2] {
        assert_eq!(event["container"]["border"], "none");
        assert_eq!(event["container"]["current_border_width"], 0);
        assert_eq!(event["container"]["focused"], false);
        assert_eq!(event["container"]["percent"], 0.0);
        assert_eq!(
            event["container"]["rect"],
            serde_json::json!({"x":0,"y":0,"width":0,"height":0})
        );
    }
    assert_eq!(events[0]["container"]["name"], Value::Null);
    assert_eq!(events[1]["container"]["name"], "focused-map");
    assert_eq!(events[2]["container"]["border"], "none");
    assert_eq!(events[2]["container"]["current_border_width"], 0);
}

#[test]
fn map_events_keep_a_new_tab_hidden_until_its_focus_event() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);
    let client = fixture.add_client();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    windows::map_window(
        &mut fixture,
        client,
        windows::WindowSpec {
            app_id: Some("first-tab"),
            title: Some("first-tab"),
            ..Default::default()
        },
    );
    fixture.niri_state().update_keyboard_focus();
    fixture.niri_state().ipc_refresh_layout();
    let mut remainder = Vec::new();
    for _ in 0..3 {
        let ((_, _), next) =
            read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
        remainder = next;
    }

    windows::map_window(
        &mut fixture,
        client,
        windows::WindowSpec {
            app_id: Some("second-tab"),
            title: Some("second-tab"),
            ..Default::default()
        },
    );
    fixture.niri_state().update_keyboard_focus();
    fixture.niri_state().ipc_refresh_layout();
    let mut events = Vec::new();
    for _ in 0..3 {
        let ((event_type, payload), next) =
            read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
        remainder = next;
        assert_eq!(event_type, EVENT_WINDOW);
        events.push(serde_json::from_str::<Value>(&payload).unwrap());
    }
    assert_eq!(
        events
            .iter()
            .map(|event| event["change"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["new", "title", "focus"]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event["container"]["visible"].as_bool().unwrap())
            .collect::<Vec<_>>(),
        [false, false, true]
    );
    assert!(remainder.is_empty());
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
    assert_eq!(event_type, EVENT_WINDOW);
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
    let (mut fixture, socket) = ipc_fixture_with_config(config);
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
    assert_eq!(event_type, EVENT_WINDOW);
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
    assert_eq!(event_type, EVENT_WINDOW);
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
        assert_eq!(message_type, EVENT_WINDOW, "unexpected message on the fd");
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
    assert_eq!(event_type, EVENT_WORKSPACE);
    let expected: Value =
        serde_json::from_str(&sway_fixture!("events/workspace.reload.json")).unwrap();
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
    assert_eq!(event_type, EVENT_WINDOW);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/window.focus.json")).unwrap();
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
    assert_eq!(event_type, EVENT_MODE);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/mode.default.json")).unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");
}

#[test]
fn closing_the_focused_window_emits_close_before_restored_focus() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    for app_id in ["first", "second"] {
        windows::map_window(
            &mut fixture,
            client,
            windows::WindowSpec {
                app_id: Some(app_id),
                title: Some(app_id),
                ..Default::default()
            },
        );
    }
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);
    let mapped = fixture
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| mapped.formatted_title() == "second")
        .unwrap()
        .1
        .window
        .clone();

    fixture
        .swayward()
        .layout
        .remove_window(&mapped, crate::utils::transaction::Transaction::new());
    fixture.niri_state().update_keyboard_focus();
    fixture.niri_state().ipc_refresh_layout();

    let ((_, close), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    let ((_, focus), _) = read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    let close: Value = serde_json::from_str(&close).unwrap();
    let focus: Value = serde_json::from_str(&focus).unwrap();
    assert_eq!(close["change"], "close");
    assert_eq!(close["container"]["app_id"], "second");
    assert_eq!(focus["change"], "focus");
    assert_eq!(focus["container"]["app_id"], "first");
}

/// Oracle: events/for_window_during_scratchpad. A `mark` whose runtime
/// `for_window` rule matches re-enters the command executor in the middle of
/// the list. The outer transaction must still order `move scratchpad` as sway
/// does: floating while visible, then move with the hidden state.
#[test]
fn nested_for_window_command_keeps_the_outer_scratchpad_event_order() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "for-window-scratchpad");
    assert!(
        crate::command::execute(fixture.niri_state(), "for_window [con_mark=\"oracle\"] nop")[0]
            .success
    );
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    let outcome = crate::command::execute(fixture.niri_state(), "mark oracle, move scratchpad");
    assert!(outcome.iter().all(|outcome| outcome.success), "{outcome:?}");
    fixture.niri_state().ipc_refresh_layout();

    let mut events = Vec::new();
    let mut remainder = Vec::new();
    while let Some(((_, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        events.push((
            event["change"].as_str().unwrap().to_owned(),
            event["container"]["scratchpad_state"]
                .as_str()
                .unwrap()
                .to_owned(),
        ));
    }
    assert_eq!(
        events,
        [
            ("mark".to_owned(), "none".to_owned()),
            ("mark".to_owned(), "none".to_owned()),
            ("floating".to_owned(), "none".to_owned()),
            ("move".to_owned(), "fresh".to_owned()),
        ]
    );
}

/// Two independent windows moved by one criteria command must retain their
/// own container ids in every floating/move event. The transaction used to
/// copy the first visible/hidden snapshot onto every event.
#[test]
fn criteria_scratchpad_events_keep_each_windows_container_id() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "scratch-a");
    map_test_window(&mut fixture, client, "scratch-b");
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = subscribe_to_window_events(&mut fixture, &socket);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "[app_id=\"^scratch-[ab]$\"] move scratchpad",
    );
    assert!(outcome[0].success, "{outcome:?}");
    fixture.niri_state().ipc_refresh_layout();

    let mut remainder = Vec::new();
    let mut by_change = std::collections::HashMap::<String, Vec<i64>>::new();
    while let Some(((_, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        let change = event["change"].as_str().unwrap().to_owned();
        if matches!(change.as_str(), "floating" | "move") {
            by_change
                .entry(change)
                .or_default()
                .push(event["container"]["id"].as_i64().unwrap());
        }
    }
    for change in ["floating", "move"] {
        let mut ids = by_change.remove(change).unwrap_or_default();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 2, "{change}: {ids:?}");
    }
}

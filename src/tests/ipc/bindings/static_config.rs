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
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(event_type, EVENT_WINDOW);
    let cleared = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(cleared["change"], "mark");
    assert_eq!(cleared["container"]["marks"], serde_json::json!([]));

    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, EVENT_WINDOW);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/window.mark.json")).unwrap();
    let marked = serde_json::from_str(&payload).unwrap();
    assert_event_shape(&expected, &marked, "$window");
    assert_eq!(
        marked["container"]["marks"],
        serde_json::json!(["event-mark"])
    );
    assert!(remainder.is_empty());
}

#[test]
fn plain_mark_on_container_emits_clear_then_add_events() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for index in 0..3 {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("event-{index}"));
        window.set_title(&format!("event-{index}"));
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        if index == 0 {
            assert!(crate::command::execute(fixture.niri_state(), "splitv")[0].success);
        }
        if index == 1 {
            assert!(crate::command::execute(fixture.niri_state(), "splith")[0].success);
        }
    }
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "mark containermark")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let ((_, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    let cleared = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(cleared["change"], "mark");
    assert_eq!(cleared["container"]["marks"], serde_json::json!([]));

    let ((_, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    let marked = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(marked["change"], "mark");
    assert_eq!(
        marked["container"]["marks"],
        serde_json::json!(["containermark"])
    );
    assert!(remainder.is_empty());
}

#[test]
fn moving_a_mark_off_a_container_emits_the_containers_unmark_event() {
    // cmd_mark calls container_find_and_unmark, which emits `mark` on the container that loses
    // the mark (sway/tree/container.c:1582-1600). Oracle row
    // events/criteria_order_split_before_child.
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for index in 0..3 {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("event-{index}"));
        window.set_title(&format!("event-{index}"));
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
        if index == 0 {
            assert!(crate::command::execute(fixture.niri_state(), "splitv")[0].success);
        }
        if index == 1 {
            assert!(crate::command::execute(fixture.niri_state(), "splith")[0].success);
        }
    }
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "mark moving")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus child")[0].success);
    fixture.niri_state().ipc_refresh_layout();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "mark moving")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let mut events = Vec::new();
    let mut remainder = Vec::new();
    for _ in 0..3 {
        let ((_, payload), rest) =
            read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
        remainder = rest;
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        events.push((
            event["change"].as_str().unwrap().to_owned(),
            event["container"]["type"].as_str().unwrap().to_owned(),
            event["container"]["name"].is_null(),
            event["container"]["marks"].clone(),
        ));
    }
    let empty = serde_json::json!([]);
    let moving = serde_json::json!(["moving"]);
    assert_eq!(
        events,
        [
            // container_clear_marks on the window, then the split's unmark, then the add.
            ("mark".to_owned(), "con".to_owned(), false, empty.clone()),
            ("mark".to_owned(), "con".to_owned(), true, empty),
            ("mark".to_owned(), "con".to_owned(), false, moving),
        ]
    );
    assert!(remainder.is_empty());
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
    assert_eq!(event_type, EVENT_WINDOW);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/window.close.json")).unwrap();
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
    let oracle: Value = serde_json::from_str(&sway_fixture!("marked.tree.json")).unwrap();
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
/// marks in the order it meets them (`sway/tree/root.c:243-262`,
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

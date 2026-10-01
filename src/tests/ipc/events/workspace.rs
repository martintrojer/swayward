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
    send_tick_barrier(&mut fixture);

    let (event_type, _) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, EVENT_TICK);
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

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let mut command = UnixStream::connect(&socket).unwrap();
    let mut remainder = Vec::new();
    for name in ["2", "3", "1"] {
        let reply = query_ipc_with_payload(
            &mut fixture,
            &mut command,
            MessageType::RunCommand,
            &format!("workspace {name}"),
        );
        assert_eq!(reply[0]["success"], true);
        loop {
            let ((event_type, payload), next) =
                read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
            remainder = next;
            assert_eq!(event_type, EVENT_WORKSPACE);
            let event = serde_json::from_str::<Value>(&payload).unwrap();
            if event["change"] == "focus" {
                assert_eq!(event["current"]["name"], name);
                assert_eq!(event["current"]["focused"], true, "{event}");
                assert_eq!(event["old"]["focused"], false, "{event}");
                if name == "2" {
                    assert_eq!(
                        event["old"]["nodes"][0]["focused"], false,
                        "the old workspace snapshot must reflect the completed focus mutation: {event}"
                    );
                    assert_eq!(event["old"]["nodes"][0]["visible"], false, "{event}");
                }
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
    assert_eq!(event_type, EVENT_WORKSPACE);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected =
        serde_json::from_str::<Value>(&sway_fixture!("events/workspace.urgent.json")).unwrap();
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
    assert_eq!(event_type, EVENT_WORKSPACE);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected =
        serde_json::from_str::<Value>(&sway_fixture!("events/workspace.move.json")).unwrap();
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
    assert_eq!(event_type, EVENT_WORKSPACE);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected =
        serde_json::from_str::<Value>(&sway_fixture!("events/workspace.rename.json")).unwrap();
    assert_event_shape(&expected, &actual, "$workspace");
    assert_eq!(actual["change"], "rename");
    assert_eq!(actual["old"], Value::Null);
    assert_eq!(actual["current"]["name"], "event-renamed");
}

#[test]
fn captured_workspace_event_sequences_pin_order_and_multiplicity() {
    for (fixture, expected) in [
        (
            sway_fixture!("events/workspace-switch-empty.sequence.json"),
            &["init", "focus", "focus", "focus", "empty"][..],
        ),
        (
            sway_fixture!("events/workspace-close-last.sequence.json"),
            &["close", "empty"][..],
        ),
        (
            sway_fixture!("events/workspace-rename.sequence.json"),
            &["rename"][..],
        ),
        (
            sway_fixture!("events/workspace-move-right-empty-destination.sequence.json"),
            &["move"][..],
        ),
        (
            sway_fixture!("events/workspace-move-right-occupied-destination.sequence.json"),
            &["move"][..],
        ),
        (
            sway_fixture!("events/workspace-move-right-last-source.sequence.json"),
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
fn subscribing_to_all_sway_event_families_succeeds() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace","output","mode","window","barconfig_update","binding","shutdown","tick","input"]"#,
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
    assert!(remainder.is_empty());
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
            sway_fixture!("events/window-map-focused.sequence.json"),
            &["new", "title", "focus"][..],
        ),
        (
            sway_fixture!("events/window-map-unfocused.sequence.json"),
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
    let expected: Value =
        serde_json::from_str(sway_fixture!("events/workspace.reload.json")).unwrap();
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
    let expected: Value = serde_json::from_str(sway_fixture!("events/window.focus.json")).unwrap();
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
    let expected: Value = serde_json::from_str(sway_fixture!("events/mode.default.json")).unwrap();
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
    let expected =
        serde_json::from_str::<Value>(sway_fixture!("events/workspace.urgent.json")).unwrap();
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
    let expected =
        serde_json::from_str::<Value>(sway_fixture!("events/workspace.move.json")).unwrap();
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
    let expected =
        serde_json::from_str::<Value>(sway_fixture!("events/workspace.rename.json")).unwrap();
    assert_event_shape(&expected, &actual, "$workspace");
    assert_eq!(actual["change"], "rename");
    assert_eq!(actual["old"], Value::Null);
    assert_eq!(actual["current"]["name"], "event-renamed");
}

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
        let events = serde_json::from_str::<Vec<Value>>(&fixture).unwrap();
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
    let scratch = ScratchDir::new("get-config");
    let root = &scratch.0;
    std::fs::write(root.join("included.kdl"), "layout { gaps 7; }\n").unwrap();
    let source = "include \"included.kdl\"\n";
    let config = swayward_config::Config::parse(&root.join("config.kdl"), source)
        .config
        .unwrap();
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
    let (mut fixture, socket) = ipc_fixture_with_config(config);
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
    let (mut fixture, socket) = ipc_fixture_with_config(config);
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


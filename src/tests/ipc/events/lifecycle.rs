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
    assert_eq!(event_type, EVENT_OUTPUT);
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
    send_tick_barrier(&mut fixture);
    let ((event_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, EVENT_TICK);
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
    assert_eq!(event_type, EVENT_SHUTDOWN);
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
    assert_eq!(event_type, EVENT_TICK);
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
    assert_eq!(event_type, EVENT_TICK);
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
    assert_eq!(event_type, EVENT_TICK);
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
    assert_eq!(event_type, EVENT_WORKSPACE);
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
    assert_eq!(event_type, EVENT_WORKSPACE);

    subscriber.write_all(&request[7..]).unwrap();
    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply_type, MessageType::GetVersion as u32);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap()["variant"],
        "swayward"
    );
}

#[test]
fn disconnected_event_subscribers_are_removed_without_an_event() {
    let (mut fixture, socket) = ipc_fixture();

    for clean_disconnect in [true, false] {
        let mut subscriber = UnixStream::connect(&socket).unwrap();
        subscriber
            .write_all(&swayward_ipc::wire::encode(
                MessageType::Subscribe,
                r#"["output"]"#,
            ))
            .unwrap();
        let _ = read_ipc_reply(&mut fixture, &mut subscriber);
        assert_eq!(
            fixture
                .swayward()
                .ipc_server
                .as_ref()
                .unwrap()
                .event_stream_count(),
            1
        );

        if clean_disconnect {
            subscriber
                .write_all(swayward_ipc::wire::CLOSE_SENTINEL)
                .unwrap();
        }
        drop(subscriber);
        let deadline = Instant::now() + Duration::from_secs(1);
        while fixture
            .swayward()
            .ipc_server
            .as_ref()
            .unwrap()
            .event_stream_count()
            != 0
            && Instant::now() < deadline
        {
            fixture.dispatch();
        }

        assert_eq!(
            fixture
                .swayward()
                .ipc_server
                .as_ref()
                .unwrap()
                .event_stream_count(),
            0,
            "subscriber must be removed after clean_disconnect={clean_disconnect}"
        );
    }
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
                payload: payload.as_bytes().to_vec(),
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

#[test]
fn send_tick_preserves_raw_payload_bytes_like_sway() {
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
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder); // initial tick
    assert!(remainder.is_empty());

    let mut command = UnixStream::connect(socket).unwrap();
    command
        .write_all(&swayward_ipc::wire::encode_raw_bytes(
            MessageType::SendTick as u32,
            b"\xff",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut command);
    assert_eq!(reply, r#"{"success": true}"#);

    subscriber.set_nonblocking(true).unwrap();
    let mut raw = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(1);
    while raw.len() < swayward_ipc::wire::HEADER_SIZE {
        fixture.dispatch();
        let mut buf = [0; 128];
        match subscriber.read(&mut buf) {
            Ok(len) => raw.extend_from_slice(&buf[..len]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("tick read failed: {error}"),
        }
        assert!(Instant::now() < deadline, "timed out waiting for raw tick");
    }
    let length = u32::from_ne_bytes(raw[6..10].try_into().unwrap()) as usize;
    while raw.len() < swayward_ipc::wire::HEADER_SIZE + length {
        fixture.dispatch();
        let mut buf = [0; 128];
        if let Ok(len) = subscriber.read(&mut buf) {
            raw.extend_from_slice(&buf[..len]);
        }
        assert!(Instant::now() < deadline, "timed out waiting for tick body");
    }
    assert_eq!(
        &raw[swayward_ipc::wire::HEADER_SIZE..][..length],
        b"{ \"first\": false, \"payload\": \"\xff\" }"
    );
}

#[test]
fn ticks_do_not_fill_non_tick_subscriber_queues() {
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);
    let mut command = UnixStream::connect(socket).unwrap();
    for _ in 0..4100 {
        command
            .write_all(&swayward_ipc::wire::encode(
                MessageType::SendTick,
                "barrier",
            ))
            .unwrap();
        let _ = read_ipc_reply(&mut fixture, &mut command);
    }
    assert_eq!(
        fixture
            .swayward()
            .ipc_server
            .as_ref()
            .unwrap()
            .event_stream_count(),
        1
    );
}

#[test]
fn a_later_tick_subscription_receives_send_tick() {
    // SEND_TICK queues only for tick subscribers, so a client that adds
    // `tick` to an existing subscription must start receiving ticks
    // (sway checks the client's current subscriptions on every event,
    // sway/sway/ipc-server.c:671-676).
    let (mut fixture, socket) = ipc_fixture();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, reply), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(reply, r#"{"success": true}"#);
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["tick"]"#,
        ))
        .unwrap();
    let ((_, reply), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(reply, r#"{"success": true}"#);
    let ((_, first), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(first, r#"{"first":true,"payload":""}"#);

    let mut command = UnixStream::connect(socket).unwrap();
    command
        .write_all(&swayward_ipc::wire::encode(MessageType::SendTick, "later"))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut command);
    assert_eq!(reply, r#"{"success": true}"#);
    let ((kind, tick), _) = read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(kind, (1 << 31) | 7);
    assert_eq!(tick, r#"{ "first": false, "payload": "later" }"#);
}

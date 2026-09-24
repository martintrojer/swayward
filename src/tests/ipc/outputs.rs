#[test]
fn headless_output_uses_configured_mode_when_added() {
    let config = swayward_config::Config::parse_mem(
        r#"output "headless-1" { mode custom=true "1270x1408@60"; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    assert_eq!(
        fixture.niri_output(1).current_mode().unwrap().size,
        (1270, 1408).into()
    );
}

#[test]
fn output_runtime_commands_apply_named_state_and_wildcard_fanout() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    fixture.add_named_output_at("right".into(), (1024, 768), Some((800, 0)));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
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
    assert_eq!(left.current_mode().unwrap().size, (1280, 720).into());
    let mut stream = UnixStream::connect(&socket).unwrap();
    let outputs = query_ipc(&mut fixture, &mut stream, MessageType::GetOutputs);
    let left = outputs
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["name"] == "left")
        .unwrap();
    assert_eq!(left["current_mode"]["width"], 1280);
    assert_eq!(left["current_mode"]["height"], 720);

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
/// connector replug moves the affected workspace; restoring the focused
/// workspace also empties the fallback output, so sway creates its replacement
/// workspace (`sway/tree/output.c:48-56`). Focus commands retain their ordinary
/// one-event-per-transition behavior while locked.
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
        ["move", "empty", "init", "move"]
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

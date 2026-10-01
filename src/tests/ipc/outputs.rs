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
fn ipc_output_snapshot_recovers_from_a_poisoned_backend_mutex() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let outputs = fixture.niri_state().backend.ipc_outputs();
    let poisoner = outputs.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.lock().unwrap();
        panic!("poison backend output state");
    })
    .join();

    let snapshot = crate::ipc::server::ipc_outputs_snapshot(fixture.niri_state());

    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot.values().next().unwrap().name, "headless-1");
}

#[test]
fn headless_output_disable_evacuates_workspaces_and_enable_reconnects_it() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    fixture.add_output(2, (1024, 768));
    fixture.niri_focus_output(2);
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    assert!(crate::command::execute(fixture.niri_state(), "output headless-2 disable")[0].success);
    assert_eq!(fixture.swayward().layout.outputs().count(), 1);
    assert_eq!(fixture.swayward().layout.windows().count(), 1);
    assert!(fixture
        .swayward()
        .layout
        .windows()
        .all(|(monitor, _)| monitor.unwrap().output_name() == "headless-1"));

    assert!(crate::command::execute(fixture.niri_state(), "output headless-2 enable")[0].success);
    assert_eq!(fixture.swayward().layout.outputs().count(), 2);
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
    assert_eq!(event_type, EVENT_OUTPUT);
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
fn rapid_output_config_changes_report_sway_output_state() {
    let mut fixture = Fixture::new();
    fixture.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    fixture.add_named_output_at("right".into(), (1024, 768), Some((800, 0)));
    let client = fixture.add_client();

    map_test_window(&mut fixture, client, "tiled");
    map_test_window(&mut fixture, client, "floating");
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "sticky enable")[0].success);
    map_test_window(&mut fixture, client, "fullscreen");
    assert!(crate::command::execute(fixture.niri_state(), "fullscreen enable")[0].success);

    for command in [
        "output left scale 1.25",
        "output left transform 90",
        "output left position 200 300",
        "output left mode --custom 640x480@60Hz",
        "output right disable",
        "output left transform 270 scale 1.5 mode --custom 1280x720@75Hz",
        "output left disable",
        "output right enable transform 90 scale 1.25 mode --custom 600x800@60Hz",
        "output left enable",
        "output right disable",
        "output right enable",
        "output * power off",
        "output * power on",
    ] {
        assert!(
            crate::command::execute(fixture.niri_state(), command)[0].success,
            "{command}"
        );
        fixture.niri_state().refresh_and_flush_clients();
    }

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let floating = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find_map(|workspace| workspace["floating_nodes"].as_array()?.first())
        .unwrap();
    assert_eq!(floating["focused"], false);
    assert!(floating["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["visible"] == false));

    let outputs = crate::ipc::tree::describe_outputs_with_power(
        &swayward.layout,
        &swayward.global_space,
        &swayward.output_power,
    );
    let left = outputs.iter().find(|output| output.name == "left").unwrap();
    let right = outputs.iter().find(|output| output.name == "right").unwrap();
    assert_eq!(left.current_mode.width, 1280);
    assert_eq!(left.current_mode.height, 720);
    assert_eq!(left.scale, 1.5);
    assert_eq!(left.scale_filter, "linear");
    assert_eq!(left.transform, "270");
    assert!(left.power);
    assert!(left.dpms);
    assert_eq!(right.current_mode.width, 600);
    assert_eq!(right.current_mode.height, 800);
    assert_eq!(right.scale, 1.25);
    assert_eq!(right.scale_filter, "linear");
    assert_eq!(right.transform, "90");
    assert!(right.power);
    assert!(right.dpms);
    assert_eq!(swayward.layout.windows().count(), 3);
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
    assert_eq!(event_type, EVENT_OUTPUT);
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
    send_tick_barrier(fixture);
    let mut events = Vec::new();
    loop {
        let ((event_type, payload), next) =
            read_ipc_reply_with_remainder(fixture, subscriber, remainder);
        remainder = next;
        if event_type == EVENT_TICK {
            break;
        }
        assert!(event_type == EVENT_WORKSPACE || event_type == EVENT_WINDOW);
        events.push(serde_json::from_str(&payload).unwrap());
    }
    (events, remainder)
}

/// Sway's session-lock implementation changes seat focus but does not call
/// `ipc_event_workspace` or `ipc_event_window` (`sway/desktop/session_lock.c`).
/// Output power and idle wake likewise have no workspace/window event. A real
/// connector replug moves the affected workspace; restoring the focused
/// workspace also empties the fallback output, so sway creates its replacement
/// workspace (`sway/tree/output.c:48-56`). After replug, selecting a new
/// workspace initializes it and destroys the empty unaddressable workspace that
/// loses focus (`sway/tree/workspace.c:313-330`).
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
        ["init", "focus", "focus", "empty"]
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

#[test]
fn disabling_an_output_that_repositions_another_emits_one_output_event() {
    // Oracle: events/disabled_output_evacuation. Sway sends one output event
    // for the whole configuration change, even when a surviving output is
    // repositioned (update_output_manager_config, sway/desktop/output.c:377-399).
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1280, 720));
    fixture.add_output(2, (1280, 720));
    fixture.niri_state().refresh_ipc_outputs();
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["output"]"#,
        ))
        .unwrap();
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    // headless-1 sits at x=0, so disabling it moves headless-2 left.
    assert!(crate::command::execute(fixture.niri_state(), "output headless-1 disable")[0].success);
    fixture.niri_state().refresh_ipc_outputs();

    let mut events = Vec::new();
    while let Some(((event_type, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        events.push((event_type, payload));
    }
    assert_eq!(
        events,
        [(EVENT_OUTPUT, r#"{"change":"unspecified"}"#.to_owned())]
    );
}

#[test]
fn renaming_a_declared_workspace_while_outputless_does_not_abort_output_enable() {
    // Renaming the declared workspace away from its name drops its persistence
    // while no output holds it. Sway destroys an empty non-persistent workspace
    // (workspace_consider_destroy, sway/tree/workspace.c:313-332), and a
    // re-enabled output then creates its own initial workspace
    // (restore_workspaces, sway/tree/output.c:31-57).
    let config = swayward_config::Config::parse_mem(r#"workspace "keep" {}"#).unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (800, 600));
    let disable = crate::command::execute(fixture.niri_state(), "output headless-1 disable");
    assert!(disable[0].success, "{disable:?}");
    // Sway refuses a rename while no output exists (sway/commands/rename.c:25-28).
    // swayward still runs it, which drops the workspace's persistence; its
    // reply is not asserted here.
    crate::command::execute(fixture.niri_state(), "rename workspace keep to other");
    let enable = crate::command::execute(fixture.niri_state(), "output headless-1 enable");
    assert!(enable[0].success, "{enable:?}");
    assert_eq!(fixture.swayward().layout.outputs().count(), 1);
    assert_eq!(fixture.swayward().layout.workspaces().count(), 1);
    assert!(!workspace_names(&mut fixture).contains(&"other".to_owned()));
    fixture.swayward().layout.verify_invariants();
}

#[test]
fn moving_a_floating_group_to_the_scratchpad_while_outputless_does_not_abort_output_enable() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for app_id in ["fixture-1", "fixture-2"] {
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
    for command in ["focus parent", "floating enable", "output headless-1 disable"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    // Sway refuses this command while no output exists
    // (sway/commands/scratchpad.c:100-103; oracle
    // outputless_floating_group_scratchpad). swayward still runs it, which
    // empties the outputless workspace; its reply is not asserted here.
    crate::command::execute(fixture.niri_state(), "[app_id=fixture-1] move scratchpad");
    let enable = crate::command::execute(fixture.niri_state(), "output headless-1 enable");
    assert!(enable[0].success, "{enable:?}");

    let layout = &fixture.swayward().layout;
    assert_eq!(layout.outputs().count(), 1);
    assert_eq!(layout.workspaces().count(), 1);
    assert_eq!(layout.windows().count(), 2);
    layout.verify_invariants();
}

#[test]
fn tiny_scale_on_large_output_does_not_overflow_tree_percentages() {
    // An 8K output at scale 0.1 is 76800x43200 logical pixels, whose area
    // exceeds i32::MAX. GET_TREE and every layout refresh must still succeed.
    let mut fixture = Fixture::new();
    fixture.add_output(1, (7680, 4320));
    assert!(crate::command::execute(fixture.niri_state(), "output headless-1 scale 0.1")[0].success);
    let state = fixture.niri_state();
    let tree = crate::ipc::tree::describe_tree(
        &state.swayward.layout,
        &state.swayward.global_space,
        &state.swayward.marks_by_window,
        &state.swayward.marks_by_container,
    );
    let output = &tree.nodes[0];
    assert_eq!((output.rect.width, output.rect.height), (76800, 43200));

    // Tabbed and stacked children take the percentage path with a titlebar
    // offset; nested window rects take the border arithmetic.
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
    for command in ["splitv", "layout stacking", "focus parent", "layout tabbed"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success, "{command}");
    }
    let state = fixture.niri_state();
    let tree = crate::ipc::tree::describe_tree(
        &state.swayward.layout,
        &state.swayward.global_space,
        &state.swayward.marks_by_window,
        &state.swayward.marks_by_container,
    );
    assert_eq!(tree.nodes[0].rect.width, 76800);
}

/// Oracle: numbered_sparse/outputs. Sway lists every workspace on the output
/// in focus order (sway/ipc-json.c:827-835): after visiting 1, 3 and 7 the
/// focus is [7, 3, 1], the same list the GET_TREE output node carries.
#[test]
fn get_outputs_focus_lists_every_workspace_in_focus_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for (workspace, app_id) in [("1", "fixture-1"), ("3", "fixture-3"), ("7", "fixture-7")] {
        let outcome =
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"));
        assert!(outcome[0].success, "{outcome:?}");
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    let id_of = |name: &str| {
        workspaces
            .iter()
            .find(|workspace| workspace.name == name)
            .unwrap()
            .id
    };
    let expected = vec![id_of("7"), id_of("3"), id_of("1")];
    let outputs = crate::ipc::tree::describe_outputs(&swayward.layout, &swayward.global_space);
    assert_eq!(outputs[0].focus, expected);
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let output_node = tree
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some(outputs[0].name.as_str()))
        .unwrap();
    assert_eq!(output_node.focus, expected, "GET_TREE and GET_OUTPUTS agree");
}

/// Oracle: output_config_live_changes / output_power_off (tree). Sway's output
/// node carries the same runtime `power` and `dpms` as GET_OUTPUTS
/// (`sway/sway/ipc-json.c:368-372`, shared by both replies).
#[test]
fn get_tree_output_nodes_report_runtime_power_like_get_outputs() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    fixture.add_output(2, (1024, 768));
    let mut stream = UnixStream::connect(socket).unwrap();

    for (command, powered) in [("output * power off", false), ("output * power on", true)] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        fixture.niri_state().refresh_and_flush_clients();
        let outputs = query_ipc(&mut fixture, &mut stream, MessageType::GetOutputs);
        let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
        for output in outputs.as_array().unwrap() {
            let name = output["name"].as_str().unwrap();
            let node = tree["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["name"] == name)
                .unwrap();
            for field in ["power", "dpms"] {
                assert_eq!(output[field], powered, "{command}: GET_OUTPUTS {name} {field}");
                assert_eq!(node[field], powered, "{command}: GET_TREE {name} {field}");
            }
        }
    }
}

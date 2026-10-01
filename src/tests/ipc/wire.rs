#[test]
fn run_command_wire_covers_representative_command_families() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    for app_id in ["first", "second"] {
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

    let mut stream = UnixStream::connect(socket).unwrap();
    for command in [
        "workspace wire-smoke",
        "workspace back_and_forth; split vertical",
        "layout tabbed",
        r#"[app_id="first"] focus"#,
        "move right",
        "floating enable",
        "floating disable",
        "fullscreen enable",
        "fullscreen disable",
        "mark wire-smoke",
    ] {
        let outcomes = query_ipc_with_payload(
            &mut fixture,
            &mut stream,
            MessageType::RunCommand,
            command,
        );
        assert!(
            outcomes
                .as_array()
                .unwrap()
                .iter()
                .all(|outcome| outcome["success"] == true),
            "command failed over IPC: {command}: {outcomes}"
        );
    }

    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert!(find_json_node_with_app_id(&tree, "first").is_some());
    assert_eq!(
        find_json_node_with_mark(&tree, "wire-smoke").unwrap()["app_id"],
        "first"
    );
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("1".into())
    );
}

#[test]
fn ipc_refresh_without_a_seat_keyboard_does_not_panic() {
    let (mut fixture, _) = ipc_fixture();
    fixture.swayward().seat.remove_keyboard();

    fixture.niri_state().ipc_refresh_keyboard_layout_index();
    fixture.niri_state().ipc_keyboard_layouts_changed();
}

#[test]
fn input_queries_and_commands_survive_device_hotplug() {
    let (mut fixture, socket) = ipc_fixture();
    let mut query = UnixStream::connect(&socket).unwrap();
    let mut command = UnixStream::connect(socket).unwrap();

    for _ in 0..32 {
        for device in [
            TestDevice::keyboard("hotplug keyboard"),
            TestDevice::pointer("hotplug pointer"),
        ] {
            fixture.niri_state().process_input_event::<TestInput>(
                smithay::backend::input::InputEvent::DeviceAdded { device },
            );
        }

        let inputs = query_ipc(&mut fixture, &mut query, MessageType::GetInputs);
        assert_eq!(inputs.as_array().unwrap().len(), 2);
        let seats = query_ipc(&mut fixture, &mut query, MessageType::GetSeats);
        assert_eq!(seats[0]["capabilities"], 3);
        assert_eq!(seats[0]["devices"], inputs);

        for input in [
            "input * xkb_switch_layout next",
            "input * tap enabled",
            "input * natural_scroll enabled",
            "input * accel_speed 0.5",
            "seat seat0 hide_cursor 1000",
        ] {
            let reply = query_ipc_with_payload(
                &mut fixture,
                &mut command,
                MessageType::RunCommand,
                input,
            );
            assert_eq!(
                reply[0]["success"],
                input.starts_with("input * xkb_switch_layout"),
                "unexpected command reply for {input}: {reply}"
            );
        }

        for device in [
            TestDevice::keyboard("hotplug keyboard"),
            TestDevice::pointer("hotplug pointer"),
        ] {
            fixture.niri_state().process_input_event::<TestInput>(
                smithay::backend::input::InputEvent::DeviceRemoved { device },
            );
        }

        assert_eq!(
            query_ipc(&mut fixture, &mut query, MessageType::GetInputs),
            serde_json::json!([])
        );
        assert_eq!(
            query_ipc(&mut fixture, &mut query, MessageType::GetSeats),
            serde_json::json!([{
                "name": "seat0",
                "capabilities": 0,
                "focus": 0,
                "devices": []
            }])
        );
    }

    fixture.swayward().seat.remove_keyboard();
    fixture.niri_state().ipc_refresh_keyboard_layout_index();
    fixture.niri_state().ipc_keyboard_layouts_changed();
    let reply = query_ipc_with_payload(
        &mut fixture,
        &mut command,
        MessageType::RunCommand,
        "input * xkb_switch_layout next",
    );
    assert_eq!(reply, serde_json::json!([{"success": true}]));
}

#[test]
fn get_seats_reports_capabilities_from_attached_devices() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();

    let seats = query_ipc(&mut fixture, &mut stream, MessageType::GetSeats);
    assert_eq!(
        seats,
        serde_json::json!([{
            "name": "seat0",
            "capabilities": 0,
            "focus": 0,
            "devices": []
        }])
    );
}

#[test]
fn get_inputs_and_seats_return_sway_schema_and_values() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::keyboard("wayland-keyboard-seat0"),
        },
    );
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::pointer("wayland-pointer-seat0"),
        },
    );
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    fixture.niri_state().ipc_refresh_layout();

    let mut stream = UnixStream::connect(socket).unwrap();
    let inputs = query_ipc(&mut fixture, &mut stream, MessageType::GetInputs);
    let mut sway_inputs: Value = serde_json::from_str(&sway_fixture!("inputs.json")).unwrap();
    sway_inputs
        .as_array_mut()
        .unwrap()
        .sort_by_key(|input| input["identifier"].as_str().unwrap().to_owned());
    assert_eq!(inputs, sway_inputs);

    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::DeviceAdded {
            device: TestDevice::libinput_pointer("Logitech G703 LS"),
        },
    );
    let inputs = query_ipc(&mut fixture, &mut stream, MessageType::GetInputs);
    let sway_libinput: Value = serde_json::from_str(&sway_fixture!("inputs-libinput.json")).unwrap();
    let actual_libinput = inputs
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["identifier"] == "1133:16518:Logitech_G703_LS")
        .unwrap();
    assert_eq!(actual_libinput, &sway_libinput[0]);

    let seats = query_ipc(&mut fixture, &mut stream, MessageType::GetSeats);
    let focused = crate::ipc::tree::window_id(fixture.swayward().layout.focus().unwrap().id());
    assert_eq!(
        seats,
        serde_json::json!([{
            "name": "seat0",
            "capabilities": 3,
            "focus": focused,
            "devices": inputs
        }])
    );
}

#[test]
fn exec_does_not_inherit_the_ipc_listener() {
    let (mut fixture, _socket) = ipc_fixture();
    let output = std::env::temp_dir().join(format!("swayward-exec-fds-{}", std::process::id()));
    let temporary = output.with_extension("pending");
    let command = format!(
        "exec sh -c 'ls -l /proc/self/fd > {} && mv {} {}'",
        temporary.display(),
        temporary.display(),
        output.display()
    );
    assert!(crate::command::execute(fixture.niri_state(), &command)[0].success);

    let deadline = Instant::now() + Duration::from_secs(2);
    while !output.exists() {
        assert!(Instant::now() < deadline, "exec did not produce fd listing");
        std::thread::sleep(Duration::from_millis(10));
    }
    let inherited = std::fs::read_to_string(&output).unwrap();
    std::fs::remove_file(output).unwrap();
    assert!(
        !inherited.lines().any(|line| line.contains(" -> socket:[")),
        "exec inherited a socket: {inherited}"
    );
}

#[test]
fn exec_no_startup_id_suppresses_only_the_desktop_token() {
    let (mut fixture, _socket) = ipc_fixture();
    let directory = std::env::temp_dir();
    let suffix = std::process::id();
    let plain = directory.join(format!("swayward-exec-env-plain-{suffix}"));
    let suppressed = directory.join(format!("swayward-exec-env-suppressed-{suffix}"));
    let plain_pending = plain.with_extension("pending");
    let suppressed_pending = suppressed.with_extension("pending");
    for path in [&plain, &suppressed, &plain_pending, &suppressed_pending] {
        let _ = std::fs::remove_file(path);
    }

    // Redirect creates its target before `env` writes anything. Write to a
    // private path and rename it last, so the observed path means that the
    // child is done rather than merely started.
    for (command, output) in [
        (
            format!(
                "exec env > {} && mv {} {}",
                plain_pending.display(),
                plain_pending.display(),
                plain.display()
            ),
            &plain,
        ),
        (
            format!(
                "exec --no-startup-id env > {} && mv {} {}",
                suppressed_pending.display(),
                suppressed_pending.display(),
                suppressed.display()
            ),
            &suppressed,
        ),
    ] {
        assert!(crate::command::execute(fixture.niri_state(), &command)[0].success);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !output.exists() {
            assert!(
                Instant::now() < deadline,
                "{command} did not write its environment"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let read = |path: &std::path::Path| {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter_map(|line| {
                line.split_once('=')
                    .map(|(name, value)| (name.to_owned(), value.to_owned()))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let plain_env = read(&plain);
    let suppressed_env = read(&suppressed);
    std::fs::remove_file(plain).unwrap();
    std::fs::remove_file(suppressed).unwrap();

    let plain_xdg = plain_env.get("XDG_ACTIVATION_TOKEN").unwrap();
    assert!(!plain_xdg.is_empty());
    assert_eq!(plain_env.get("DESKTOP_STARTUP_ID"), Some(plain_xdg));
    assert!(!suppressed_env["XDG_ACTIVATION_TOKEN"].is_empty());
    assert!(!suppressed_env.contains_key("DESKTOP_STARTUP_ID"));
}

#[test]
fn get_bar_config_distinguishes_no_bars_from_an_unknown_id() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();

    assert_eq!(
        query_ipc(&mut fixture, &mut stream, MessageType::GetBarConfig),
        serde_json::json!([])
    );
    assert_eq!(
        query_ipc_with_payload(
            &mut fixture,
            &mut stream,
            MessageType::GetBarConfig,
            "bar-0",
        ),
        serde_json::json!({"success": false, "error": "No bar with that ID"})
    );
}

/// Sway writes this reply as a C string literal rather than serialising it
/// (`sway/sway/ipc-server.c:870`), so it carries spaces a JSON encoder would
/// not produce. The comparison above parses both sides and so cannot see
/// that; SWAY_COMPATIBILITY.md nonetheless called the reply byte-identical,
/// while swayward was sending the compact form.
#[test]
fn get_bar_config_unknown_id_is_byte_identical_to_sway() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBarConfig,
            "bar-0",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        reply,
        r#"{ "success": false, "error": "No bar with that ID" }"#
    );
}

/// A request type outside `MessageType` must get a reply, not a disconnect.
///
/// Sway answers `IPC_SYNC` with `{"success": false}`
/// (`sway/sway/ipc-server.c:919-924`) and keeps the connection open for
/// anything else it does not know (`ipc-server.c:927-929`). Decoding the
/// header through `MessageType::try_from` turned both into a `?`-propagated
/// error that dropped the client with no JSON body.
#[test]
fn unknown_request_types_get_a_structured_reply_and_keep_the_connection() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();

    // IPC_SYNC, sway/include/ipc.h:20.
    stream
        .write_all(&swayward_ipc::wire::encode_raw(11, ""))
        .unwrap();
    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(reply_type, 11);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"success": false})
    );

    stream
        .write_all(&swayward_ipc::wire::encode_raw(9999, ""))
        .unwrap();
    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(reply_type, 9999);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"success": false, "error": "not implemented"})
    );

    // The connection survives both, so a normal request still answers.
    let version = query_ipc(&mut fixture, &mut stream, MessageType::GetVersion);
    assert_eq!(version["variant"], "swayward");
}

#[test]
fn invalid_utf8_command_reply_is_byte_identical_to_sway() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();
    let mut frame = swayward_ipc::wire::encode_raw(0, "");
    frame[6..10].copy_from_slice(&1u32.to_ne_bytes());
    frame.push(0xff);
    stream.write_all(&frame).unwrap();
    stream.set_nonblocking(true).unwrap();

    let payload = b"[ { \"success\": false, \"parse_error\": true, \"error\": \"Unknown\\/invalid command '\xff'\" } ]";
    let mut expected = b"i3-ipc".to_vec();
    expected.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    expected.extend_from_slice(&0u32.to_ne_bytes());
    expected.extend_from_slice(payload);
    let mut actual = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(1);
    while actual.len() < expected.len() {
        fixture.dispatch();
        let mut buf = [0; 4096];
        match stream.read(&mut buf) {
            Ok(0) => panic!("IPC connection closed before a reply"),
            Ok(len) => actual.extend_from_slice(&buf[..len]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("error reading IPC reply: {error}"),
        }
        assert!(Instant::now() < deadline, "timed out waiting for IPC reply");
    }
    assert_eq!(actual, expected);
}

#[test]
fn malformed_frames_disconnect_instead_of_matching_sways_timeout() {
    let (mut fixture, socket) = ipc_fixture();

    for frame in [
        b"i3-ipc\0\0".to_vec(),
        {
            let mut frame = swayward_ipc::wire::encode_raw(0, "nop");
            frame[6..10].copy_from_slice(&10u32.to_ne_bytes());
            frame
        },
        {
            let mut frame = swayward_ipc::wire::encode_raw(0, "");
            frame[6..10].copy_from_slice(&u32::MAX.to_ne_bytes());
            frame
        },
    ] {
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream.write_all(&frame).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        stream.set_nonblocking(true).unwrap();

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            fixture.dispatch();
            let mut byte = [0; 1];
            match stream.read(&mut byte) {
                Ok(0) => break,
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Ok(_) => panic!("malformed frame unexpectedly received a reply"),
                Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("error reading malformed-frame response: {error}"),
            }
            assert!(
                Instant::now() < deadline,
                "malformed frame left the client hanging"
            );
        }
    }
}

#[test]
fn malformed_frames_do_not_hang_or_wedge_the_server() {
    // "It never hangs" is the half of the wire invariant most likely to fail,
    // and these are the frames a buggy client actually sends. Each case uses a
    // fresh connection: the server is entitled to drop a client that sends a
    // malformed frame, but it must not stop serving anyone else.
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();

    let cases: [(&str, Vec<u8>); 4] = [
        ("truncated header", b"i3-ipc\x00\x00".to_vec()),
        ("bad magic", {
            let mut f = swayward_ipc::wire::encode_raw(7, "");
            f[0] = b'X';
            f
        }),
        ("length longer than payload", {
            let mut f = swayward_ipc::wire::encode_raw(0, "");
            f[6..10].copy_from_slice(&64u32.to_ne_bytes());
            f
        }),
        ("payload that is not utf-8", {
            let mut f = swayward_ipc::wire::encode_raw(0, "");
            f[6..10].copy_from_slice(&2u32.to_ne_bytes());
            f.extend_from_slice(&[0xff, 0xfe]);
            f
        }),
    ];

    for (name, frame) in cases {
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream.write_all(&frame).unwrap();
        // Drive the loop. The server may reply or drop this client; either is
        // a valid answer to a frame it cannot parse. What it must not do is
        // block, which would show up as this never returning.
        fixture.dispatch();

        let mut healthy = UnixStream::connect(&socket).unwrap();
        let version = query_ipc(&mut fixture, &mut healthy, MessageType::GetVersion);
        assert_eq!(
            version["variant"], "swayward",
            "server stopped serving after a {name}"
        );
    }
}

/// Oracle: command-fuzz newline-separated, newline-trailing and
/// newline-inside-quotes. Sway rewrites each newline that ends a non-empty
/// line into `;` before parsing a RUN_COMMAND payload, ignoring quotes
/// (sway/sway/ipc-server.c:640-648). The quoted case names the workspace
/// `oracle;newline`, as a direct sway probe shows.
#[test]
fn run_command_splits_on_newlines_like_sway() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1280, 720));
    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = |fixture: &mut Fixture, stream: &mut UnixStream| {
        query_ipc(fixture, stream, MessageType::GetWorkspaces)
            .as_array()
            .unwrap()
            .iter()
            .filter(|workspace| workspace["focused"] == true)
            .map(|workspace| workspace["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };

    for (payload, replies, focused) in [
        (
            "nop first\nworkspace oracle-newline",
            serde_json::json!([{"success": true}, {"success": true}]),
            "oracle-newline",
        ),
        (
            "workspace oracle-trailing\n",
            serde_json::json!([{"success": true}]),
            "oracle-trailing",
        ),
        (
            "workspace \"oracle\nnewline\"",
            serde_json::json!([{"success": true}]),
            "oracle;newline",
        ),
        (
            "nop a\n\nworkspace oracle-blank\n\n",
            serde_json::json!([{"success": true}, {"success": true}]),
            "oracle-blank",
        ),
    ] {
        let reply =
            query_ipc_with_payload(&mut fixture, &mut stream, MessageType::RunCommand, payload);
        assert_eq!(reply, replies, "{payload:?}");
        assert_eq!(workspaces(&mut fixture, &mut stream), [focused], "{payload:?}");
    }
}

/// Oracle: empty/seats, and the seats rows of every captured scenario, where
/// sway's seat focus is the id of GET_TREE's single focused node: a workspace
/// when it is empty, a split container after `focus parent`
/// (`sway/sway/ipc-json.c:1238`, `seat_get_focus`).
#[test]
fn get_seats_focus_is_the_focused_tree_node() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1280, 720));
    let mut stream = UnixStream::connect(socket).unwrap();
    fn focused_id(node: &Value) -> Option<i64> {
        if node["focused"] == true {
            return node["id"].as_i64();
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .filter_map(|key| node[key].as_array())
            .flatten()
            .find_map(focused_id)
    }
    let check = |fixture: &mut Fixture, stream: &mut UnixStream, expected_type: &str| {
        fixture.niri_state().ipc_refresh_layout();
        let tree = query_ipc(fixture, stream, MessageType::GetTree);
        let seats = query_ipc(fixture, stream, MessageType::GetSeats);
        let focused = focused_id(&tree).unwrap();
        let node = crate::ipc::server::find_node_by_id(&tree, focused).unwrap();
        assert_eq!(node["type"], expected_type, "{tree:#}");
        assert_eq!(seats[0]["focus"], focused, "{seats:#}");
    };

    check(&mut fixture, &mut stream, "workspace");

    let client = fixture.add_client();
    for _ in 0..2 {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    check(&mut fixture, &mut stream, "con");

    // splitv wraps the focused window, so one `focus parent` selects the new
    // split container.
    assert!(crate::command::execute(fixture.niri_state(), "splitv")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    check(&mut fixture, &mut stream, "con");
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    let focused = crate::ipc::server::find_node_by_id(&tree, focused_id(&tree).unwrap()).unwrap();
    assert!(
        !focused["nodes"].as_array().unwrap().is_empty(),
        "focus parent selects a split container: {focused:#}"
    );
}

/// A libinput device whose configuration queries return fixed raw values.
#[derive(Clone)]
struct FakeLibinput {
    send_events: u32,
    tap_fingers: i32,
    tap: u32,
    tap_map: u32,
    tap_drag: u32,
    tap_drag_lock: u32,
    accel: Option<(f64, u32)>,
    natural_scroll: Option<bool>,
    left_handed: Option<bool>,
    click_methods: u32,
    click_method: u32,
    clickfinger_map: u32,
    middle_emulation: Option<u32>,
    scroll_methods: u32,
    scroll_method: u32,
    scroll_button: u32,
    scroll_button_lock: u32,
    dwt: Option<u32>,
    dwtp: Option<u32>,
    calibration: Option<[f32; 6]>,
}

impl crate::input::backend_ext::LibinputQuery for FakeLibinput {
    fn send_events_mode(&self) -> u32 {
        self.send_events
    }
    fn tap_finger_count(&self) -> i32 {
        self.tap_fingers
    }
    fn tap_enabled(&self) -> u32 {
        self.tap
    }
    fn tap_button_map(&self) -> u32 {
        self.tap_map
    }
    fn tap_drag_enabled(&self) -> u32 {
        self.tap_drag
    }
    fn tap_drag_lock_enabled(&self) -> u32 {
        self.tap_drag_lock
    }
    fn accel(&self) -> Option<(f64, u32)> {
        self.accel
    }
    fn natural_scroll(&self) -> Option<bool> {
        self.natural_scroll
    }
    fn left_handed(&self) -> Option<bool> {
        self.left_handed
    }
    fn click_methods(&self) -> u32 {
        self.click_methods
    }
    fn click_method(&self) -> u32 {
        self.click_method
    }
    fn clickfinger_button_map(&self) -> u32 {
        self.clickfinger_map
    }
    fn middle_emulation(&self) -> Option<u32> {
        self.middle_emulation
    }
    fn scroll_methods(&self) -> u32 {
        self.scroll_methods
    }
    fn scroll_method(&self) -> u32 {
        self.scroll_method
    }
    fn scroll_button(&self) -> u32 {
        self.scroll_button
    }
    fn scroll_button_lock(&self) -> u32 {
        self.scroll_button_lock
    }
    fn dwt(&self) -> Option<u32> {
        self.dwt
    }
    fn dwtp(&self) -> Option<u32> {
        self.dwtp
    }
    fn calibration_matrix(&self) -> Option<[f32; 6]> {
        self.calibration
    }
}

/// A clickpad as libinput reports one: tapping, two click methods, two-finger
/// and edge scrolling, disable-while-typing. The expected object follows
/// sway's describe_libinput_device at 1.12, cited per field below. No sway
/// capture backs it yet: oracle row pending hardware capture
/// (review2-core-get-inputs-libinput-touchpad).
#[test]
fn libinput_object_has_every_sway_field_for_touchpads_and_mice() {
    let touchpad = FakeLibinput {
        send_events: 0,
        tap_fingers: 3,
        tap: 1,
        tap_map: 0,
        tap_drag: 1,
        tap_drag_lock: 0,
        accel: Some((0.25, 2)),
        natural_scroll: Some(true),
        left_handed: Some(false),
        click_methods: 1 | 2,
        click_method: 2,
        clickfinger_map: 1,
        middle_emulation: Some(0),
        scroll_methods: 1 | 2,
        scroll_method: 1,
        scroll_button: 0,
        scroll_button_lock: 0,
        dwt: Some(1),
        dwtp: None,
        calibration: None,
    };
    assert_eq!(
        crate::input::backend_ext::describe_libinput_device(&touchpad),
        serde_json::json!({
            "send_events": "enabled",          // ipc-json.c:903-916
            "tap": "enabled",                  // :918-928, finger count > 0
            "tap_button_map": "lrm",           // :930-940
            "tap_drag": "enabled",             // :942-952
            "tap_drag_lock": "disabled",       // :954-969
            "accel_speed": 0.25,               // :972-975
            "accel_profile": "adaptive",       // :977-995
            "natural_scroll": "enabled",       // :998-1005
            "left_handed": "disabled",         // :1007-1014
            "click_method": "clickfinger",     // :1016-1031, any click method
            "clickfinger_button_map": "lmr",   // :1033-1043
            "middle_emulation": "disabled",    // :1046-1058
            "scroll_method": "two_finger",     // :1060-1078; no ON_BUTTON_DOWN,
                                               // so no scroll_button (:1080-1095)
            "dwt": "enabled",                  // :1098-1109; dwtp unavailable
        })
    );

    // A device without tapping, click methods, dwt or a matrix keeps exactly
    // the G703 mouse fields the pinned oracle captured
    // (sway-ipc/fixtures/inputs-libinput.json).
    let mouse = FakeLibinput {
        tap_fingers: 0,
        accel: Some((0.0, 2)),
        natural_scroll: Some(false),
        click_methods: 0,
        scroll_methods: 4,
        scroll_method: 0,
        scroll_button: 274,
        dwt: None,
        ..touchpad.clone()
    };
    let fixture: Value =
        serde_json::from_str(&sway_fixture!("inputs-libinput.json")).unwrap();
    assert_eq!(
        crate::input::backend_ext::describe_libinput_device(&mouse),
        fixture[0]["libinput"]
    );

    // Values sway does not name print "unknown"; a matrix is six doubles; a
    // sticky drag lock is named (ipc-json.c:962-966, :1124-1134).
    let unusual = FakeLibinput {
        send_events: 7,
        tap_drag_lock: 2,
        calibration: Some([1.0, 0.0, 0.0, 0.0, 1.0, 0.5]),
        dwtp: Some(0),
        ..touchpad
    };
    let unusual = crate::input::backend_ext::describe_libinput_device(&unusual);
    assert_eq!(unusual["send_events"], "unknown");
    assert_eq!(unusual["tap_drag_lock"], "enabled_sticky");
    assert_eq!(unusual["dwtp"], "disabled");
    assert_eq!(
        unusual["calibration_matrix"],
        serde_json::json!([1.0, 0.0, 0.0, 0.0, 1.0, 0.5])
    );
}

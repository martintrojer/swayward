#[test]
fn ipc_refresh_without_a_seat_keyboard_does_not_panic() {
    let (mut fixture, _) = ipc_fixture();
    fixture.swayward().seat.remove_keyboard();

    fixture.niri_state().ipc_refresh_keyboard_layout_index();
    fixture.niri_state().ipc_keyboard_layouts_changed();
}

#[test]
fn get_inputs_and_seats_return_sway_schema_and_values() {
    let mut fixture = Fixture::new();
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
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
    let mut sway_inputs: Value = serde_json::from_str(sway_fixture!("inputs.json")).unwrap();
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
    let sway_libinput: Value = serde_json::from_str(sway_fixture!("inputs-libinput.json")).unwrap();
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
fn invalid_utf8_command_gets_a_structured_json_failure() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();
    let mut frame = swayward_ipc::wire::encode_raw(0, "");
    frame[6..10].copy_from_slice(&1u32.to_ne_bytes());
    frame.push(0xff);
    stream.write_all(&frame).unwrap();

    let (reply_type, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(reply_type, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([{
            "success": false,
            "error": "command is not valid UTF-8",
            "parse_error": true,
        }])
    );
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

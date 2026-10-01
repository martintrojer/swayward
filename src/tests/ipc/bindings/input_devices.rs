#[test]
fn mouse_input_device_binding_prefers_exact_device_and_wildcard_matches_another() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "rename workspace to wildcard-mouse"; }
            MouseLeft input-device="0:0:first_mouse" { command "rename workspace to exact-mouse"; }
            MouseRight input-device="0:0:first_mouse" { command "rename workspace to wrong-mouse"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("first mouse"),
            0x110,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("second mouse"),
            0x111,
            pressed,
        );
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("second mouse"),
            0x110,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard-mouse")
    );
}

#[test]
fn device_identifier_matches_sways_libinput_format() {
    use crate::input::backend_ext::NiriInputDevice as _;

    let device = TestDevice::keyboard("  keyboard with spaces  ");
    assert_eq!(device.sway_identifier(), "0:0:keyboard_with_spaces");
}

#[test]
fn input_device_binding_prefers_exact_device_and_wildcard_matches_another() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            x { command "rename workspace to wildcard"; }
            x input-device="0:0:first_keyboard" { command "rename workspace to exact"; }
            z input-device="0:0:first_keyboard" { command "rename workspace to wrong"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("first keyboard"),
            53,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact")
    );

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("second keyboard"),
            52,
            pressed,
        );
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong")
    );

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("second keyboard"),
            53,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );
}

fn set_xkb_layout(fixture: &mut Fixture, layout: u32) {
    let keyboard = fixture.swayward().seat.get_keyboard().unwrap();
    keyboard.with_xkb_state(fixture.niri_state(), |mut context| {
        context.set_layout(smithay::input::keyboard::Layout(layout));
    });
}

#[test]
fn group_binding_overrides_wildcard_only_in_its_active_group() {
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { xkb { layout "us,ru,us"; }; }; }
        binds {
            x { command "rename workspace to wildcard"; };
            Group2+x { command "rename workspace to exact"; };
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );

    set_xkb_layout(&mut fixture, 1);
    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact")
    );
    set_xkb_layout(&mut fixture, 2);
    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );
}

#[test]
fn translated_keysym_binding_fires_in_its_xkb_layout() {
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { xkb { layout "us,ru"; }; }; }
        binds { Cyrillic_ze { command "rename workspace to cyrillic"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[33]]);
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("cyrillic")
    );

    set_xkb_layout(&mut fixture, 1);
    type_key_chords(&mut fixture, &[&[33]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("cyrillic")
    );
}

fn add_tiled_windows(fixture: &mut Fixture, client: super::client::ClientId, count: usize) {
    for _ in 0..count {
        windows::map_window(fixture, client, windows::WindowSpec::default());
    }
}

#[test]
fn titlebar_wheel_binding_takes_precedence_over_tab_focus() {
    let config = swayward_config::Config::parse_mem(
        r#"layout { gaps 0; }
        binds {
            WheelScrollDown mouse-regions="titlebar" { command "mark bound"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    add_tiled_windows(&mut fixture, client, 3);
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus left")[0].success);
    let focused = fixture.swayward().layout.focus().unwrap().id();
    fixture.niri_state().move_cursor((100., 10.).into());
    pointer_axis(&mut fixture, 0., 120.);

    let swayward = fixture.swayward();
    assert!(swayward
        .marks_by_window
        .values()
        .chain(swayward.marks_by_container.values())
        .flatten()
        .any(|mark| mark == "bound"));
    assert_eq!(swayward.layout.focus().unwrap().id(), focused);
}

#[test]
fn pointer_button_binding_requires_the_configured_rendered_region() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace startup"; }
            MouseLeft mouse-regions="contents" { command "workspace clicked"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("clicked")
        .is_none());
}

/// Startup continues without a seat keyboard when no keymap compiles, so every
/// live input and reload path must tolerate its absence instead of panicking.
#[test]
fn input_and_reload_without_a_seat_keyboard_do_not_panic() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    fixture.swayward().seat.remove_keyboard();
    assert!(fixture.swayward().seat.get_keyboard().is_none());

    pointer_motion_absolute(&mut fixture, 100., 100.);
    pointer_button(&mut fixture, 0x110, true);
    pointer_button(&mut fixture, 0x110, false);
    pointer_axis(&mut fixture, 0., 120.);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    fixture.niri_state().do_action(
        swayward_config::Action::SwitchLayout(swayward_ipc::LayoutSwitchTarget::Next),
        false,
    );

    let mut config = swayward_config::Config::default();
    config.input.keyboard.repeat_rate = 42;
    config.input.keyboard.xkb.layout = "us,de".into();
    fixture.niri_state().reload_config(Ok(config));
    assert!(fixture.swayward().seat.get_keyboard().is_none());
}

/// Oracle: sway-ipc-oracle 5fb0576, events scenarios binding_mouse_buttons
/// and binding_mouse_wheel, captured from sway 88869399. Sway names
/// BTN_LEFT..BTN_LEFT+8 `buttonN` and its scroll pseudo-buttons, KEY_MAX + 1..4,
/// by their raw xkb keysym name (sway/sway/ipc-server.c:434-439). The pinned
/// oracle predates the capture, so the captured symbols are copied here; every
/// other field matches the capture byte for byte.
#[test]
fn mouse_binding_events_name_buttons_and_wheel_like_sway() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "nop left"; }
            MouseMiddle { command "nop middle"; }
            MouseRight { command "nop right"; }
            MouseBack { command "nop back"; }
            MouseForward { command "nop forward"; }
            WheelScrollUp { command "nop wheel-up"; }
            WheelScrollDown { command "nop wheel-down"; }
            WheelScrollLeft { command "nop wheel-left"; }
            WheelScrollRight { command "nop wheel-right"; }
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture_with_config(config);
    fixture.add_output(1, (1280, 720));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["binding"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let mut remainder = Vec::new();
    let mut next_binding = |fixture: &mut Fixture| {
        let ((event_type, payload), rest) =
            read_ipc_reply_with_remainder(fixture, &mut subscriber, std::mem::take(&mut remainder));
        remainder = rest;
        assert_eq!(event_type, EVENT_BINDING, "{payload}");
        serde_json::from_str::<Value>(&payload).unwrap()
    };
    let sway = |command: &str, symbol: &str| {
        serde_json::json!({
            "change": "run",
            "binding": {
                "command": command,
                "event_state_mask": [],
                "input_code": 0,
                "input_codes": [],
                "input_type": "mouse",
                "symbol": symbol,
                "symbols": [symbol],
            }
        })
    };

    for (button, (command, symbol)) in [0x110, 0x112, 0x111, 0x113, 0x114].into_iter().zip([
        ("nop left", "button1"),
        ("nop middle", "button3"),
        ("nop right", "button2"),
        ("nop back", "button4"),
        ("nop forward", "button5"),
    ]) {
        pointer_button(&mut fixture, button, true);
        pointer_button(&mut fixture, button, false);
        assert_eq!(next_binding(&mut fixture), sway(command, symbol));
    }
    for ((horizontal, vertical), (command, symbol)) in
        [(0., -120.), (0., 120.), (-120., 0.), (120., 0.)]
            .into_iter()
            .zip([
                ("nop wheel-up", "0x00000300"),
                ("nop wheel-down", "0x00000301"),
                ("nop wheel-left", "0x00000302"),
                ("nop wheel-right", "0x00000303"),
            ])
    {
        pointer_axis(&mut fixture, horizontal, vertical);
        assert_eq!(next_binding(&mut fixture), sway(command, symbol));
    }
}

/// Sway types a tap-capable libinput pointer as "touchpad"
/// (`input_device_get_type`, sway/sway/input/input-manager.c:93-117) and
/// reports each device's own configured scroll factor
/// (sway/sway/ipc-json.c:1189-1197). Both kinds give the seat the pointer
/// capability (sway/sway/input/seat.c:613-615).
#[test]
fn get_inputs_types_touchpads_and_reports_their_own_scroll_factor() {
    let config = swayward_config::Config::parse_mem(
        r#"input {
            mouse { scroll-factor 2.0; }
            touchpad { scroll-factor 0.5; }
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture_with_config(config);
    for device in [
        TestDevice::libinput_pointer("test mouse"),
        TestDevice::touchpad("test touchpad"),
    ] {
        fixture.niri_state().process_input_event::<TestInput>(
            smithay::backend::input::InputEvent::DeviceAdded { device },
        );
    }

    let mut query = UnixStream::connect(&socket).unwrap();
    let inputs = query_ipc(&mut fixture, &mut query, MessageType::GetInputs);
    let by_name = |name: &str| {
        inputs
            .as_array()
            .unwrap()
            .iter()
            .find(|input| input["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing from {inputs}"))
            .clone()
    };
    let mouse = by_name("test mouse");
    let touchpad = by_name("test touchpad");
    assert_eq!(mouse["type"], "pointer");
    assert_eq!(mouse["scroll_factor"], 2.0);
    assert_eq!(touchpad["type"], "touchpad");
    assert_eq!(touchpad["scroll_factor"], 0.5);

    let seats = query_ipc(&mut fixture, &mut query, MessageType::GetSeats);
    assert_eq!(seats[0]["capabilities"].as_u64().unwrap() & 1, 1);
}

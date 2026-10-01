#[test]
fn ordinary_modified_keysym_bind_still_matches() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+Return { command "rename workspace to modified"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[133, 36]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("modified")
        .is_some());
}

#[test]
fn translated_keysym_uses_post_transition_consumed_modifiers() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Alt+at { command "rename workspace to translated"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[64, 50, 11]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("translated")
        .is_some());
}

#[test]
fn consumed_shift_still_matches_an_unchanged_raw_keysym() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+Shift+BackSpace { command "rename workspace to shifted-backspace"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[133, 50, 22]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("shifted-backspace")
        .is_some());
}

#[test]
fn bindcode_uses_the_xkb_keycode_from_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { "code:39" { command "rename workspace to bindcode"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("bindcode")
        .is_some());
}

#[test]
fn release_key_binding_dispatches_only_on_release_through_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    key_event(&mut fixture, 53, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn another_key_cancels_a_held_release_binding_without_an_ipc_event() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "nop release"; }; }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1280, 720));
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    key_event(&mut fixture, 53, true);
    key_event(&mut fixture, 52, true);
    key_event(&mut fixture, 53, false);
    key_event(&mut fixture, 52, false);
    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::SwayBinding {
            command: "sentinel".into(),
            event_state_mask: vec![],
            input_codes: vec![],
            input_code: 0,
            symbols: vec!["t".into()],
            symbol: Some("t".into()),
            input_type: "keyboard".into(),
        },
    );

    let mut commands = Vec::new();
    loop {
        let (message_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
        assert_eq!(message_type, (1 << 31) | 5);
        let command = serde_json::from_str::<Value>(&payload).unwrap()["binding"]["command"]
            .as_str()
            .unwrap()
            .to_owned();
        commands.push(command.clone());
        if command == "sentinel" {
            break;
        }
    }
    assert_eq!(commands, ["sentinel"]);
}

#[test]
fn release_key_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            x { command "mode other"; }
            x release=true { command "workspace key-released"; }
        }
        mode "other" { y { command "nop"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_survives_config_reload_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace key-released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    super::i3_conformance::reload_test_config(&mut fixture, "", "font monospace\n").unwrap();
    assert_eq!(fixture.swayward().binding_mode, "default");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_is_not_replaced_by_the_new_modes_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace original-release"; }; }
        mode "other" { x release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("original-release")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "mode other"; }
            MouseLeft release=true { command "workspace mouse-released"; }
        }
        mode "other" { MouseLeft release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("mouse-released")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_dispatches_only_on_release() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { MouseLeft release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    pointer_button(&mut fixture, 0x110, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn pointer_button_event_dispatches_a_real_mouse_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace startup"; }
            MouseLeft { command "workspace clicked"; }
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
        .is_some());
}

#[test]
fn binding_modes_switch_binds_emit_events_and_list_over_ipc() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+R { command "mode resize"; }; }
        mode "resize" {
            Super+1 { command "workspace 7"; };
            Escape { command "mode default"; };
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode","binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/mode.resize.json")).unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    type_key_chords(&mut fixture, &[&[133, 10]]);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 5);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/binding.run.json")).unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$binding",
    );

    let swayward = fixture.swayward();
    // The bind switched to workspace 7, which is what this asserts. It is not
    // necessarily first: sway sorts numbered workspaces numerically
    // (sway/sway/tree/output.c:387-405), so the startup workspace 1 precedes it.
    assert!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .any(|workspace| workspace.num == 7 && workspace.focused)
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    let expected: Value = serde_json::from_str(&sway_fixture!("events/mode.default.json")).unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    let mut query = UnixStream::connect(socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "resize"])
    );

    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"name": "default"})
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    let state = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(state, serde_json::json!({"name": "resize"}));
    assert_eq!(state.as_object().unwrap().len(), 1);
}

#[test]
fn runtime_mode_definition_with_set_creates_a_switchable_pango_mode() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode --pango_markup created set $destination workspace-7",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "default");
    assert_eq!(
        fixture.swayward().sway_variables,
        [("$destination".into(), "workspace-7".into())]
    );
    let config = fixture.swayward().config.borrow();
    let created = config
        .binding_modes
        .iter()
        .find(|mode| mode.name == "created")
        .unwrap();
    assert!(created.pango_markup);
    assert!(created.binds.0.is_empty());
    drop(config);

    let mut query = UnixStream::connect(&socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "created"])
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode created");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change":"created","pango_markup":true})
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace $destination")[0].success);
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-7".into())
    );

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode inline set $next workspace-8; workspace $next",
    );
    assert!(outcome.iter().all(|result| result.success), "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-8".into())
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode missing");
    assert!(!outcome[0].success);
    assert_eq!(outcome[0].error.as_deref(), Some("Unknown mode `missing'"));
}

/// Gesture binds are the only nested subcommands still refused, top level or
/// nested: swayward has no gesture command-binding table. The refusal happens
/// at parse time, so the named mode is not created either.
#[test]
fn runtime_mode_definition_rejects_only_gesture_binding_subcommands() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for subcommand in ["bindgesture swipe:3:left nop", "unbindgesture swipe:3:left"] {
        let command = format!("mode blocked {subcommand}");
        let outcome = crate::command::execute(fixture.niri_state(), &command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("gesture events have no sway command-binding model")
        );
    }
    assert!(fixture
        .swayward()
        .config
        .borrow()
        .binding_modes
        .iter()
        .all(|mode| mode.name != "blocked"));
}

/// `mode <name> bindsym` inserts into the named mode without switching to it
/// (`sway/sway/commands/mode.c:69-84`). The binding must fire once that mode
/// is active, stay silent in the default mode, and be discarded by reload.
#[test]
fn runtime_nested_mode_bindsym_fires_only_in_that_mode_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let added = crate::command::execute(
        fixture.niri_state(),
        "mode nested bindsym Mod4+a workspace nested-bound",
    );
    assert!(added[0].success, "{added:?}");
    // The nested form defines the mode but does not enter it.
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Inactive in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace still-default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("still-default")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-bound")
    );

    // Unbinding through the nested form removes it again.
    let removed = crate::command::execute(fixture.niri_state(), "mode nested unbindsym Mod4+a");
    assert!(removed[0].success, "{removed:?}");
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "mode nested bindsym Mod4+a workspace should-not-survive",
        )[0]
        .success
    );
    let config =
        swayward_config::Config::parse_mem(r#"mode "nested" { x { command "nop"; }; }"#).unwrap();
    fixture.niri_state().reload_config(Ok(config));
    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-check")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("reload-check")
    );
}

/// The keycode and switch nested forms reach the same tables as their
/// top-level counterparts, and target the named mode rather than the active
/// one.
#[test]
fn runtime_nested_mode_bindcode_and_bindswitch_target_the_named_mode() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    for command in [
        "mode nested bindcode 39 workspace nested-code",
        "mode nested bindswitch lid:on workspace nested-switch",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Neither fires in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace default-still")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-still")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-code")
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-switch")
    );

    for command in [
        "mode nested unbindcode 39",
        "mode nested unbindswitch lid:on",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );
}

/// A nested bind aimed at the mode the compositor is already in must not
/// leak into the default table, and a nested bind issued while a mode is
/// active must land in the named mode rather than the active one.
#[test]
fn runtime_nested_mode_bindsym_ignores_the_active_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds { Mod4+a { command "workspace default-mode"; }; }
mode "other" { Mod4+b { command "nop"; }; }
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode elsewhere bindsym Mod4+a workspace elsewhere-bound",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "other");

    // The active mode did not receive the binding.
    assert!(crate::command::execute(fixture.niri_state(), "workspace untouched")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("untouched")
    );

    // Neither did the default table.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-mode")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode elsewhere")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("elsewhere-bound")
    );
}

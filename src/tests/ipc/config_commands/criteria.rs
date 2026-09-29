#[test]
fn command_bind_executes_the_sway_command_path() {
    let config = swayward_config::Config::parse_mem(
        "binds { Super+1 repeat=false { command \"workspace 7\"; }; }",
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::Keyboard {
            event: TestKeyEvent {
                device: TestDevice::keyboard("test keyboard"),
                key: 133,
                count: 1,
                state: smithay::backend::input::KeyState::Pressed,
            },
        },
    );
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .logo
    );
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::Keyboard {
            event: TestKeyEvent {
                device: TestDevice::keyboard("test keyboard"),
                key: 10,
                count: 2,
                state: smithay::backend::input::KeyState::Pressed,
            },
        },
    );

    let swayward = fixture.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(workspaces[0].num, 7);
}

#[test]
fn empty_workspace_commands_return_sway_failures() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for (command, error) in [
        (
            "focus floating",
            "Failed to find a floating container in workspace.",
        ),
        (
            "focus mode_toggle",
            "Failed to find a floating container in workspace.",
        ),
        (
            "focus tiling",
            "Failed to find a tiling container in workspace.",
        ),
        ("resize grow height 10 px", "Cannot resize nothing"),
        ("resize grow width 10 px", "Cannot resize nothing"),
        ("resize grow width 10 px or 5 ppt", "Cannot resize nothing"),
        ("resize set 50 ppt 50 ppt", "Cannot resize nothing"),
        ("resize shrink height 10 px", "Cannot resize nothing"),
        ("resize shrink width 10 px", "Cannot resize nothing"),
        ("resize invalid", "Cannot resize nothing"),
        ("scratchpad show", "Scratchpad is empty"),
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert_eq!(outcome.len(), 1, "{command}: {outcome:?}");
        assert!(!outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(outcome[0].error.as_deref(), Some(error), "{command}");
    }
}

#[test]
fn empty_workspace_move_and_floating_commands_match_sway_errors() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for (command, parse_error, error) in [
        (
            "move scratchpad",
            true,
            "Can't move an empty workspace to the scratchpad",
        ),
        ("floating toggle", true, "Can't float an empty workspace"),
        ("move left", false, "Cannot move workspaces in a direction"),
        ("move right", false, "Cannot move workspaces in a direction"),
        ("move up", false, "Cannot move workspaces in a direction"),
        ("move down", false, "Cannot move workspaces in a direction"),
        (
            "move container to workspace 2",
            false,
            "Can't move an empty workspace",
        ),
    ] {
        assert_eq!(
            crate::command::execute(fixture.niri_state(), command),
            [swayward_ipc::CommandOutcome {
                success: false,
                error: Some(error.into()),
                parse_error: Some(parse_error),
            }],
            "{command}"
        );
    }
}

#[test]
fn focus_floating_succeeds_when_a_floating_window_exists() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
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
    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus floating")[0].success);
}

#[test]
fn empty_workspace_command_parse_errors_match_sway() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for (command, parse_error, error) in [
        ("border 1pixel", true, "Only views can have borders"),
        ("border none", true, "Only views can have borders"),
        ("resize grow width 10 px", true, "Cannot resize nothing"),
        ("scratchpad show", true, "Scratchpad is empty"),
        ("sticky toggle", false, "No current container"),
        ("mark oracle", true, "Only containers can have marks"),
    ] {
        assert_eq!(
            crate::command::execute(fixture.niri_state(), command),
            [swayward_ipc::CommandOutcome {
                success: false,
                error: Some(error.into()),
                parse_error: Some(parse_error),
            }],
            "{command}"
        );
    }
}

#[test]
fn workspace_root_cannot_be_marked() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
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
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);

    assert_eq!(
        crate::command::execute(fixture.niri_state(), "mark oracle"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Only containers can have marks".into()),
            parse_error: Some(true),
        }]
    );
}

#[test]
fn criteria_with_no_matches_returns_sway_failure() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    assert_eq!(
        crate::command::execute(fixture.niri_state(), r#"[app_id="missing"] nop"#),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No matching node.".into()),
            parse_error: Some(false),
        }]
    );
}

#[test]
fn portable_security_context_criteria_match_exactly_the_intended_windows() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let sandboxed =
        fixture.add_client_with_security_context(Some(crate::swayward::SecurityContextMetadata {
            sandbox_engine: Some("flatpak".into()),
            app_id: Some("org.example.Sandbox".into()),
            instance_id: Some("instance-1".into()),
        }));
    let other =
        fixture.add_client_with_security_context(Some(crate::swayward::SecurityContextMetadata {
            sandbox_engine: Some("snap".into()),
            app_id: Some("org.example.Other".into()),
            instance_id: Some("instance-2".into()),
        }));
    let unrestricted = fixture.add_client();
    map_test_window(&mut fixture, sandboxed, "same-app-id");
    map_test_window(&mut fixture, other, "same-app-id");
    map_test_window(&mut fixture, unrestricted, "same-app-id");

    for (criterion, expected_marks) in [
        (r#"sandbox_engine="^flatpak$""#, 1),
        (r#"sandbox_app_id="^org\.example\.Sandbox$""#, 1),
        (r#"sandbox_instance_id="^instance-1$""#, 1),
        // Missing metadata is not coerced to an empty string: this must not
        // match the unrestricted window.
        (r#"sandbox_engine="^$""#, 0),
    ] {
        let command = format!(r#"[{criterion}] mark --add portable"#);
        let outcome = crate::command::execute(fixture.niri_state(), &command);
        if expected_marks == 0 {
            assert!(!outcome[0].success, "{criterion}: {outcome:?}");
            assert_eq!(outcome[0].error.as_deref(), Some("No matching node."));
        } else {
            assert!(outcome[0].success, "{criterion}: {outcome:?}");
        }
        assert_eq!(
            fixture
                .swayward()
                .marks_by_window
                .values()
                .filter(|marks| marks.iter().any(|mark| mark == "portable"))
                .count(),
            expected_marks,
            "{criterion}"
        );
        fixture.swayward().marks_by_window.clear();
    }
}

#[test]
fn xdg_toplevel_tags_match_only_the_intended_windows() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let tagged = fixture.add_client();
    let other = fixture.add_client();

    let (tagged_surface, tagged_toplevel) = {
        let window = fixture.client(tagged).create_window();
        window.xdg_toplevel.set_app_id("same-app-id".into());
        (window.surface.clone(), window.xdg_toplevel.clone())
    };
    fixture
        .client(tagged)
        .set_toplevel_tag(&tagged_toplevel, "Editor");
    fixture.client(tagged).window(&tagged_surface).commit();
    fixture.roundtrip(tagged);
    let window = fixture.client(tagged).window(&tagged_surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(tagged);
    map_test_window(&mut fixture, other, "same-app-id");
    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"for_window [tag="^Other$"] mark --add updated-tag"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let other_toplevel = fixture.client(other).state.windows[0].xdg_toplevel.clone();
    fixture
        .client(other)
        .set_toplevel_tag(&other_toplevel, "Other");
    fixture.roundtrip(other);
    assert_eq!(
        fixture
            .swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "updated-tag"))
            .count(),
        1
    );

    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[tag="^Editor$"] mark --add tagged"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        fixture
            .swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "tagged"))
            .count(),
        1
    );

    for criterion in [r#"tag="^Missing$""#, r#"tag="^$""#] {
        let outcome = crate::command::execute(
            fixture.niri_state(),
            &format!(r#"[{criterion}] mark should-not-appear"#),
        );
        assert!(!outcome[0].success, "{criterion}: {outcome:?}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("No matching node."),
            "{criterion}"
        );
    }

    fixture.swayward().marks_by_window.clear();
    let tagged_window = fixture
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| mapped.tag().as_deref() == Some("Editor"))
        .unwrap()
        .1
        .window
        .clone();
    fixture.swayward().layout.activate_window(&tagged_window);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[tag="__focused__"] mark --add focused-tag"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        fixture
            .swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "focused-tag"))
            .count(),
        1
    );
}

#[test]
fn x11_only_criteria_fail_before_matching_wayland_windows() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "same-as-x11-class");

    for (criterion, error) in [
        (
            r#"class="same-as-x11-class""#,
            "X11-only criterion 'class' is unsupported",
        ),
        (
            r#"instance=".*""#,
            "X11-only criterion 'instance' is unsupported",
        ),
        ("id=1", "X11-only criterion 'id' is unsupported"),
        (
            r#"window_role=".*""#,
            "X11-only criterion 'window_role' is unsupported",
        ),
        (
            "window_type=normal",
            "X11-only criterion 'window_type' is unsupported",
        ),
    ] {
        let outcome = crate::command::execute(
            fixture.niri_state(),
            &format!(r#"[{criterion}] mark should-not-appear"#),
        );
        assert!(!outcome[0].success, "{criterion}: {outcome:?}");
        assert_eq!(outcome[0].parse_error, Some(true), "{criterion}");
        assert_eq!(outcome[0].error.as_deref(), Some(error), "{criterion}");
        assert!(fixture.swayward().marks_by_window.is_empty(), "{criterion}");
    }
}

#[test]
fn criteria_global_settings_require_matches_and_run_once_per_match() {
    use swayward_config::layout::{FocusWrapping, SmartBorders};
    use swayward_config::misc::PopupDuringFullscreen;

    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    let before = fixture.swayward().config.borrow().layout.focus_wrapping;
    assert_eq!(
        crate::command::execute(
            fixture.niri_state(),
            r#"[app_id="missing"] focus_wrapping toggle"#,
        ),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No matching node.".into()),
            parse_error: Some(false),
        }]
    );
    assert_eq!(
        fixture.swayward().config.borrow().layout.focus_wrapping,
        before,
        "zero matches must not mutate global state"
    );

    let client = fixture.add_client();
    map_test_window(&mut fixture, client, "matched");
    let command = concat!(
        r#"[app_id="matched"] floating_minimum_size 111 x 77, "#,
        "floating_maximum_size 999 x 777, focus_wrapping toggle, ",
        "force_focus_wrapping toggle, popup_during_fullscreen ignore, ",
        "smart_borders no_gaps, workspace_auto_back_and_forth toggle",
    );
    let outcomes = crate::command::execute(fixture.niri_state(), command);
    assert!(
        outcomes.iter().all(|outcome| outcome.success),
        "{outcomes:?}"
    );
    {
        let config = fixture.swayward().config.borrow();
        assert_eq!(config.layout.floating_minimum_size.width, 111);
        assert_eq!(config.layout.floating_minimum_size.height, 77);
        assert_eq!(config.layout.floating_maximum_size.width, 999);
        assert_eq!(config.layout.floating_maximum_size.height, 777);
        assert_eq!(config.layout.focus_wrapping, FocusWrapping::Force);
        assert_eq!(config.layout.smart_borders, SmartBorders::NoGaps);
        assert!(config.input.workspace_auto_back_and_forth);
        assert_eq!(
            config.popup_during_fullscreen,
            PopupDuringFullscreen::Ignore
        );
    }

    for _ in 0..2 {
        map_test_window(&mut fixture, client, "matched");
    }
    {
        let mut config = fixture.swayward().config.borrow_mut();
        config.layout.focus_wrapping = FocusWrapping::Yes;
        config.layout.smart_borders = SmartBorders::On;
        config.input.workspace_auto_back_and_forth = true;
    }
    crate::command::reset_global_setting_executions();
    for (index, (command, expected_wrapping, expected_smart_borders, expected_back_and_forth)) in [
        (
            r#"[app_id="matched"] focus_wrapping toggle"#,
            FocusWrapping::No,
            SmartBorders::On,
            true,
        ),
        (
            r#"[app_id="matched"] force_focus_wrapping toggle"#,
            FocusWrapping::Force,
            SmartBorders::On,
            true,
        ),
        (
            r#"[app_id="matched"] smart_borders toggle"#,
            FocusWrapping::Force,
            SmartBorders::Off,
            true,
        ),
        (
            r#"[app_id="matched"] workspace_auto_back_and_forth toggle"#,
            FocusWrapping::Force,
            SmartBorders::Off,
            false,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let config = fixture.swayward().config.borrow();
        assert_eq!(config.layout.focus_wrapping, expected_wrapping, "{command}");
        assert_eq!(
            config.layout.smart_borders, expected_smart_borders,
            "{command}"
        );
        assert_eq!(
            config.input.workspace_auto_back_and_forth, expected_back_and_forth,
            "{command}"
        );
        assert_eq!(
            crate::command::global_setting_executions(),
            (index + 1) * 3,
            "{command} must execute once for each of the three retained targets"
        );
    }
}

#[test]
fn layout_and_split_commands_preserve_a_focused_floating_window_and_the_tree() {
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
    fixture.swayward().layout.toggle_window_floating(None);

    let mut stream = UnixStream::connect(socket).unwrap();
    for (command, expected_reply) in [
        (
            "layout tabbed",
            r#"[{"success":false,"error":"Unable to change layout of floating windows","parse_error":false}]"#,
        ),
        (
            "layout toggle split",
            r#"[{"success":false,"error":"Unable to change layout of floating windows","parse_error":false}]"#,
        ),
        ("split v", r#"[{"success":true}]"#),
    ] {
        let before = fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree();

        stream
            .write_all(&swayward_ipc::wire::encode(
                MessageType::RunCommand,
                command,
            ))
            .unwrap();
        let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
        assert_eq!(reply, expected_reply, "reply for {command}");

        let after = fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree();
        assert_eq!(after, before, "tree changed after {command}");
    }
}

#[test]
fn criteria_lifecycle_commands_fail_without_changing_state() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    add_two_tiled_windows(&mut fixture);
    let before = fixture.swayward().config.borrow().layout.clone();
    let for_window = fixture.swayward().for_window.len();

    for (criteria, matches) in [
        (r#"[app_id="missing"]"#, 0),
        (r#"[app_id="left"]"#, 1),
        ("[all]", 2),
    ] {
        for command in ["exit", "reload"] {
            let input = format!("{criteria} {command}");
            let outcome = crate::command::execute(fixture.niri_state(), &input);
            let expected = if matches == 0 {
                "No matching node.".to_owned()
            } else {
                format!("criteria are not supported for {command}")
            };
            assert_eq!(outcome.len(), 1, "{input}: {outcome:?}");
            assert_eq!(
                outcome[0].error.as_deref(),
                Some(expected.as_str()),
                "{input}"
            );
            assert_eq!(outcome[0].parse_error, Some(false), "{input}");
            assert!(!fixture.swayward().shutdown_requested, "{input}");
            assert_eq!(fixture.swayward().config.borrow().layout, before, "{input}");
            assert_eq!(fixture.swayward().for_window.len(), for_window, "{input}");
        }
    }
}


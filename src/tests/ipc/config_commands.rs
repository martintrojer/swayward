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
fn criteria_with_no_matches_returns_sway_failure() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    assert_eq!(
        crate::command::execute(fixture.niri_state(), r#"[app_id="missing"] nop"#),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No matching node.".into()),
            parse_error: None,
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
            parse_error: None,
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
        ("split v", r#"[{"success":true}]"#),
        (
            "layout tabbed",
            r#"[{"success":false,"error":"Unable to change layout of floating windows"}]"#,
        ),
        (
            "layout toggle split",
            r#"[{"success":false,"error":"Unable to change layout of floating windows"}]"#,
        ),
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
            assert_eq!(outcome[0].parse_error, None, "{input}");
            assert!(!fixture.swayward().shutdown_requested, "{input}");
            assert_eq!(fixture.swayward().config.borrow().layout, before, "{input}");
            assert_eq!(fixture.swayward().for_window.len(), for_window, "{input}");
        }
    }
}

fn add_two_tiled_windows(fixture: &mut Fixture) {
    let client = fixture.add_client();
    for app_id in ["left", "right"] {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}

fn tiled_window_rects(fixture: &mut Fixture) -> Vec<Value> {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn collect(value: &Value, rects: &mut Vec<Value>) {
        if value["type"] == "con" && value["app_id"].is_string() {
            rects.push(value["rect"].clone());
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    collect(child, rects);
                }
            }
        }
    }
    let mut rects = Vec::new();
    collect(&tree, &mut rects);
    rects.sort_by_key(|rect| rect["x"].as_i64().unwrap());
    rects
}

/// Tiled window rects on one named workspace.
///
/// [`tiled_window_rects`] flattens every workspace, so a test that creates a
/// second workspace cannot use it: the two sets interleave under the sort.
fn tiled_window_rects_on(fixture: &mut Fixture, workspace: &str) -> Vec<Value> {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn collect(value: &Value, rects: &mut Vec<Value>) {
        if value["type"] == "con" && value["app_id"].is_string() {
            rects.push(value["rect"].clone());
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    collect(child, rects);
                }
            }
        }
    }
    fn find_ws<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        if value["type"] == "workspace" && value["name"] == name {
            return Some(value);
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    if let Some(found) = find_ws(child, name) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    let ws = find_ws(&tree, workspace).expect("workspace not in tree");
    let mut rects = Vec::new();
    collect(ws, &mut rects);
    rects.sort_by_key(|rect| rect["x"].as_i64().unwrap());
    rects
}

#[test]
fn reloaded_gap_defaults_do_not_change_an_existing_workspace() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let initial = swayward_config::Config::parse_mem(
        r#"layout {
            gaps 10
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(initial);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);
    assert_eq!(before[0]["y"], 8);
    assert_eq!(before[1]["y"], 8);

    let path = std::env::temp_dir().join(format!(
        "swayward-gap-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(
        &path,
        r#"layout {
            gaps 16
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "reload")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::from_str::<Value>(sway_fixture!("events/workspace.reload.json")).unwrap()
    );
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 16.);
    assert_eq!(tiled_window_rects(&mut fixture), before);

    std::fs::remove_file(path).unwrap();
}

#[test]
fn runtime_gaps_all_changes_existing_workspaces() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.outer_gaps_configured = true;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner all set 16")[0].success);
    let after = tiled_window_rects(&mut fixture);
    assert_ne!(after, before);
    assert_eq!(after[0]["x"], 14);
    assert_eq!(after[0]["y"], 14);
    assert_eq!(after[1]["y"], 14);
}

/// Sway's two-argument form sets the default for workspaces created later and
/// leaves existing ones alone (`sway/sway/commands/gaps.c:48-91`), because a
/// live workspace reads its own gaps, not the global default
/// (`sway/sway/tree/workspace.c:224-225`).
#[test]
fn gaps_defaults_form_does_not_disturb_an_existing_workspace() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.outer_gaps_configured = true;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 40")[0].success);

    // The default moved...
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 40.);
    // ...and the existing workspace did not.
    assert_eq!(tiled_window_rects(&mut fixture), before);
}

/// The converse, and the other half of the "must not fight" requirement: the
/// runtime form mutates the live workspace and must leave the default alone, so
/// a workspace created afterwards still inherits the configured default.
#[test]
fn runtime_gaps_form_does_not_overwrite_the_defaults() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 30")[0].success);

    // The live workspace moved.
    let after = tiled_window_rects(&mut fixture);
    assert_eq!(after[0]["x"], 30);
    // The default did not, so a later workspace still gets 10.
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 10.);

    // Prove it by creating one and reading its gaps back.
    assert!(crate::command::execute(fixture.niri_state(), "workspace fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "fresh")[0]["x"], 10);
}

/// Both directions in one session: setting the default, then a runtime change,
/// then another default write, must not let either clobber the other.
#[test]
fn gaps_defaults_and_runtime_forms_hold_separate_state() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 25")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 5")[0].success);

    // Runtime change won for the live workspace; the default is still 25.
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 25.);

    // A further default write still does not touch the live workspace.
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 50")[0].success);
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 50.);
}

/// `workspace <name> gaps <kind> <px>` is a per-workspace-name default applied
/// at creation (`sway/sway/tree/workspace.c:226-243`), so it must affect a
/// workspace of that name created afterwards and not the current one.
#[test]
fn workspace_gaps_apply_to_a_later_workspace_of_that_name() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(
        crate::command::execute(fixture.niri_state(), "workspace roomy gaps inner 45")[0].success
    );

    // The current workspace is untouched, as in sway.
    assert_eq!(tiled_window_rects(&mut fixture), before);
    // The global default is untouched too: this is per-name state.
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 10.);

    // A workspace with that name picks the value up.
    assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "roomy")[0]["x"], 45);

    // A differently named workspace still gets the global default.
    assert!(crate::command::execute(fixture.niri_state(), "workspace plain")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "plain")[0]["x"], 10);
}

#[test]
fn reload_rereads_config_and_emits_the_sway_workspace_event() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let path = std::env::temp_dir().join(format!(
        "swayward-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, "layout { gaps 7; }").unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "reload")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let expected: Value =
        serde_json::from_str(sway_fixture!("events/workspace.reload.json")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(), expected);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 7.);
    subscriber.set_nonblocking(true).unwrap();
    fixture.dispatch();
    let mut byte = [0];
    assert!(matches!(
        subscriber.read(&mut byte),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));

    std::fs::remove_file(path).unwrap();
}

#[test]
fn reload_reports_malformed_config_in_the_command_reply() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let mut fixture = Fixture::new();
    let path = std::env::temp_dir().join(format!(
        "swayward-bad-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, "binds { Mod+H { command; }; }").unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );

    let outcome = crate::command::execute(fixture.niri_state(), "reload");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Error(s) reloading config.".into()),
            parse_error: None,
        }]
    );

    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_config_reload_keeps_the_compositor_responsive() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let malformed =
        swayward_config::Config::parse_mem("binds { Mod+H { command; }; }").map_err(|error| {
            assert!(format!("{error:?}").contains("expected command"));
        });
    assert!(malformed.is_err());

    fixture.niri_state().reload_config(malformed);

    let outcome = crate::command::execute(fixture.niri_state(), "nop");
    assert_eq!(outcome.len(), 1);
    assert!(outcome[0].success, "{outcome:?}");
}

#[test]
fn reload_replaces_map_time_rules_while_windows_are_mapped() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let first = fixture.client(client).create_window();
    first.xdg_toplevel.set_app_id("special".into());
    first.commit();
    let surface = first.surface.clone();
    fixture.roundtrip(client);
    let first = fixture.client(client).window(&surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let first_id = fixture.swayward().layout.focus().unwrap().id();

    super::i3_conformance::reload_test_config(
        &mut fixture,
        r#"for_window [app_id="special"] mark reloaded"#,
    )
    .unwrap();

    let second = fixture.client(client).create_window();
    second.xdg_toplevel.set_app_id("special".into());
    second.commit();
    let surface = second.surface.clone();
    fixture.roundtrip(client);
    let second = fixture.client(client).window(&surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let second_id = fixture.swayward().layout.focus().unwrap().id();

    let marks = &fixture.swayward().marks_by_window;
    assert!(marks.get(&first_id).is_none_or(Vec::is_empty));
    assert_eq!(
        marks.get(&second_id).map(Vec::as_slice),
        Some(["reloaded".to_owned()].as_slice())
    );
}

#[test]
fn reload_updates_runtime_for_window_execution_state() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "for_window [con_mark=trigger] mark --add fired",
        )[0]
        .success
    );

    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let mapped = fixture.swayward().layout.focus().unwrap().id();

    fixture.swayward().executed_for_window.insert((
        mapped,
        "[con_mark=trigger]".into(),
        "mark --add fired".into(),
    ));
    fixture.niri_state().reload_config(Err(()));
    assert!(
        !fixture.swayward().executed_for_window.is_empty(),
        "a failed reload must preserve criteria from the active config"
    );
    fixture.swayward().executed_for_window.clear();

    assert!(crate::command::execute(fixture.niri_state(), "mark trigger")[0].success);
    assert!(fixture
        .swayward()
        .executed_for_window
        .iter()
        .any(|(window, criteria, _)| *window == mapped && criteria == "[con_mark=trigger]"));
    assert!(!fixture.swayward().runtime_for_window.is_empty());

    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(fixture.swayward().executed_for_window.is_empty());
    assert!(fixture.swayward().for_window.is_empty());
    assert!(fixture.swayward().runtime_for_window.is_empty());
}

#[test]
fn title_format_updates_get_tree_and_titlebar_after_client_title_change() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("format-app".into());
    window.set_title("before");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    assert!(crate::command::execute(fixture.niri_state(), "border normal")[0].success);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[app_id="format-app"] title_format [%app_id|%shell|%class|%instance|%sandbox_engine|%sandbox_app_id|%sandbox_instance_id] %title"#,
    );
    assert!(outcome[0].success);

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "before"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert_eq!(
        workspace.tiling().titlebar_titles(),
        ["[format-app|xdg_shell|||||] before"]
    );

    let window = fixture.client(client).window(&surface);
    window.set_title("%app_id");
    window.commit();
    fixture.double_roundtrip(client);

    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "%app_id"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert_eq!(
        workspace.tiling().titlebar_titles(),
        ["[format-app|xdg_shell|||||] %app_id"]
    );

    assert!(crate::command::execute(fixture.niri_state(), "title_format %title")[0].success);
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "%app_id"
    );
}

#[test]
fn title_format_updates_a_split_container_representation() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for title in ["one", "two", "three"] {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("app-{title}"));
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    fixture.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "mark formatted-split")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[con_mark=formatted-split] title_format group: %title %app_id"#,
    );
    assert!(outcome[0].success, "{outcome:?}");

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    let split = find_json_node_with_mark(&tree, "formatted-split").unwrap();
    assert_eq!(split["name"], Value::Null);
    let workspace_node = tree["nodes"][1]["nodes"][0].as_object().unwrap();
    assert_eq!(
        workspace_node["representation"],
        "T[app-one V[app-three app-two]]"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert!(workspace
        .tiling()
        .titlebar_titles()
        .iter()
        .any(|title| title == "group: V[three two] %app_id"));
}

#[test]
fn translated_for_window_nop_has_no_observable_window_effect() {
    fn mapped_leaf(config: Option<&str>) -> Value {
        let mut fixture = Fixture::new();
        fixture.add_output(1, (1920, 1080));
        if let Some(config) = config {
            super::i3_conformance::reload_test_config(&mut fixture, config).unwrap();
        }
        let client = fixture.add_client();
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id("nop-target".into());
        window.set_title("unchanged");
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);

        let swayward = fixture.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let mut leaf = find_json_node(&tree, "con", true).unwrap().clone();
        let leaf = leaf.as_object_mut().unwrap();
        leaf.remove("id");
        leaf.remove("foreign_toplevel_identifier");
        Value::Object(leaf.clone())
    }

    let baseline = mapped_leaf(Some(
        r#"for_window [app_id="^does-not-match$"] nop arbitrary comment text"#,
    ));
    let with_nop = mapped_leaf(Some(
        r#"for_window [app_id="^nop-target$"] nop arbitrary comment text"#,
    ));
    assert_eq!(with_nop, baseline);
}

#[test]
fn translated_map_time_sticky_command_executes_for_the_mapped_window() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            match app-id="^sticky-map$"
            open-floating true
            sway-for-window-command "sticky enable"
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("sticky-map".into());
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let floating = &tree["nodes"][1]["nodes"][0]["floating_nodes"][0];
    assert_eq!(floating["app_id"], "sticky-map");
    assert_eq!(floating["sticky"], true);
}

#[test]
fn runtime_assign_applies_only_to_windows_mapped_after_registration() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    map_test_window(&mut f, client, "assigned-existing");
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"assign [app_id="^assigned-"] workspace 7: target"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    map_test_window(&mut f, client, "assigned-future");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace_for = |app_id: &str| {
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|output| output["nodes"].as_array().unwrap())
            .find(|workspace| find_json_node_with_app_id(workspace, app_id).is_some())
            .unwrap()["name"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(workspace_for("assigned-existing"), "1");
    assert_eq!(workspace_for("assigned-future"), "7: target");
}

#[test]
fn runtime_assign_uses_the_first_matching_rule() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [
        r#"assign [app_id="^assigned$"] workspace first"#,
        r#"assign [app_id="^assigned$"] workspace second"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "first");
}

#[test]
fn file_config_assignment_precedes_a_runtime_assignment() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            match app-id="^assigned$"
            open-on-workspace "configured"
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^assigned$"] workspace runtime"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "configured");
}

#[test]
fn runtime_assign_supports_workspace_numbers() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^numbered$"] workspace number 7: target"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "numbered");

    assert!(f
        .swayward()
        .layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("7: target")));
}

#[test]
fn runtime_assign_skips_a_missing_output_and_uses_the_next_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [
        r#"assign [app_id="^assigned$"] output missing"#,
        r#"assign [app_id="^assigned$"] workspace fallback"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    map_test_window(&mut f, client, "assigned");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|output| output["nodes"].as_array().unwrap())
        .find(|workspace| find_json_node_with_app_id(workspace, "assigned").is_some())
        .unwrap();
    assert_eq!(workspace["name"], "fallback");
}

#[test]
fn runtime_assign_supports_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(1);
    let client = f.add_client();

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"assign [app_id="^output$"] output headless-2"#,
        )[0]
        .success
    );
    map_test_window(&mut f, client, "output");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let output = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| find_json_node_with_app_id(output, "output").is_some())
        .unwrap();
    assert_eq!(output["name"], "headless-2");
}

#[test]
fn successful_reload_clears_runtime_assign_and_no_focus_rules() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for command in [
        r#"assign [app_id="^future$"] workspace target"#,
        r#"no_focus [app_id="^future$"]"#,
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    assert_eq!(f.swayward().runtime_window_rules.len(), 2);

    f.niri_state()
        .reload_config(Err::<swayward_config::Config, _>(()));
    assert_eq!(f.swayward().runtime_window_rules.len(), 2);

    f.niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(f.swayward().runtime_window_rules.is_empty());
}

#[test]
fn runtime_no_focus_does_not_leave_the_first_window_unfocused() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), r#"no_focus [app_id="^first$"]"#)[0].success);
    map_test_window(&mut f, client, "first");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["app_id"],
        "first"
    );
}

#[test]
fn runtime_no_focus_applies_only_to_windows_mapped_after_registration() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["existing", "already-mapped"] {
        map_test_window(&mut f, client, app_id);
    }
    let focused_app_id = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node(&tree, "con", true).unwrap()["app_id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(focused_app_id(&mut f), "already-mapped");
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"no_focus [app_id="^(already-mapped|future)$"]"#,
        )[0]
        .success
    );
    assert_eq!(
        focused_app_id(&mut f),
        "already-mapped",
        "registering no_focus must not change an already mapped view"
    );

    map_test_window(&mut f, client, "future");
    assert_eq!(
        focused_app_id(&mut f),
        "already-mapped",
        "a future no_focus match must not steal focus"
    );
}

#[test]
fn for_window_applies_matching_command_when_window_maps() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            r#"for_window [app_id="^dialog$"] floating enable"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([{"success": true}])
    );

    let id = fixture.add_client();
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("dialog".into());
    window.set_title("Dialog");
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "floating_con", false).unwrap()["app_id"],
        "dialog"
    );
}

#[test]
fn marking_a_mapped_window_runs_each_newly_matching_for_window_rule_once() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(
        crate::command::execute(
            f.niri_state(),
            "for_window [con_mark=trigger] sticky toggle",
        )[0]
        .success
    );

    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("first".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    let mapped = f.swayward().layout.focus().unwrap().id();
    let sticky = |f: &mut Fixture, app_id: &str| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        find_json_node_with_app_id(&tree, app_id).unwrap()["sticky"] == true
    };
    assert!(!sticky(&mut f, "first"));

    assert!(crate::command::execute(f.niri_state(), "mark trigger")[0].success);
    assert!(
        sticky(&mut f, "first"),
        "the mark-dependent action must run"
    );

    let second = f.client(client).create_window();
    second.xdg_toplevel.set_app_id("second".into());
    second.commit();
    let surface = second.surface.clone();
    f.roundtrip(client);
    let second = f.client(client).window(&surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark trigger")[0].success);
    assert!(sticky(&mut f, "second"));

    let con_id = crate::ipc::tree::window_id(mapped);
    assert!(
        crate::command::execute(f.niri_state(), &format!("[con_id={con_id}] mark trigger"),)[0]
            .success
    );
    assert!(
        sticky(&mut f, "first"),
        "moving a global mark away and back must not rerun the first view's rule"
    );
}

#[test]
fn runtime_command_refusals_are_sway_shaped() {
    let mut f = Fixture::new();
    for (command, error) in [
        (
            "opacity 0.5",
            "opacity requires mutable per-container opacity support",
        ),
        (
            "inhibit_idle visible",
            "inhibit_idle requires user inhibitor policy support",
        ),
        (
            "urgent allow",
            "urgent allow|deny requires client urgency-request policy support",
        ),
    ] {
        assert_eq!(
            crate::command::execute(f.niri_state(), command),
            [swayward_ipc::CommandOutcome {
                success: false,
                error: Some(error.into()),
                parse_error: Some(true),
            }]
        );
    }
}

#[test]
fn shortcuts_inhibitor_disable_sets_future_policy_and_deactivates_current() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let current = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert_eq!(f.client(client).state.shortcut_inhibitor_events, [true]);

    let outcome = crate::command::execute(f.niri_state(), "shortcuts_inhibitor disable");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert_eq!(
        f.client(client).state.shortcut_inhibitor_events,
        [true, false]
    );

    current.destroy();
    f.double_roundtrip(client);
    f.client(client).state.shortcut_inhibitor_events.clear();
    let future = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert!(f.client(client).state.shortcut_inhibitor_events.is_empty());

    let outcome = crate::command::execute(f.niri_state(), "shortcuts_inhibitor enable");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert!(f.client(client).state.shortcut_inhibitor_events.is_empty());

    future.destroy();
    f.double_roundtrip(client);
    let _enabled_future = f.client(client).inhibit_shortcuts(&surface);
    f.double_roundtrip(client);
    assert_eq!(f.client(client).state.shortcut_inhibitor_events, [true]);
}

#[test]
fn runtime_presentation_command_refusals_are_explicit() {
    let mut f = Fixture::new();
    for (command, error) in [
        (
            "allow_tearing yes",
            "allow_tearing requires immediate presentation support",
        ),
        (
            "max_render_time 1",
            "max_render_time requires per-view render deadline support",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].error.as_deref(), Some(error), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
    }
    assert_eq!(
        crate::command::execute(f.niri_state(), "max_render_time")[0]
            .error
            .as_deref(),
        Some("Missing max render time argument.")
    );
}

#[test]
fn create_output_adds_a_headless_output() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let mut stream = UnixStream::connect(socket).unwrap();

    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(
        outputs
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["headless-1", "headless-2"]
    );
    assert_eq!(
        f.niri_output(2).current_mode().unwrap().size,
        (1920, 1080).into()
    );

    let removed = f.niri_output(2);
    f.swayward().remove_output(&removed);
    assert!(crate::command::execute(f.niri_state(), "create_output")[0].success);
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(
        outputs
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["headless-1", "headless-3"]
    );
}

#[test]
fn create_output_rejects_unsupported_backends_without_changing_output_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let snapshot = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::json!({
            "outputs": describe_outputs(&swayward.layout, &swayward.global_space),
            "workspaces": describe_workspaces(&swayward.layout, &swayward.global_space),
        })
    };
    let before = snapshot(&mut f);

    let state = f.niri_state();
    let outcome = crate::command::create_output(None, &mut state.swayward);
    assert_eq!(
        outcome,
        swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Can only create outputs for Wayland, X11 or headless backends".into()),
            parse_error: None,
        }
    );
    assert_eq!(snapshot(&mut f), before);
}

#[test]
fn urgent_command_updates_a_hidden_scratchpad_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("hidden-urgent".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let other = f.client(client).create_window();
    other.commit();
    let other_surface = other.surface.clone();
    f.roundtrip(client);
    let other = f.client(client).window(&other_surface);
    other.attach_new_buffer();
    other.ack_last_and_commit();
    f.double_roundtrip(client);
    let outcome = crate::command::execute(f.niri_state(), "[app_id=hidden-urgent] urgent enable");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(test_window_is_urgent(&mut f, "hidden-urgent"));
}

#[test]
fn urgent_command_changes_only_an_unfocused_selected_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    for app_id in ["target", "focused"] {
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

    let urgent = |f: &mut Fixture| {
        f.swayward()
            .layout
            .windows()
            .find(|(_, window)| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.as_deref() == Some("target")
                })
            })
            .unwrap()
            .1
            .is_urgent()
    };

    for (command, expected) in [
        ("[app_id=target] urgent enable", true),
        ("[app_id=target] urgent toggle", false),
        ("[app_id=target] urgent toggle", true),
        ("[app_id=target] urgent disable", false),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(urgent(&mut f), expected, "{command}");
    }
}

#[test]
fn scratchpad_show_remaps_floating_center_between_asymmetric_outputs() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (683, 768), Some((0, 0)));
    f.add_named_output_at("right".into(), (1024, 768), Some((683, 0)));
    assert!(
        crate::command::execute(f.niri_state(), "focus output left, workspace left")[0].success
    );
    assert!(
        crate::command::execute(f.niri_state(), "focus output right, workspace right")[0].success
    );
    assert!(crate::command::execute(f.niri_state(), "workspace left")[0].success);

    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", true).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let window = f.client(client).window(&surface);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "move position 40 px 100 px")[0].success);
    let left = rect(&mut f);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let right = rect(&mut f);
    assert_eq!(right["x"], 743, "left={left} right={right}");
    assert_ne!(right["x"], left["x"]);

    assert!(crate::command::execute(f.niri_state(), "move position 600 px 100 px")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let left_again = rect(&mut f);
    assert_eq!(left_again["x"], 400);
    assert_ne!(left_again["x"], right["x"]);
}

#[test]
fn initially_floating_window_uses_and_clamps_client_size() {
    fn mapped_rect(requested: (u16, u16), honor_requested_size: bool) -> Value {
        let mut config = swayward_config::Config::default();
        config.animations.off = true;
        config.window_rules.push(swayward_config::WindowRule {
            open_floating: Some(true),
            ..Default::default()
        });
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1280, 800));
        let client = f.add_client();
        let window = f.client(client).create_window();
        window.set_size(requested.0, requested.1);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(
            if honor_requested_size {
                requested.0
            } else {
                1280
            },
            if honor_requested_size {
                requested.1
            } else {
                800
            },
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let requested = f
            .swayward()
            .layout
            .focus()
            .unwrap()
            .expected_size()
            .unwrap();
        let window = f.client(client).window(&surface);
        window.set_size(
            requested.w.try_into().unwrap(),
            requested.h.try_into().unwrap(),
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false)
            .unwrap()
            .clone()
    }

    let requested = mapped_rect((400, 150), true);
    assert_eq!(requested["geometry"]["width"], 400);
    assert_eq!(requested["geometry"]["height"], 150);
    assert_eq!(requested["rect"]["width"], 400);
    assert_eq!(requested["rect"]["height"], 150);

    let clamped = mapped_rect((1600, 1000), true);
    assert_eq!(clamped["geometry"]["width"], 1600);
    assert_eq!(clamped["geometry"]["height"], 1000);
    assert_eq!(clamped["rect"]["width"], 1280);
    assert_eq!(clamped["rect"]["height"], 800);

    assert_ne!(
        mapped_rect((400, 150), false)["geometry"],
        requested["geometry"]
    );
}

#[test]
fn focused_split_rejects_border_and_resizes_as_one_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1200, 800));
    let client = f.add_client();
    for app_id in ["outside", "upper", "lower"] {
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
    f.swayward().layout.consume_or_expel_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "move down")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let border = crate::command::execute(f.niri_state(), "border pixel 7");
    assert!(!border[0].success, "{border:?}");
    assert_eq!(
        border[0].error.as_deref(),
        Some("Only views can have borders")
    );

    let widths = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        ["outside", "upper", "lower"].map(|app_id| {
            find_json_node_with_app_id(&tree, app_id).unwrap()["rect"]["width"]
                .as_i64()
                .unwrap()
        })
    };
    let before = widths(&mut f);
    let resize = crate::command::execute(f.niri_state(), "resize grow width 10 ppt");
    assert!(resize[0].success, "{resize:?}");
    let after = widths(&mut f);
    assert!(after[0] < before[0], "before={before:?} after={after:?}");
    assert!(after[1] > before[1], "before={before:?} after={after:?}");
    assert!(after[2] > before[2], "before={before:?} after={after:?}");

    assert!(crate::command::execute(f.niri_state(), "mark group")[0].success);
    let resize =
        crate::command::execute(f.niri_state(), "[con_mark=group] resize shrink width 5 ppt");
    assert!(resize[0].success, "{resize:?}");
    let shrunk = widths(&mut f);
    assert!(shrunk[0] > after[0], "after={after:?} shrunk={shrunk:?}");
    assert!(shrunk[1] < after[1], "after={after:?} shrunk={shrunk:?}");
    assert!(shrunk[2] < after[2], "after={after:?} shrunk={shrunk:?}");
}

#[test]
fn tiled_grow_at_workspace_edge_reports_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "resize grow right 10 px");

    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot resize any further")
    );
}

#[test]
fn floating_grow_edges_change_origin_and_size_like_sway() {
    let config = swayward_config::Config::parse_mem("animations { off; }").unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output_at(1, (1280, 800), Some((100, 50)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "resize set 400 px 300 px")[0].success);
    let requested = f
        .swayward()
        .layout
        .focus()
        .unwrap()
        .expected_size()
        .unwrap();
    let window = f.client(client).window(&surface);
    window.set_size(
        requested.w.try_into().unwrap(),
        requested.h.try_into().unwrap(),
    );
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let rect = |f: &mut Fixture| {
        f.niri_state().ipc_refresh_layout();
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    for (direction, delta) in [
        ("left", (-10, 0, 10, 0)),
        ("right", (0, 0, 10, 0)),
        ("up", (0, -10, 0, 10)),
        ("down", (0, 0, 0, 10)),
    ] {
        let before = rect(&mut f);
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!("resize grow {direction} 10 px or 25 ppt"),
        );
        assert!(outcome[0].success, "{direction}: {outcome:?}");
        let requested = f
            .swayward()
            .layout
            .focus()
            .unwrap()
            .expected_size()
            .unwrap();
        let window = f.client(client).window(&surface);
        window.set_size(
            requested.w.try_into().unwrap(),
            requested.h.try_into().unwrap(),
        );
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let after = rect(&mut f);
        assert_eq!(
            after["x"].as_i64(),
            before["x"].as_i64().map(|x| x + delta.0)
        );
        assert_eq!(
            after["y"].as_i64(),
            before["y"].as_i64().map(|y| y + delta.1)
        );
        assert_eq!(
            after["width"].as_i64(),
            before["width"].as_i64().map(|width| width + delta.2)
        );
        assert_eq!(
            after["height"].as_i64(),
            before["height"].as_i64().map(|height| height + delta.3)
        );
    }

    assert!(crate::command::execute(f.niri_state(), "resize set 1280 px 800 px")[0].success);
    let requested = f
        .swayward()
        .layout
        .focus()
        .unwrap()
        .expected_size()
        .unwrap();
    let window = f.client(client).window(&surface);
    window.set_size(
        requested.w.try_into().unwrap(),
        requested.h.try_into().unwrap(),
    );
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let before = rect(&mut f);
    assert_eq!(before["width"], 1280);
    assert_eq!(before["height"], 800);
    let outcome = crate::command::execute(f.niri_state(), "resize grow right 10 px or 25 ppt");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot resize any further")
    );
    assert_eq!(rect(&mut f), before);
}

#[test]
fn move_command_rejects_fullscreen_floating_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move left");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Cannot move fullscreen floating container")
    );
}

#[test]
fn move_command_uses_sway_floating_pixel_distances() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };
    let before = rect(&mut f);

    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    let moved = rect(&mut f);
    assert_eq!(moved["x"].as_i64(), before["x"].as_i64().map(|x| x - 10));

    assert!(crate::command::execute(f.niri_state(), "move down 20 px")[0].success);
    let moved = rect(&mut f);
    assert_eq!(moved["y"].as_i64(), before["y"].as_i64().map(|y| y + 20));
}

#[test]
fn move_position_uses_workspace_coordinates_and_rejects_absolute_ppt() {
    let mut f = Fixture::new();
    f.add_output(1, (1000, 800));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move position 5 px 15")[0].success);
    assert_eq!(rect(&mut f)["x"], 5);
    assert_eq!(rect(&mut f)["y"], 15);

    assert!(crate::command::execute(f.niri_state(), "move position 20 ppt 25 ppt")[0].success);
    assert_eq!(rect(&mut f)["x"], 200);
    assert_eq!(rect(&mut f)["y"], 200);

    for command in [
        "move absolute position 20 ppt 5 px",
        "move absolute position 5 px 20 ppt",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success);
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Cannot move to absolute positions by ppt")
        );
    }
}

#[test]
fn move_absolute_position_is_verbatim_under_a_bar_and_gaps() {
    // `move absolute position` must use the requested coordinate unchanged.
    // Sway applies the workspace origin only on the relative form
    // (`sway/sway/commands/move.c:913-916`) and then calls
    // container_floating_move_to, which performs no bounds check
    // (`sway/sway/tree/container.c:1127-1159`).
    //
    // swayward routed the coordinate through floating_pos, whose setter adds
    // working_area.loc back (`src/layout/floating.rs:120-129`), so a request
    // that already accounted for the bar gained the bar and the gaps a second
    // time. Only a preset with no slack revealed it: a full-workspace-height
    // window at the workspace origin overflowed the bottom edge, while shorter
    // presets absorbed the shift invisibly.
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.gaps = 4.;
    config.layout.outer_gaps_configured = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();

    // A top bar, so the workspace origin is not the output origin.
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
    use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;

    let layer = f.client(client).create_layer(None, Layer::Top, "bar");
    layer.set_configure_props(crate::tests::client::LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 20)),
        exclusive_zone: Some(20),
        ..Default::default()
    });
    let layer_surface = layer.surface.clone();
    layer.commit();
    f.double_roundtrip(client);
    let layer = f.client(client).layer(&layer_surface);
    let size = layer.configures_received.last().unwrap().1.size;
    layer.attach_new_buffer();
    layer.set_size(size.0 as u16, size.1 as u16);
    layer.ack_last_and_commit();
    f.double_roundtrip(client);

    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    // find_json_node would return the hidden __i3 scratch workspace, whose
    // rect is all zeroes.
    fn visible<'a>(node: &'a Value, kind: &str) -> Option<&'a Value> {
        if node["type"] == kind && node["name"] != "__i3" && node["name"] != "__i3_scratch" {
            return Some(node);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| node[key].as_array()?.iter().find_map(|c| visible(c, kind)))
    }

    let node = |f: &mut Fixture, kind: &str| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        visible(&tree, kind).unwrap()["rect"].clone()
    };

    let ws = node(&mut f, "workspace");
    let (ws_x, ws_y) = (ws["x"].as_i64().unwrap(), ws["y"].as_i64().unwrap());
    let (ws_w, ws_h) = (
        ws["width"].as_i64().unwrap(),
        ws["height"].as_i64().unwrap(),
    );
    assert_eq!(ws_y, 20, "workspace rect starts below the bar");

    // The tallCenter preset from a real script: full workspace height at the
    // workspace origin, computed from the IPC workspace rect. Zero slack, so
    // any displacement overflows.
    let command =
        format!("resize set {ws_w} px {ws_h} px, move absolute position {ws_x} px {ws_y} px");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    f.double_roundtrip(client);

    let rect = node(&mut f, "floating_con");
    assert_eq!(
        rect["y"], ws_y,
        "absolute y must be used verbatim, not offset by the bar and gaps"
    );
    assert_eq!(rect["x"], ws_x, "absolute x must be used verbatim");

    let bottom = rect["y"].as_i64().unwrap() + rect["height"].as_i64().unwrap();
    assert!(
        bottom <= ws_y + ws_h,
        "a full-height window overflowed the workspace: bottom {bottom} > {}",
        ws_y + ws_h
    );
}

#[test]
fn move_position_centers_on_root_and_pointer() {
    let mut f = Fixture::new();
    f.add_output_at(1, (1000, 800), Some((100, 50)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let rect = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        find_json_node(&tree, "floating_con", false).unwrap()["rect"].clone()
    };

    assert!(crate::command::execute(f.niri_state(), "move position 150 75")[0].success);
    let relative = rect(&mut f);
    assert_eq!(relative["x"], 250);
    assert_eq!(relative["y"], 125);

    assert!(crate::command::execute(f.niri_state(), "move absolute position 150 75")[0].success);
    let absolute = rect(&mut f);
    assert_eq!(absolute["x"], 150);
    assert_eq!(absolute["y"], 75);

    assert!(crate::command::execute(f.niri_state(), "move absolute position center")[0].success);
    let centered = rect(&mut f);
    assert_eq!(centered["x"], 600);
    assert_eq!(centered["y"], 450);

    f.niri_state().move_cursor((300., 250.).into());
    assert!(crate::command::execute(f.niri_state(), "move position pointer")[0].success);
    let pointer = rect(&mut f);
    assert_eq!(pointer["x"], 300);
    assert_eq!(pointer["y"], 250);
}

#[test]
fn move_position_targets_floating_windows_by_criteria() {
    let mut f = Fixture::new();
    f.add_output(1, (1000, 800));
    let client = f.add_client();
    for app_id in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    }

    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[app_id="first"] move position 25 px 30 px"#,
        )[0]
        .success
    );

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn find_first(value: &Value) -> Option<&Value> {
        if value["app_id"] == "first" {
            return Some(value);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| value[key].as_array()?.iter().find_map(find_first))
    }
    let first = find_first(&tree).unwrap();
    assert_eq!(first["rect"]["x"], 25);
    assert_eq!(first["rect"]["y"], 30);
}

#[test]
fn floating_ipc_rect_uses_final_position_during_animation() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("animated".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.swayward().layout.toggle_window_floating(None);
    f.swayward().layout.move_floating_window(
        None,
        swayward_ipc::PositionChange::SetFixed(100.),
        swayward_ipc::PositionChange::SetFixed(200.),
        true,
    );

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let node = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(node["rect"]["x"], 100);
    assert_eq!(node["rect"]["y"], 200);
}

#[test]
fn floating_input_region_holes_click_through_but_decorations_activate() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.border.off = false;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for app_id in ["bottom", "top"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        assert!(
            crate::command::execute(f.niri_state(), "resize set width 200 height 100")[0].success
        );
        let window = f.client(client).window(&surface);
        window.set_size(200, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let id = f.swayward().layout.focus().unwrap().window.clone();
        f.swayward().layout.move_floating_window(
            Some(&id),
            swayward_ipc::PositionChange::SetFixed(100.),
            swayward_ipc::PositionChange::SetFixed(100.),
            false,
        );
        surfaces.push(surface);
    }

    let focused_app_id = |f: &mut Fixture| {
        f.swayward().layout.focus().and_then(|window| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
        })
    };
    let (bottom, tile_pos, window_loc) = {
        let workspace = f.swayward().layout.active_workspace().unwrap();
        let bottom = workspace
            .windows()
            .find(|window| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.as_deref() == Some("bottom")
                })
            })
            .unwrap()
            .window
            .clone();
        let (tile, tile_pos, _) = workspace
            .tiles_with_render_positions()
            .find(|(tile, _, _)| {
                crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                    role.app_id.as_deref() == Some("top")
                })
            })
            .unwrap();
        (bottom, tile_pos, tile.window_loc())
    };
    let inside = tile_pos + window_loc + smithay::utils::Point::from((25., 25.));
    let outside = tile_pos + window_loc + smithay::utils::Point::from((175., 25.));
    let border_in_tile = smithay::utils::Point::from((window_loc.x / 2., window_loc.y + 10.));

    f.client(client)
        .set_input_region(&surfaces[1], Some((0, 0, 100, 100)));
    f.double_roundtrip(client);
    f.niri_state().move_cursor(inside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("top"));

    f.swayward().layout.activate_window_without_raising(&bottom);
    let output = f.niri_output(1);
    assert_eq!(
        f.swayward()
            .layout
            .window_under(&output, outside)
            .and_then(|(window, _)| {
                crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
            })
            .as_deref(),
        Some("bottom")
    );
    f.niri_state().move_cursor(outside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("bottom"));

    f.client(client).set_input_region(&surfaces[1], None);
    f.double_roundtrip(client);
    f.niri_state().move_cursor(inside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("bottom"));

    f.client(client).reset_input_region(&surfaces[1]);
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="top"] focus"#)[0].success);
    f.swayward().layout.activate_window_without_raising(&bottom);
    f.niri_state().move_cursor(outside);
    pointer_button(&mut f, 0x110, true);
    pointer_button(&mut f, 0x110, false);
    assert_eq!(focused_app_id(&mut f).as_deref(), Some("top"));

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="top"] focus"#)[0].success);
    let workspace = f.swayward().layout.active_workspace().unwrap();
    let (tile, _, _) = workspace
        .tiles_with_render_positions()
        .find(|(tile, _, _)| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.as_deref() == Some("top")
            })
        })
        .unwrap();
    assert_eq!(
        tile.hit(border_in_tile),
        Some(crate::layout::HitType::Activate {
            is_tab_indicator: false
        })
    );
}

#[test]
fn floating_stacking_and_focus_match_sway_before_and_after_raise() {
    let two: Value = serde_json::from_str(sway_fixture!("two_floating.tree.json")).unwrap();
    assert_eq!(
        floating_order(&two),
        (
            vec!["fixture-1", "fixture-2"],
            vec!["fixture-2", "fixture-1"]
        )
    );

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for title in ["fixture-tiled", "fixture-1", "fixture-2", "fixture-3"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(title.into());
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if title != "fixture-tiled" {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }

    let describe = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap()
    };
    let before: Value =
        serde_json::from_str(sway_fixture!("three_floating_before_raise.tree.json")).unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&before));

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="^fixture-1$"] focus"#)[0].success);
    let after: Value =
        serde_json::from_str(sway_fixture!("three_floating_after_raise.tree.json")).unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&after));
}

#[test]
fn live_ipc_rectangle_roles_match_sway_relationships() {
    let expected = nested_fixture_tree();
    let actual = nested_live_tree();
    assert_rectangle_roles_match_fixture(&expected, &actual, "$tree");
}

#[test]
fn nested_tiling_rectangles_match_sway_roles() {
    let tree = nested_live_tree();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let top = &workspace["nodes"][0];
    let nested = &workspace["nodes"][1];
    let bottom_left = &nested["nodes"][0];
    let bottom_right = &nested["nodes"][1];

    assert_eq!(
        nested["rect"]["x"],
        bottom_left["rect"]["x"].as_i64().unwrap()
            - bottom_left["deco_rect"]["x"].as_i64().unwrap()
    );
    assert!(nested["rect"]["x"].as_i64().unwrap() > top["rect"]["x"].as_i64().unwrap());
    assert_eq!(
        nested["rect"]["y"],
        bottom_left["rect"]["y"].as_i64().unwrap()
            - bottom_left["deco_rect"]["height"].as_i64().unwrap()
    );
    assert_eq!(nested["rect"]["width"], bottom_left["rect"]["width"]);
    assert_eq!(
        nested["rect"]["height"],
        bottom_right["rect"]["y"].as_i64().unwrap()
            + bottom_right["rect"]["height"].as_i64().unwrap()
            - nested["rect"]["y"].as_i64().unwrap()
    );
    for window in [top, bottom_left, bottom_right] {
        assert!(window["deco_rect"]["height"].as_i64().unwrap() > 0);
        assert_eq!(window["current_border_width"], 4);
        assert_eq!(window["window_rect"]["x"], 4);
        assert_eq!(window["window_rect"]["y"], 0);
        assert_eq!(
            window["window_rect"]["width"].as_i64().unwrap(),
            window["rect"]["width"].as_i64().unwrap() - 8
        );
        assert_eq!(
            window["window_rect"]["height"].as_i64().unwrap(),
            window["rect"]["height"].as_i64().unwrap() - 4
        );
    }
}

#[test]
fn border_none_zeroes_deco_and_uses_the_whole_rect_for_window() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "border none")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let window = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(
        window["deco_rect"],
        serde_json::json!({"x": 0, "y": 0, "width": 0, "height": 0})
    );
    assert_eq!(window["window_rect"]["x"], 0);
    assert_eq!(window["window_rect"]["y"], 0);
    assert_eq!(window["window_rect"]["width"], window["rect"]["width"]);
    assert_eq!(window["window_rect"]["height"], window["rect"]["height"]);
}

#[test]
fn live_ipc_percent_matches_sway_parent_shares() {
    let expected = nested_fixture_tree();
    let representation = nested_representation_live_tree();
    assert_eq!(
        expected["nodes"][1]["nodes"][0]["representation"],
        representation["nodes"][1]["nodes"][0]["representation"],
        "workspace representation at $tree.nodes[1].nodes[0]"
    );
    let actual = nested_live_tree();
    assert_percent_value_matches_fixture(
        &expected["nodes"][1]["nodes"][0]["nodes"][1],
        &actual["nodes"][1]["nodes"][0]["nodes"][1],
        "$tree.nodes[1].nodes[0].nodes[1]",
    );
    assert_percent_matches_fixture(&expected, &actual, "$tree");
}

#[test]
fn root_focus_lists_outputs_once_in_global_mru_order() {
    let mut f = Fixture::new();
    for output in 1..=3 {
        f.add_output(output, (1280, 720));
    }
    let client = f.add_client();
    for output in [1, 2, 3] {
        f.niri_focus_output(output);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let root_focus = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .focus
    };
    let output_ids = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .nodes
        .into_iter()
        .filter(|node| node.name.as_deref() != Some("__i3"))
        .map(|node| node.id)
        .collect::<Vec<_>>()
    };

    let ids = output_ids(&mut f);
    assert_eq!(root_focus(&mut f), [ids[2], ids[1], ids[0]]);
    f.niri_focus_output(1);
    assert_eq!(root_focus(&mut f), [ids[0], ids[2], ids[1]]);
    let focus = root_focus(&mut f);
    assert_eq!(focus.len(), ids.len());
    assert_eq!(
        focus.iter().collect::<std::collections::HashSet<_>>().len(),
        ids.len()
    );
}

#[test]
fn criteria_focus_output_ignores_hidden_scratchpad_match_and_uses_seat_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left-head".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right-head".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let command = format!(r#"[con_id="{hidden}"] focus output right-head"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "left-head"
    );
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn criteria_focus_output_succeeds_without_an_output_and_keeps_scratchpad_hidden() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let output = f.swayward().layout.active_output().unwrap().clone();

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(f.swayward().layout.active_output(), Some(&output));
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn focus_output_prefers_a_name_over_a_direction_and_resolves_directions() {
    let mut f = Fixture::new();
    f.add_named_output_at("origin".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("left".into(), (1280, 720), Some((1280, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
    assert!(crate::command::execute(f.niri_state(), "focus output origin")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
}

#[test]
fn focus_output_uses_nearest_geometry_then_wraps_to_farthest_opposite() {
    let mut f = Fixture::new();
    f.add_named_output_at("west".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("center".into(), (800, 600), Some((800, 0)));
    f.add_named_output_at("east".into(), (800, 600), Some((1600, 0)));
    f.add_named_output_at("far-east".into(), (800, 600), Some((2400, 0)));
    let client = f.add_client();

    let mut ids = std::collections::HashMap::new();
    for output in ["west", "east", "far-east", "center"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("{output}-window"));
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        f.niri_state().update_keyboard_focus();
        ids.insert(output, f.swayward().layout.focus().unwrap().id());
    }

    assert!(crate::command::execute(f.niri_state(), "focus output east")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);
    let latest = f.client(client).create_window();
    latest.xdg_toplevel.set_app_id("east-latest".into());
    latest.commit();
    let latest_surface = latest.surface.clone();
    f.roundtrip(client);
    let latest = f.client(client).window(&latest_surface);
    latest.attach_new_buffer();
    latest.ack_last_and_commit();
    f.double_roundtrip(client);
    f.niri_state().update_keyboard_focus();
    ids.insert("east", f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "focus output center")[0].success);

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "east");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);

    assert!(crate::command::execute(f.niri_state(), "focus output far-east")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "west");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["west"]);
}

#[test]
fn directional_focus_prefers_an_adjacent_output_over_local_wrapping() {
    for (target_position, layout, command, wrapping, expected_output) in [
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (0, 600),
            "layout stacked",
            "focus down",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Force,
            "source",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::No,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Workspace,
            "source",
        ),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = wrapping;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("target".into(), (800, 600), Some(target_position));
        let client = f.add_client();

        for output in ["target", "source", "source"] {
            assert!(
                crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0]
                    .success
            );
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        assert!(crate::command::execute(f.niri_state(), layout)[0].success);

        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected_output
        );
    }
}

#[test]
fn no_wrapping_crosses_an_adjacent_output_but_does_not_wrap_outputs() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::No;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
}

#[test]
fn workspace_wrapping_uses_local_wrap_instead_of_an_adjacent_output() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let mut ids = Vec::new();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "source"
    );
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids[0]);
}

#[test]
fn workspace_wrapping_allows_output_focus_from_a_focused_workspace_node() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_workspace_focused());

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "target"
    );
}

#[test]
fn global_fullscreen_blocks_directional_output_focus_but_workspace_fullscreen_does_not() {
    for (global, direction, source, expected) in [
        (false, "right", "left", "right"),
        (false, "left", "right", "left"),
        (true, "right", "left", "left"),
        (true, "left", "right", "right"),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {source}"))[0].success
        );
        let client = f.add_client();
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mode = if global { " global" } else { "" };
        assert!(
            crate::command::execute(f.niri_state(), &format!("fullscreen enable{mode}"))[0].success
        );

        assert!(crate::command::execute(f.niri_state(), &format!("focus {direction}"))[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected,
            "global={global} direction={direction}"
        );
    }
}

#[test]
fn focus_output_reports_sway_errors() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let output = f.swayward().layout.active_output().unwrap().clone();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output missing"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("There is no output with that name.".into()),
            parse_error: Some(true),
        }]
    );
    assert_eq!(f.swayward().layout.active_output(), Some(&output));

    let mut f = Fixture::new();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output right"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No focused workspace to base directions off of.".into()),
            parse_error: None,
        }]
    );
}

#[test]
fn move_output_reports_the_missing_target() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(
        crate::command::execute(f.niri_state(), "move output missing")[0]
            .error
            .as_deref(),
        Some("Can't find output with name/direction 'missing'")
    );
}

#[test]
fn move_output_accepts_direction_name_and_workspace_forms() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let outputs = [f.niri_output(1).name(), f.niri_output(2).name()];
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move output right")[0].success);
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == focused)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        &outputs[1]
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("move container to output {}", outputs[0])
        )[0]
        .success
    );
    assert!(crate::command::execute(f.niri_state(), "move workspace output right")[0].success);
}

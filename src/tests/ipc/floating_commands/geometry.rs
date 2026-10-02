#[test]
fn floating_grow_edges_change_origin_and_size_like_sway() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    config.layout.floating_maximum_size = swayward_config::FloatingSize {
        width: 1280,
        height: 800,
    };
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
    for command in [
        "resize grow right 10 px or 25 ppt",
        "resize grow width 10 px or 25 ppt",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            outcome,
            [swayward_ipc::CommandOutcome {
                success: false,
                error: Some("Cannot resize any further".into()),
                parse_error: Some(true),
            }],
            "{command}"
        );
        assert_eq!(rect(&mut f), before, "{command}");
    }

    assert!(crate::command::execute(f.niri_state(), "resize set 1 px 1 px")[0].success);
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
    let outcome = crate::command::execute(f.niri_state(), "resize shrink height 10 px");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Cannot resize any further".into()),
            parse_error: Some(true),
        }]
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

/// Oracle: fullscreen_floating_group_move_refused. A fullscreen floating
/// group root is a fullscreen floating container too
/// (`cmd_move_in_direction`, sway/commands/move.c:688-692). Random seed 314
/// step 18.
#[test]
fn directional_move_of_a_fullscreen_floating_group_is_refused() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    for command in ["focus parent", "floating toggle", "fullscreen toggle"] {
        let reply = crate::command::execute(f.niri_state(), command);
        assert!(reply[0].success, "{command}: {reply:?}");
    }

    let outcome = crate::command::execute(f.niri_state(), "move left");
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
    // (`sway/sway/tree/container.c:1113-1145`).
    // A full-workspace-height window leaves no slack for an accidental second
    // application of the bar or gap offset.
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
    assert_eq!(
        ws_y, 24,
        "workspace rect starts below the bar and outer gap"
    );

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
    let (mut f, _) = ipc_fixture();
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

    let tree = get_tree(&mut f);
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
    let two: Value = serde_json::from_str(&sway_fixture!("two_floating.tree.json")).unwrap();
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
        serde_json::from_str(&sway_fixture!("three_floating_before_raise.tree.json")).unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&before));

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="^fixture-1$"] focus"#)[0].success);
    let after: Value =
        serde_json::from_str(&sway_fixture!("three_floating_after_raise.tree.json")).unwrap();
    assert_eq!(floating_order(&describe(&mut f)), floating_order(&after));
}

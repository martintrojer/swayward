#[test]
fn criteria_directional_move_uses_the_materialized_target_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["target", "middle", "target", "focused"] {
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
    let focused = f.swayward().layout.focus().unwrap().id();

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="target"] move right"#);

    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.clone().unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["middle", "target", "focused", "target"]);

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="target"] move left"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| {
                role.app_id.clone().unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["target", "middle", "target", "focused"]);
}

#[test]
fn criteria_commands_do_not_change_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
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
        surfaces.push(surface);
    }
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="target"] mark selected"#)[0].success
    );

    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert_eq!(surfaces.len(), 2);
}

#[test]
fn multi_target_mark_moves_to_last_match_and_unmark_clears_every_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();

    for title in ["first", "second"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("shared-app".into());
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="shared-app"] mark shared"#);
    assert_eq!(outcome.len(), 1);
    assert!(outcome[0].success);
    assert!(f
        .swayward()
        .marks_by_window
        .get(&ids[0])
        .is_none_or(Vec::is_empty));
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[1]).map(Vec::as_slice),
        Some(["shared".to_owned()].as_slice())
    );

    for (id, mark) in ids.iter().zip(["first", "second"]) {
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!(
                r#"[con_id="{}"] mark {mark}"#,
                crate::ipc::tree::window_id(*id)
            ),
        );
        assert!(outcome[0].success);
    }
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="shared-app"] unmark"#)[0].success);
    assert!(ids.iter().all(|id| f
        .swayward()
        .marks_by_window
        .get(id)
        .is_none_or(Vec::is_empty)));
}

#[test]
fn semicolon_starts_a_new_criteria_scope() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();
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
        ids.push(f.swayward().layout.focus().unwrap().id());
    }

    let outcomes = crate::command::execute(
        f.niri_state(),
        r#"[app_id="first"] mark first; [app_id="second"] mark second"#,
    );

    assert!(outcomes.iter().all(|outcome| outcome.success));
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[0]).map(Vec::as_slice),
        Some(["first".to_owned()].as_slice())
    );
    assert_eq!(
        f.swayward().marks_by_window.get(&ids[1]).map(Vec::as_slice),
        Some(["second".to_owned()].as_slice())
    );
}

#[test]
fn comma_chain_keeps_the_original_criteria_targets() {
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

    assert!(crate::command::execute(f.niri_state(), "mark original")[0].success);
    let outcomes = crate::command::execute(
        f.niri_state(),
        "[con_mark=original] unmark original, mark retained",
    );
    assert!(outcomes.iter().all(|outcome| outcome.success));
    assert_eq!(
        f.swayward().marks_by_window.values().next().unwrap(),
        &["retained"]
    );
}

#[test]
fn output_workspaces_and_move_replacements_use_next_free_numbers() {
    let mut f = Fixture::new();
    f.add_named_output_at("fake-0".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("fake-1".into(), (100, 100), Some((100, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output fake-1")[0].success);
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.output.as_str(), workspace.name.as_str()))
            .collect::<Vec<_>>(),
        [("fake-0", "1"), ("fake-1", "2")]
    );

    assert!(crate::command::execute(f.niri_state(), "focus output fake-0")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move workspace to output fake-1")[0].success);
    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.output == "fake-0")
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>(),
        ["3"]
    );
}

#[test]
fn rename_ignores_an_empty_inactive_source_sway_would_have_destroyed() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for command in ["workspace 5", "open", "workspace 6", "open", "workspace 6"] {
        let _ = crate::command::execute(f.niri_state(), command);
    }

    let outcome = crate::command::execute(f.niri_state(), "rename workspace 5 to 5: foo");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].parse_error, Some(true));
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("There is no workspace with that name")
    );
}

#[test]
fn rename_workspace_updates_name_number_and_rejects_collisions() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    for command in [
        "workspace 5",
        "rename workspace to 7: web",
        "workspace mail",
        "rename workspace mail to inbox",
        "rename workspace inbox to mail",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    let collision = crate::command::execute(f.niri_state(), "rename workspace mail to 7: web");
    assert!(!collision[0].success);
    assert_eq!(collision[0].parse_error, Some(true));
    for command in [
        "rename workspace mail to chat",
        "rename workspace chat to CHAT",
        "rename workspace chat to 9 web",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert!(!crate::command::execute(f.niri_state(), "rename workspace to next")[0].success);

    let swayward = f.swayward();
    assert_eq!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .map(|workspace| (workspace.num, workspace.name.as_str()))
            .collect::<Vec<_>>(),
        [(9, "9 web")]
    );
}

#[test]
fn tiled_and_floating_default_borders_remain_independent_in_get_tree() {
    let config = swayward_config::Config::parse_mem(
        r#"window-rule {
            sway-border "pixel"
            sway-border-width 5
            sway-floating-border "normal"
            sway-floating-border-width 2
        }
        window-rule {
            match app-id="floating"
            open-floating true
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();
    for app_id in ["tiled", "floating"] {
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
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let tiled = &workspace["nodes"][0];
    let floating = &workspace["floating_nodes"][0];
    assert_eq!(tiled["border"], "pixel");
    assert_eq!(tiled["current_border_width"], 5);
    assert_eq!(floating["border"], "normal");
    assert_eq!(floating["current_border_width"], 2);
}

#[test]
fn default_border_changes_only_windows_mapped_after_the_command() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();

    for app_id in ["existing", "explicit"] {
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
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="explicit"] border pixel 7"#,)[0]
            .success
    );
    assert!(crate::command::execute(f.niri_state(), "default_border pixel 3")[0].success);

    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("new".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let existing = find_json_node_with_app_id(&tree, "existing").unwrap();
    assert_eq!(existing["border"], "normal");
    assert_eq!(existing["current_border_width"], 4);
    let explicit = find_json_node_with_app_id(&tree, "explicit").unwrap();
    assert_eq!(explicit["border"], "pixel");
    assert_eq!(explicit["current_border_width"], 7);
    let new = find_json_node_with_app_id(&tree, "new").unwrap();
    assert_eq!(new["border"], "pixel");
    assert_eq!(new["current_border_width"], 3);
}

#[test]
fn default_floating_border_changes_only_windows_mapped_after_the_command() {
    let config = swayward_config::Config::parse_mem("window-rule { open-floating true; }").unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (800, 600));
    let client = f.add_client();

    for app_id in ["existing-float", "new-float"] {
        if app_id == "new-float" {
            assert!(
                crate::command::execute(f.niri_state(), "default_floating_border pixel 3",)[0]
                    .success
            );
        }
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
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let existing = find_json_node_with_app_id(&tree, "existing-float").unwrap();
    assert_eq!(existing["border"], "normal");
    assert_eq!(existing["current_border_width"], 4);
    let new = find_json_node_with_app_id(&tree, "new-float").unwrap();
    assert_eq!(new["border"], "pixel");
    assert_eq!(new["current_border_width"], 3);
}

#[test]
fn edge_border_modes_apply_to_workspace_edges_and_visible_view_count() {
    fn window_nodes(config: &str, windows: usize) -> Vec<Value> {
        let config = swayward_config::Config::parse_mem(config).unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (800, 600));
        let client = f.add_client();
        for _ in 0..windows {
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        let swayward = f.swayward();
        swayward.layout.update_render_elements(None);
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let workspace = &tree["nodes"][1]["nodes"][0];
        workspace["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .chain(workspace["floating_nodes"].as_array().unwrap())
            .cloned()
            .collect()
    }

    let config = |mode: &str, smart: &str, gaps: u8| {
        format!(
            r#"layout {{
                gaps {gaps}
                hide-edge-borders "{mode}"
                smart-borders "{smart}"
            }}
            window-rule {{ sway-border "pixel"; sway-border-width 7; }}"#
        )
    };

    let vertical = window_nodes(&config("vertical", "off", 0), 2);
    assert_eq!(vertical[0]["window_rect"]["x"], 0);
    assert_eq!(vertical[0]["window_rect"]["y"], 7);
    assert_eq!(vertical[0]["window_rect"]["width"], 393);
    assert_eq!(vertical[0]["window_rect"]["height"], 586);
    assert_eq!(vertical[1]["window_rect"]["x"], 7);
    assert_eq!(vertical[1]["window_rect"]["y"], 7);
    assert_eq!(vertical[1]["window_rect"]["width"], 393);
    assert_eq!(vertical[1]["window_rect"]["height"], 586);

    let horizontal = window_nodes(&config("horizontal", "off", 0), 2);
    assert_eq!(horizontal[0]["window_rect"]["x"], 7);
    assert_eq!(horizontal[0]["window_rect"]["y"], 0);
    assert_eq!(horizontal[0]["window_rect"]["width"], 386);
    assert_eq!(horizontal[0]["window_rect"]["height"], 600);
    assert_eq!(horizontal[1]["window_rect"]["x"], 7);
    assert_eq!(horizontal[1]["window_rect"]["y"], 0);
    assert_eq!(horizontal[1]["window_rect"]["width"], 386);
    assert_eq!(horizontal[1]["window_rect"]["height"], 600);

    let smart_single = window_nodes(&config("none", "on", 0), 1);
    assert_eq!(
        smart_single[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
    let smart_two = window_nodes(&config("none", "on", 0), 2);
    assert!(smart_two.iter().all(|node| {
        node["window_rect"] == serde_json::json!({ "x": 7, "y": 7, "width": 386, "height": 586 })
    }));
    let smart_and_edges = window_nodes(&config("both", "on", 0), 2);
    assert_eq!(
        smart_and_edges[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 393, "height": 600 })
    );
    assert_eq!(
        smart_and_edges[1]["window_rect"],
        serde_json::json!({ "x": 7, "y": 0, "width": 393, "height": 600 })
    );

    let no_gaps = window_nodes(&config("none", "no-gaps", 0), 1);
    assert_eq!(
        no_gaps[0]["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
    let with_gaps = window_nodes(&config("none", "no-gaps", 16), 1);
    assert_eq!(with_gaps[0]["window_rect"]["x"], 7);
    assert_eq!(with_gaps[0]["window_rect"]["y"], 7);

    let floating = window_nodes(
        &format!(
            "{}\nwindow-rule {{ open-floating true; }}",
            config("both", "on", 0)
        ),
        1,
    );
    assert_eq!(floating[0]["window_rect"]["x"], 7);
    assert_eq!(floating[0]["window_rect"]["y"], 7);
    assert_eq!(
        floating[0]["window_rect"]["width"].as_i64().unwrap(),
        floating[0]["rect"]["width"].as_i64().unwrap() - 14
    );
    assert_eq!(
        floating[0]["window_rect"]["height"].as_i64().unwrap(),
        floating[0]["rect"]["height"].as_i64().unwrap() - 14
    );
    assert_eq!(floating[0]["current_border_width"], 7);

    let initial = swayward_config::Config::parse_mem(&config("none", "off", 0)).unwrap();
    let mut f = Fixture::with_config(initial);
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
    f.niri_state()
        .reload_config(Ok(swayward_config::Config::parse_mem(&config(
            "both", "on", 0,
        ))
        .unwrap()));
    let swayward = f.swayward();
    swayward.layout.update_render_elements(None);
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let node = find_json_node(&tree, "con", false).unwrap();
    assert_eq!(
        node["window_rect"],
        serde_json::json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
}

#[test]
fn configured_border_width_matches_rendering_and_tree_for_tiled_and_floating_windows() {
    let config =
        swayward_config::Config::parse_mem(r#"layout { border { on; width 7; }; }"#).unwrap();
    let mut f = Fixture::with_config(config);
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

    for (node_type, floating) in [("con", false), ("floating_con", true)] {
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
        let swayward = f.swayward();
        let tile = swayward
            .layout
            .active_workspace()
            .unwrap()
            .tiles()
            .next()
            .unwrap();
        assert_eq!(tile.effective_border_width(), Some(7.));
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let node = find_json_node(&tree, node_type, false).unwrap();
        assert_eq!(node["border"], "normal");
        assert_eq!(node["current_border_width"], 7);
    }

    assert!(crate::command::execute(f.niri_state(), "border none")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let node = find_json_node(&tree, "floating_con", false).unwrap();
    assert_eq!(node["border"], "none");
    // Sway reports the retained thickness whatever the style
    // (sway/ipc-json.c:760-761).
    assert_eq!(node["current_border_width"], 7);
}

#[test]
fn border_command_updates_rendering_and_tree_metadata() {
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

    // The fixture's default thickness, which `border none` keeps.
    let default_width = {
        let swayward = f.swayward();
        let mapped = swayward.layout.focus().unwrap();
        let tile = swayward.layout.active_workspace().unwrap().tiles().next().unwrap();
        assert_eq!(tile.window().id(), mapped.id());
        i32::from(tile.sway_border_thickness().1)
    };
    for (command, style, stored_width, ipc_width, has_titlebar, rendered_width) in [
        ("border none", "none", 0, default_width, false, None),
        ("border pixel 3", "pixel", 3, 3, false, Some(3.)),
        ("border normal 5", "normal", 5, 5, true, Some(5.)),
        // Sway keeps the thickness across style changes without a width
        // (cmd_border, sway/commands/border.c:90-92; oracle
        // border_thickness_survives_style_changes). This test window has no
        // xdg-decoration, so the first toggle goes straight to none.
        ("border toggle", "none", 0, 5, false, None),
        ("border toggle", "pixel", 5, 5, false, Some(5.)),
        ("border toggle", "normal", 5, 5, true, Some(5.)),
        ("border pixel 4", "pixel", 4, 4, false, Some(4.)),
        ("border none", "none", 0, 4, false, None),
        ("border pixel", "pixel", 4, 4, false, Some(4.)),
        ("border normal", "normal", 4, 4, true, Some(4.)),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let swayward = f.swayward();
        let mapped = swayward.layout.focus().unwrap();
        assert_eq!(
            swayward.layout.window_border(&mapped.window),
            Some((
                match style {
                    "none" => swayward_ipc::command::BorderStyle::None,
                    "pixel" => swayward_ipc::command::BorderStyle::Pixel,
                    "normal" => swayward_ipc::command::BorderStyle::Normal,
                    _ => unreachable!(),
                },
                stored_width
            ))
        );
        let tile = swayward
            .layout
            .active_workspace()
            .unwrap()
            .tiles()
            .next()
            .unwrap();
        assert_eq!(tile.effective_border_width(), rendered_width);
        assert_eq!(tile.has_sway_titlebar(), has_titlebar);
        let tree = describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        );
        let node = tree
            .nodes
            .iter()
            .flat_map(|output| &output.nodes)
            .flat_map(|workspace| workspace.nodes.iter().chain(&workspace.floating_nodes))
            .next()
            .unwrap();
        assert_eq!(format!("{:?}", node.border).to_ascii_lowercase(), style);
        assert_eq!(node.current_border_width, ipc_width);
    }

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    // `border none` keeps the thickness the loop above left behind (4).
    for (command, style, width) in [("border none", "none", 4), ("border pixel 7", "pixel", 7)] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let node = find_json_node(&tree, "floating_con", false).unwrap();
        assert_eq!(node["border"], style, "{command}");
        assert_eq!(node["current_border_width"], width, "{command}");
    }
}

#[test]
fn border_csd_fails_without_client_decoration_support() {
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

    let outcome = crate::command::execute(f.niri_state(), "border csd");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("This window doesn't support client side decorations")
    );
}


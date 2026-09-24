#[test]
fn live_ipc_descriptions_match_sway_schema_and_values() {
    let config = swayward_config::Config::parse_mem(
        "layout { gaps 0; outer-gaps { left 0; right 0; top 0; bottom 0; }; border { on; width 2; }; }",
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1270, 1408));
    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let id = f.add_client();
    let window = f.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.set_title("fixture-1");
    window.set_size(696, 491);
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let mut stream = UnixStream::connect(&socket).unwrap();
    let ours = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let fixture: Value = serde_json::from_str(sway_fixture!("one_window.tree.json")).unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_same_values(&fixture, &ours, "$tree");
    assert_tree_rectangles_match_fixture(&fixture, &ours, "$tree");
    assert_focus_matches_fixture(&fixture, &ours, "$tree");
    assert_percent_matches_fixture(&fixture, &ours, "$tree");
    assert_eq!(
        ours["nodes"][1]["nodes"][0]["nodes"][0]["geometry"],
        fixture["nodes"][1]["nodes"][0]["nodes"][0]["geometry"],
        "tiled leaf geometry must remain the client's natural map-time geometry"
    );
    assert_eq!(
        fixture["nodes"][1]["nodes"][0]["representation"],
        ours["nodes"][1]["nodes"][0]["representation"],
        "workspace representation at $tree.nodes[1].nodes[0]"
    );

    let window = f.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-2".into());
    window.set_title("fixture-2");
    window.set_size(696, 491);
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let fixture: Value = serde_json::from_str(sway_fixture!("one_floating.tree.json")).unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_rectangle_roles_match_fixture(&fixture, &ours, "$tree");
    assert_focus_matches_fixture(&fixture, &ours, "$tree");
    let expected = find_json_node(&fixture, "floating_con", false).unwrap();
    let actual_percent = ours["nodes"][1]["nodes"][0]["floating_nodes"][0]["percent"]
        .as_f64()
        .unwrap();
    let expected_percent = expected["percent"].as_f64().unwrap();
    assert!(
        (actual_percent - expected_percent).abs() < 0.02,
        "floating percent: expected {expected_percent}, got {actual_percent}"
    );
    let floating = find_json_node(&ours, "floating_con", false).unwrap();
    assert_eq!(
        floating["deco_rect"]["height"].as_i64().unwrap() > 0,
        expected["deco_rect"]["height"].as_i64().unwrap() > 0
    );
    assert_eq!(floating["deco_rect"]["x"], floating["rect"]["x"]);
    assert_eq!(floating["deco_rect"]["width"], floating["rect"]["width"]);
    assert_eq!(floating["deco_rect"]["y"], floating["rect"]["y"]);

    let fixture_trees = [
        sway_fixture!("empty.tree.json"),
        sway_fixture!("empty_named.tree.json"),
        sway_fixture!("fullscreen.tree.json"),
        sway_fixture!("marked.tree.json"),
        sway_fixture!("named_workspace.tree.json"),
        sway_fixture!("nested_h_in_v.tree.json"),
        sway_fixture!("numbered_sparse.tree.json"),
        sway_fixture!("one_floating.tree.json"),
        sway_fixture!("one_window.tree.json"),
        sway_fixture!("stacked.tree.json"),
        sway_fixture!("tabbed.tree.json"),
        sway_fixture!("two_split_h.tree.json"),
        sway_fixture!("two_split_v.tree.json"),
        sway_fixture!("two_workspaces.tree.json"),
    ];
    let mut fixture_nodes = Vec::new();
    for fixture in fixture_trees {
        collect_fixture_nodes(&serde_json::from_str(fixture).unwrap(), &mut fixture_nodes);
    }
    assert_node_schema_appears_in_fixtures(&ours, &fixture_nodes, "$tree");

    let scratch = &ours["nodes"][0];
    assert_eq!(scratch["name"], "__i3");
    assert_eq!(scratch["nodes"][0]["name"], "__i3_scratch");
    assert!(ours["nodes"][1]["nodes"][0]["nodes"][0]["app_id"].is_string());

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    let fixture: Value =
        serde_json::from_str(sway_fixture!("one_floating.workspaces.json")).unwrap();
    assert_same_shape(&fixture, &ours, "$workspaces");
    let expected_focus = fixture[0]["focus"].as_array().unwrap();
    let actual_focus = ours[0]["focus"].as_array().unwrap();
    assert_eq!(expected_focus.len(), actual_focus.len());
    assert_eq!(ours[0]["floating_nodes"].as_array().unwrap().len(), 1);
    assert_eq!(ours[0]["floating_nodes"][0]["app_id"], "fixture-2");
    assert_eq!(ours[0]["focused"], true);
    assert_eq!(ours[0]["representation"], fixture[0]["representation"]);

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    let fixture: Value = serde_json::from_str(sway_fixture!("one_window.outputs.json")).unwrap();
    assert_same_shape(&fixture, &ours, "$outputs");
    assert_same_values(&fixture, &ours, "$outputs");

    let output_name = f.niri_output(1).name();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(workspaces.as_array().unwrap().len(), 1);
    assert_eq!(workspaces[0]["num"], 1);
    assert_eq!(workspaces[0]["name"], "1");
    assert_eq!(workspaces[0]["output"], output_name);

    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(outputs.as_array().unwrap().len(), 1);
    assert_eq!(outputs[0]["name"], output_name);

    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "mark fixture-mark",
        ))
        .unwrap();
    let (_, outcome) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&outcome).unwrap(),
        serde_json::json!([{"success": true}])
    );
    let marks = query_ipc(&mut f, &mut stream, MessageType::GetMarks);
    assert_eq!(marks, serde_json::json!(["fixture-mark"]));
}

#[test]
fn workspace_with_only_floating_windows_reports_empty_tiling_representation() {
    let mut f = Fixture::new();
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1270, 1408));
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

    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(workspaces[0]["representation"], "V[]");
}

#[test]
fn workspace_rect_includes_outer_and_edge_gaps() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 17.;
    config.layout.outer_gaps = swayward_config::OuterGaps::all(23.);
    config.layout.outer_gaps_configured = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1270, 1408));

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(
        tree["nodes"][1]["nodes"][0]["rect"],
        serde_json::json!({"x": 40, "y": 40, "width": 1190, "height": 1328})
    );
}

#[test]
fn split_children_report_their_arranged_share_including_gaps() {
    let mut f = Fixture::new();
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1270, 1408));
    assert!(crate::command::execute(f.niri_state(), "gaps inner all set 17")[0].success);
    assert!(crate::command::execute(f.niri_state(), "gaps outer all set 23")[0].success);
    let client = f.add_client();

    for app_id in ["fixture-1", "fixture-2", "fixture-3"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let children = tree["nodes"][1]["nodes"][0]["nodes"]
        .as_array()
        .unwrap();
    for (child, expected) in children.iter().zip([
        0.3245481927710843,
        0.3245481927710843,
        0.3253012048192771,
    ]) {
        let actual = child["percent"].as_f64().unwrap();
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }
}

#[test]
fn tabbed_children_report_visibility_and_full_parent_percent() {
    let mut f = Fixture::new();
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    for app_id in ["fixture-1", "fixture-2"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let first = find_json_node_with_app_id(&tree, "fixture-1").unwrap();
    let second = find_json_node_with_app_id(&tree, "fixture-2").unwrap();
    assert_eq!(first["visible"], false);
    assert_eq!(second["visible"], true);
    assert_eq!(first["percent"], 1.0);
    assert_eq!(second["percent"], 1.0);
}

#[test]
fn workspace_fullscreen_controls_focus_visibility_and_percent() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
    let client = f.add_client();

    let map = |f: &mut Fixture, app_id: &str| {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };

    map(&mut f, "first");
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);
    map(&mut f, "second");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let first = find_json_node_with_app_id(&tree, "first").unwrap();
    let second = find_json_node_with_app_id(&tree, "second").unwrap();
    assert_eq!(first["focused"], true);
    assert_eq!(first["visible"], true);
    assert_eq!(first["percent"], 1.0);
    assert_eq!(second["focused"], false);
    assert_eq!(second["visible"], false);
    assert_eq!(second["percent"], 0.0);
    assert_eq!(second["border"], "none");
    assert_eq!(second["current_border_width"], 0);

}

#[test]
fn toggling_fullscreen_updates_sibling_visibility_and_percent() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 800));
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
    }
    assert!(crate::command::execute(f.niri_state(), "fullscreen toggle")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let first = find_json_node_with_app_id(&tree, "first").unwrap();
    let second = find_json_node_with_app_id(&tree, "second").unwrap();
    assert_eq!(first["focused"], false);
    assert_eq!(first["visible"], false);
    assert_eq!(first["percent"], 0.5);
    assert_eq!(second["focused"], true);
    assert_eq!(second["visible"], true);
    assert_eq!(second["percent"], 1.0);
    assert_eq!(second["deco_rect"]["height"], 0);
}

#[test]
fn nested_tabbed_children_report_arranged_area_share() {
    let mut f = Fixture::new();
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
    for app_id in ["fixture-1", "fixture-2"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }
    for command in [
        "focus parent",
        "layout stacking",
        "[app_id=\"^fixture-1$\"] focus",
        "splith",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-3".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    for command in [
        "focus parent",
        "layout tabbed",
        "[app_id=\"^fixture-1$\"] focus",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let split = find_json_parent_of_app_id(&tree, "fixture-1").unwrap();
    let parent = find_json_parent_of_app_id(&tree, "fixture-2").unwrap();
    let percent = split["percent"].as_f64().unwrap();
    assert!(percent < 1., "nested tab child must not report full-parent percent");
    assert_eq!(parent["nodes"][1]["percent"], 1.0);
}

#[test]
fn split_containers_report_sway_container_state_fields() {
    let tree = nested_representation_live_tree();
    let split = find_json_parent_of_app_id(&tree, "fixture-3").unwrap();
    assert_eq!(split["type"], "con");
    assert_eq!(split["floating"], "auto_off");
    assert_eq!(split["scratchpad_state"], "none");
}

#[test]
fn emptied_workspace_is_recreated_with_default_layout() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);
    let mut surfaces = Vec::new();
    for app_id in ["fixture-1", "fixture-2"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
    }
    for surface in &surfaces {
        let window = f.client(client).window(surface);
        window.attach_null();
        window.commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "workspace __fixture_reset")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("recreated".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspace = find_json_parent_of_app_id(&tree, "recreated").unwrap();
    assert_eq!(workspace["type"], "workspace");
    assert_eq!(workspace["name"], "1");
    assert_eq!(workspace["layout"], "splitv");
}

#[test]
fn layout_on_a_focused_nested_split_does_not_promote_to_the_workspace_root() {
    for (outer, expected_outer, inner, expected_inner) in [
        ("stacking", "stacked", "tabbed", "tabbed"),
        ("tabbed", "tabbed", "stacking", "stacked"),
    ] {
        let mut f = Fixture::new();
        let handle = f.swayward().event_loop.clone();
        let ipc_server =
            crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
        let socket = ipc_server.socket_path.clone().unwrap();
        f.swayward().ipc_server = Some(ipc_server);
        f.niri_state().ipc_keyboard_layouts_changed();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();

        assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
        for app_id in ["fixture-1", "fixture-2"] {
            let window = f.client(client).create_window();
            window.xdg_toplevel.set_app_id(app_id.into());
            let surface = window.surface.clone();
            window.commit();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(crate::command::execute(f.niri_state(), &format!("layout {outer}"))[0].success);
        assert!(crate::command::execute(f.niri_state(), r#"[app_id="^fixture-1$"] focus"#)[0].success);
        assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("fixture-3".into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(crate::command::execute(f.niri_state(), &format!("layout {inner}"))[0].success);
        f.niri_state().ipc_refresh_layout();

        let mut stream = UnixStream::connect(socket).unwrap();
        let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
        let workspace = &tree["nodes"][1]["nodes"][0];
        assert_eq!(workspace["layout"], expected_outer);
        assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(workspace["nodes"][0]["layout"], expected_inner);
        assert_eq!(workspace["nodes"][0]["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(workspace["nodes"][0]["nodes"][0]["layout"], "splith");
        assert_eq!(
            workspace["nodes"][0]["nodes"][0]["nodes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
}

#[test]
fn focus_parent_then_layout_targets_the_parent_of_the_focused_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for command in [None, Some("split v")] {
        if let Some(command) = command {
            assert!(crate::command::execute(f.niri_state(), command)[0].success);
        }
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    // `split v` retargets the singleton workspace root rather than wrapping it
    // (sway container.c:1565), so the two windows sit directly under the
    // workspace. `focus parent` then focuses that root, and `layout tabbed`
    // targets its parent, the workspace itself.
    assert_eq!(workspace["layout"], "tabbed");
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 2);
    assert!(workspace["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["focused"] == false));
}

#[test]
fn focus_child_from_workspace_restores_the_floating_child() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for floating in [false, true] {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if floating {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }
    let floating = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_workspace_focused());
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), floating);

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    assert_ne!(f.swayward().layout.focus().unwrap().id(), floating);
}

#[test]
fn focused_container_can_be_marked_and_targeted_by_con_id() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    f.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark parent")[0].success);
    let swayward = f.swayward();
    assert!(!swayward.marks_by_container.is_empty());
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let parent = find_json_node_with_mark(&tree, "parent").unwrap();
    let parent_id = parent["id"].as_i64().unwrap();

    let outcome = crate::command::execute(
        f.niri_state(),
        &format!("[con_id={parent_id}] layout tabbed"),
    );
    assert!(outcome[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["layout"],
        "tabbed"
    );

    let outcome = crate::command::execute(f.niri_state(), "[con_id=__focused__] layout stacked");
    assert!(outcome[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["layout"],
        "stacked"
    );
}

#[test]
fn criteria_split_commands_apply_to_every_matched_window() {
    for (command, expected_layout) in [
        ("split vertical", "splitv"),
        ("splitv", "splitv"),
        ("splith", "splith"),
        ("splitt", "splitv"),
    ] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        for app_id in ["matched-1", "other", "matched-2", "matched-3"] {
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

        let outcome =
            crate::command::execute(f.niri_state(), &format!("[app_id=matched-] {command}"));
        assert!(outcome[0].success, "{command}: {outcome:?}");

        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        for app_id in ["matched-1", "matched-2", "matched-3"] {
            let parent = find_json_parent_of_app_id(&tree, app_id).unwrap();
            assert_eq!(parent["layout"], expected_layout, "{command}: {app_id}");
            assert_eq!(
                parent["nodes"].as_array().unwrap().len(),
                1,
                "{command}: {app_id}"
            );
        }
        let other_parent = find_json_parent_of_app_id(&tree, "other").unwrap();
        assert_eq!(other_parent["layout"], "splith", "{command}: non-match");
        assert_eq!(
            other_parent["nodes"].as_array().unwrap().len(),
            4,
            "{command}: non-match"
        );
    }
}

#[test]
fn split_none_flattens_only_a_singleton_parent_and_preserves_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    map_test_window(&mut f, client, "second");
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let before = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .ipc_tiling_tree();
    assert_eq!(before.nodes().len(), 4);

    let outcome = crate::command::execute(f.niri_state(), "split none");
    assert!(outcome[0].success, "{outcome:?}");
    {
        let workspace = f.swayward().layout.active_workspace().unwrap();
        assert_eq!(workspace.ipc_tiling_tree().nodes().len(), 3);
        workspace.verify_invariants(None);
    }
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);

    let before = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .ipc_tiling_tree();
    let outcome = crate::command::execute(f.niri_state(), "split none");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Can only flatten a child container with no siblings".into()),
            parse_error: None,
        }]
    );
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.ipc_tiling_tree(), before);
    workspace.verify_invariants(None);
}

#[test]
fn criteria_split_none_flattens_the_matched_singleton_parent() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    map_test_window(&mut f, client, "matched");
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "[app_id=matched] split none");
    assert!(outcome[0].success, "{outcome:?}");
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.ipc_tiling_tree().nodes().len(), 3);
    workspace.verify_invariants(None);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
}

#[test]
fn criteria_split_command_applies_to_a_matched_split_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    f.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark split-target")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "[con_mark=split-target] split vertical");
    assert!(outcome[0].success, "{outcome:?}");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let marked = find_json_node_with_mark(&tree, "split-target").unwrap();
    fn find_parent(value: &Value, id: i64) -> Option<&Value> {
        for key in ["nodes", "floating_nodes"] {
            let children = value[key].as_array()?;
            if children.iter().any(|child| child["id"] == id) {
                return Some(value);
            }
            if let Some(parent) = children.iter().find_map(|child| find_parent(child, id)) {
                return Some(parent);
            }
        }
        None
    }
    let parent = find_parent(&tree, marked["id"].as_i64().unwrap()).unwrap();
    assert_eq!(parent["layout"], "splitv");
    assert_eq!(parent["nodes"].as_array().unwrap().len(), 1);
}

#[test]
fn criteria_layout_applies_to_every_matched_windows_container() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for workspace in ["one", "two"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for app_id in ["matched-layout", "other"] {
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
    }

    for (command, expected) in [
        ("layout tabbed", TreeLayout::Tabbed),
        ("layout default", TreeLayout::SplitH),
        ("layout toggle all", TreeLayout::SplitV),
    ] {
        let outcome = crate::command::execute(
            f.niri_state(),
            &format!("[app_id=matched-layout] {command}"),
        );
        assert!(outcome[0].success, "{command}: {outcome:?}");
        for workspace in ["one", "two"] {
            let tree = f
                .swayward()
                .layout
                .workspaces()
                .find(|(_, _, candidate)| candidate.sway_name().as_deref() == Some(workspace))
                .unwrap()
                .2
                .ipc_tiling_tree();
            assert!(
                matches!(
                    tree,
                    IpcNode::Split {
                        layout,
                        ..
                    } if layout == expected
                ),
                "{command}: {workspace}"
            );
        }
    }
}

#[test]
fn criteria_fullscreen_applies_to_every_matched_split_and_its_descendants() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut windows = Vec::new();
    for workspace in ["one", "two"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for app_id in ["outside", "inside-1", "inside-2"] {
            map_test_window(&mut f, client, app_id);
            windows.push((
                workspace,
                app_id,
                f.swayward().layout.focus().unwrap().window.clone(),
            ));
        }
        f.swayward().layout.nest_or_unnest_window_left(None);
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!("mark fullscreen-group-{workspace}")
            )[0]
            .success
        );
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        "[con_mark=fullscreen-group-] fullscreen enable",
    );
    assert!(outcome[0].success, "{outcome:?}");
    for (workspace, app_id, window) in &windows {
        assert_eq!(
            f.swayward().layout.fullscreen_mode(window),
            (*app_id != "outside").then_some(crate::layout::tiling_tree::FullscreenMode::Workspace),
            "{workspace}: {app_id}"
        );
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        "[con_mark=fullscreen-group-] fullscreen disable",
    );
    assert!(outcome[0].success, "{outcome:?}");
    for (workspace, app_id, window) in windows {
        assert_eq!(
            f.swayward().layout.fullscreen_mode(&window),
            None,
            "{workspace}: {app_id}"
        );
    }
}

#[test]
fn criteria_kill_closes_every_descendant_of_every_matched_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for workspace in ["one", "two"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for app_id in ["outside", "inside-1", "inside-2"] {
            let window = f.client(client).create_window();
            window.xdg_toplevel.set_app_id(app_id.into());
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
            surfaces.push((app_id, surface));
        }
        f.swayward().layout.nest_or_unnest_window_left(None);
        assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
        assert!(
            crate::command::execute(f.niri_state(), &format!("mark kill-group-{workspace}"))[0]
                .success
        );
    }

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=kill-group-] kill");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    for (app_id, surface) in surfaces {
        assert_eq!(
            f.client(client).window(&surface).close_requested,
            app_id != "outside",
            "{app_id}"
        );
    }
}

#[test]
fn kill_closes_every_descendant_of_the_focused_split() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
    }
    f.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "kill");
    assert!(outcome[0].success, "{outcome:?}");
    f.double_roundtrip(client);
    assert!(!f.client(client).window(&surfaces[0]).close_requested);
    assert!(f.client(client).window(&surfaces[1]).close_requested);
    assert!(f.client(client).window(&surfaces[2]).close_requested);
}

#[test]
fn container_mark_survives_singleton_flattening() {
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

    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark survivor")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout toggle split")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=survivor] focus");
    assert!(outcome[0].success, "{outcome:?}");
}

#[test]
fn view_criteria_exclude_splits_but_container_criteria_include_them() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["first", "second", "third"] {
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

    f.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark split")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let split_id = find_json_node_with_mark(&tree, "split").unwrap()["id"]
        .as_i64()
        .unwrap();

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=split] layout tabbed");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error, None);
    let outcome = crate::command::execute(
        f.niri_state(),
        &format!("[con_id={split_id}] layout stacking"),
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error, None);

    let outcome = crate::command::execute(f.niri_state(), "[all] kill");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error, None);
}

#[test]
fn mark_on_empty_workspace_fails_without_removing_existing_mark() {
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

    assert!(crate::command::execute(f.niri_state(), "mark keepme")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "mark keepme");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Only containers can have marks")
    );
    assert_eq!(
        f.swayward().marks_by_window.values().next().unwrap(),
        &["keepme".to_owned()]
    );
}

#[test]
fn focused_leaf_con_id_matches_get_tree_and_focused_criteria() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("focused-leaf".into());
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
    let focused_id = find_json_node(&tree, "con", true).unwrap()["id"]
        .as_i64()
        .unwrap();

    for (criterion, mark) in [
        (format!(r#"con_id={focused_id}"#), "numeric"),
        ("con_id=__focused__".to_owned(), "focused"),
    ] {
        let result = crate::command::execute(
            f.niri_state(),
            &format!(r#"[{criterion} app_id="focused-leaf"] mark {mark}"#),
        );
        assert!(result[0].success, "{criterion}: {result:?}");
    }
    let focused = f.swayward().layout.focus().unwrap().id();
    assert_eq!(
        f.swayward().marks_by_window.get(&focused).unwrap(),
        &["focused".to_owned()]
    );

    crate::command::execute(f.niri_state(), &format!(r#"[id={focused_id}] mark x11-id"#));
    assert_eq!(
        f.swayward().marks_by_window.get(&focused).unwrap(),
        &["focused".to_owned()],
        "a native Wayland view must not expose its con_id as an X11 window id"
    );

    let result = crate::command::execute(f.niri_state(), "[con_id=not-a-number] nop");
    assert_eq!(result[0].parse_error, Some(true));
    assert_eq!(
        result[0].error.as_deref(),
        Some("The value for 'con_id' should be '__focused__' or numeric")
    );
}

#[test]
fn swap_con_id_and_mark_preserve_focus_and_reject_invalid_targets() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut ids = Vec::new();
    for name in ["first", "second", "third"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(name.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        ids.push(f.swayward().layout.focus().unwrap().id());
    }
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("[con_id={}] focus", crate::ipc::tree::window_id(ids[0]))
        )[0]
        .success
    );
    assert!(crate::command::execute(f.niri_state(), "mark target")[0].success);
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("[con_id={}] focus", crate::ipc::tree::window_id(ids[2]))
        )[0]
        .success
    );
    let focused = f.swayward().layout.focus().unwrap().id();

    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!(
                "swap container with con_id {}",
                crate::ipc::tree::window_id(ids[1])
            )
        )[0]
        .success
    );
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert!(crate::command::execute(f.niri_state(), "swap container with mark target")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);

    let x11_id = crate::ipc::tree::window_id(ids[0]);
    let unsupported_id =
        crate::command::execute(f.niri_state(), &format!("swap container with id {x11_id}"));
    assert_eq!(unsupported_id[0].parse_error, Some(true));
    assert_eq!(
        unsupported_id[0].error.as_deref(),
        Some("swap container with id is unsupported because X11 window IDs are unavailable")
    );

    assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
    let fourth = f.client(client).create_window();
    fourth.xdg_toplevel.set_app_id("fourth".into());
    fourth.commit();
    let surface = fourth.surface.clone();
    f.roundtrip(client);
    let fourth = f.client(client).window(&surface);
    fourth.attach_new_buffer();
    fourth.ack_last_and_commit();
    f.double_roundtrip(client);
    let fourth = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    let parent = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .focused_container_node()
        .unwrap();
    let child = crate::ipc::tree::window_id(fourth);
    let result = crate::command::execute(
        f.niri_state(),
        &format!("swap container with con_id {child}"),
    );
    assert_eq!(
        result[0].error.as_deref(),
        Some("Cannot swap ancestor and descendant")
    );
    assert_eq!(
        f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .focused_container_node(),
        Some(parent)
    );

    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    let self_id = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    for (command, expected) in [
        (
            "swap container with con_id 999",
            "Failed to find con_id '999'",
        ),
        (
            &format!("swap container with con_id {self_id}"),
            "Cannot swap a container with itself",
        ),
    ] {
        let result = crate::command::execute(f.niri_state(), command);
        assert_eq!(result[0].error.as_deref(), Some(expected));
    }
}

#[test]
fn map_time_marks_remain_globally_unique() {
    let mut config = swayward_config::Config::default();
    for app_id in ["first", "second"] {
        config.window_rules.push(swayward_config::WindowRule {
            matches: vec![swayward_config::window_rule::Match {
                app_id: Some(format!("^{app_id}$").parse().unwrap()),
                ..Default::default()
            }],
            sway_for_window_commands: vec!["mark --add shared".into()],
            ..Default::default()
        });
    }
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
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
    }

    assert_eq!(
        f.swayward()
            .marks_by_window
            .values()
            .filter(|marks| marks.iter().any(|mark| mark == "shared"))
            .count(),
        1
    );
}

#[test]
fn marks_are_globally_unique_across_windows_and_containers() {
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

    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark keep")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark --add unique")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace second")[0].success);
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
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark --toggle unique")[0].success);

    fn mark_count(state: &crate::swayward::State, expected: &str) -> usize {
        state
            .swayward
            .marks_by_window
            .values()
            .chain(state.swayward.marks_by_container.values())
            .flatten()
            .filter(|mark| mark.as_str() == expected)
            .count()
    }
    assert_eq!(mark_count(f.niri_state(), "unique"), 1);
    assert_eq!(mark_count(f.niri_state(), "keep"), 1);

    assert!(crate::command::execute(f.niri_state(), "mark --toggle unique")[0].success);
    assert_eq!(mark_count(f.niri_state(), "unique"), 0);
    assert_eq!(mark_count(f.niri_state(), "keep"), 1);

    assert!(crate::command::execute(f.niri_state(), "mark unique")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark unique")[0].success);
    assert_eq!(mark_count(f.niri_state(), "unique"), 1);
    assert!(f
        .swayward()
        .marks_by_container
        .values()
        .all(|marks| !marks.iter().any(|mark| mark == "unique")));

    assert!(crate::command::execute(f.niri_state(), "workspace empty")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "mark unique");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(mark_count(f.niri_state(), "unique"), 1);
}

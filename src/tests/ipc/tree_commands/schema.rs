#[test]
fn get_tree_reports_sway_default_floating_rules() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    for (name, min_size, max_size, expected_floating) in [
        ("fixed-width", (300, 100), (300, 200), true),
        ("fixed-height-zero-width", (0, 200), (0, 200), false),
        ("fixed-both", (300, 200), (300, 200), true),
    ] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(name.into());
        window.set_min_size(min_size.0, min_size.1);
        window.set_max_size(max_size.0, max_size.1);
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        let mut stream = UnixStream::connect(&socket).unwrap();
        let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
        let node = find_json_node_with_app_id(&tree, name).unwrap();
        assert_eq!(
            node["floating"],
            if expected_floating { "user_on" } else { "auto_off" },
            "GET_TREE floating state for {name}"
        );
    }

    let parent = f.client(client).create_window();
    parent.xdg_toplevel.set_app_id("parent".into());
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);

    let dialog = f.client(client).create_window();
    dialog.xdg_toplevel.set_app_id("dialog".into());
    dialog.set_parent(Some(&parent_toplevel));
    let dialog_surface = dialog.surface.clone();
    dialog.commit();
    f.roundtrip(client);
    let dialog = f.client(client).window(&dialog_surface);
    dialog.attach_new_buffer();
    dialog.ack_last_and_commit();
    f.double_roundtrip(client);

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node_with_app_id(&tree, "dialog").unwrap()["floating"],
        "user_on"
    );
}

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
    let fixture: Value = serde_json::from_str(&sway_fixture!("one_window.tree.json")).unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_same_values(&fixture, &ours, "$tree");
    assert_tree_rectangles_match_fixture(&fixture, &ours, "$tree");
    assert_focus_matches_fixture(&fixture, &ours, "$tree");
    assert_percent_matches_fixture(&fixture, &ours, "$tree");
    assert_eq!(
        ours["nodes"][1]["nodes"][0]["nodes"][0]["geometry"],
        serde_json::json!({"x": 0, "y": 0, "width": 696, "height": 491}),
        "tiled leaf geometry must remain the requested map-time geometry"
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
    let fixture: Value = serde_json::from_str(&sway_fixture!("one_floating.tree.json")).unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_rectangle_roles_match_fixture(&fixture, &ours, "$tree");
    assert_focus_matches_fixture(&fixture, &ours, "$tree");
    let expected = find_json_node(&fixture, "floating_con", false).unwrap();
    // The oracle normalizes floating percent because sway's default floating
    // rectangle varies between fresh runs.
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
        collect_fixture_nodes(&serde_json::from_str(&fixture).unwrap(), &mut fixture_nodes);
    }
    assert_node_schema_appears_in_fixtures(&ours, &fixture_nodes, "$tree");

    let scratch = &ours["nodes"][0];
    assert_eq!(scratch["name"], "__i3");
    assert_eq!(scratch["nodes"][0]["name"], "__i3_scratch");
    assert!(ours["nodes"][1]["nodes"][0]["nodes"][0]["app_id"].is_string());

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    let fixture: Value =
        serde_json::from_str(&sway_fixture!("one_floating.workspaces.json")).unwrap();
    assert_same_shape(&fixture, &ours, "$workspaces");
    let expected_focus = fixture[0]["focus"].as_array().unwrap();
    let actual_focus = ours[0]["focus"].as_array().unwrap();
    assert_eq!(expected_focus.len(), actual_focus.len());
    assert_eq!(ours[0]["floating_nodes"].as_array().unwrap().len(), 1);
    assert_eq!(ours[0]["floating_nodes"][0]["app_id"], "fixture-2");
    assert_eq!(ours[0]["focused"], true);
    assert_eq!(ours[0]["representation"], fixture[0]["representation"]);

    let ours = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    let fixture: Value = serde_json::from_str(&sway_fixture!("one_window.outputs.json")).unwrap();
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
fn moved_workspace_keeps_destination_output_focus_order() {
    let config = swayward_config::Config::parse_mem(
        r#"
        output "headless-1" { mode custom=true "1270x1408@60"; scale 1; }
        output "headless-2" { mode custom=true "1270x1408@60"; scale 1; }
        "#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    assert!(crate::command::execute(f.niri_state(), "workspace __fixture_reset")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    let client = f.add_client();

    map_test_window(&mut f, client, "fixture-1");
    assert!(crate::command::execute(f.niri_state(), "move workspace to output right")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "fixture-2");
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let output_focus = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|output| {
            let workspaces = output["nodes"].as_array().unwrap();
            let workspace_names = workspaces
                .iter()
                .map(|workspace| workspace["name"].as_str().unwrap())
                .collect::<Vec<_>>();
            let focused_names = output["focus"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    workspaces
                        .iter()
                        .find(|workspace| workspace["id"] == *id)
                        .unwrap()["name"]
                        .as_str()
                        .unwrap()
                })
                .collect::<Vec<_>>();
            (workspace_names, focused_names)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        output_focus,
        [
            (vec!["3"], vec!["3"]),
            (vec!["1", "2"], vec!["2", "1"]),
        ]
    );
}

#[test]
fn active_emptied_workspace_retains_its_layout_and_representation() {
    let mut f = Fixture::new();
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
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
    assert!(crate::command::execute(f.niri_state(), "move down")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(workspaces[0]["layout"], "splitv");
    assert_eq!(workspaces[0]["representation"], "V[]");
}

#[test]
fn scratchpad_tree_preserves_insertion_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();

    for app_id in ["first", "second"] {
        map_test_window(&mut f, client, app_id);
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let scratchpad = tree["nodes"][0]["nodes"][0]["floating_nodes"]
        .as_array()
        .unwrap();
    let app_ids = scratchpad
        .iter()
        .map(|node| node["app_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(app_ids, ["first", "second"]);
    assert_eq!(
        tree["nodes"][0]["nodes"][0]["focus"],
        serde_json::json!([scratchpad[1]["id"], scratchpad[0]["id"]])
    );
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
fn layout_tabbed_preserves_fullscreen_pending_percentages() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    for app_id in ["first", "second"] {
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
    for command in ["[app_id=\"^first$\"] focus", "fullscreen toggle"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let mut stream = UnixStream::connect(&socket).unwrap();
    let before = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let second_rect = find_json_node_with_app_id(&before, "second").unwrap()["rect"].clone();

    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let wrapper = find_json_parent_of_app_id(&tree, "first").unwrap();
    assert_eq!(wrapper["percent"], 0.0);
    assert_eq!(wrapper["nodes"][0]["percent"], Value::Null);
    assert_eq!(wrapper["nodes"][1]["percent"], Value::Null);
    assert_eq!(wrapper["nodes"][1]["rect"], second_rect);
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


#[test]
fn initial_workspace_keeps_pre_mode_orientation_and_later_workspace_uses_configured_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"output "headless-1" { mode custom=true "1270x1408@60"; }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    for (workspace, app_id) in [(None, "initial"), (Some("2"), "later")] {
        if let Some(workspace) = workspace {
            assert!(crate::command::execute(
                f.niri_state(),
                &format!("workspace {workspace}")
            )[0]
            .success);
        }
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

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(
        find_json_parent_of_app_id(&tree, "initial").unwrap()["layout"],
        "splith"
    );
    assert_eq!(
        find_json_parent_of_app_id(&tree, "later").unwrap()["layout"],
        "splitv"
    );
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
fn configured_workspace_layout_uses_sways_layout_field_without_an_i3_alias() {
    let mut config = swayward_config::Config::default();
    config.layout.workspace_layout = swayward_config::WorkspaceLayout::Tabbed;
    let mut f = Fixture::with_config(config);
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

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["nodes"][0]["layout"], "tabbed");
    assert!(workspace.get("workspace_layout").is_none());
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
        let outer = &workspace["nodes"][0]["rect"];
        let inner = &workspace["nodes"][0]["nodes"][0]["rect"];
        assert_eq!(outer["y"], 44);
        assert_eq!(outer["height"], 1036);
        assert_eq!(inner["y"], if expected_inner == "stacked" { 110 } else { 66 });
        assert_eq!(inner["height"], if expected_inner == "stacked" { 970 } else { 1014 });
    }
}

#[test]
fn layout_splitv_wraps_a_single_window_without_changing_the_workspace_axis() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["orientation"], "horizontal");
    assert_eq!(workspace["representation"], "H[V[fixture-1]]");
    assert_eq!(workspace["nodes"][0]["layout"], "splitv");
    assert_eq!(workspace["nodes"][0]["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn splitting_a_focused_container_keeps_it_nested() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(crate::command::execute(f.niri_state(), "workspace resize-levels")[0].success);
    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    map_test_window(&mut f, client, "second");
    map_test_window(&mut f, client, "third");
    map_test_window(&mut f, client, "fourth");
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "split h")[0].success);
    map_test_window(&mut f, client, "fifth");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["representation"], "H[H[first second third fourth] fifth]");
    assert_eq!(workspace["nodes"][0]["layout"], "splith");
}

#[test]
fn removing_one_of_two_split_windows_preserves_the_wrapper() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "fixture-1");
    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    map_test_window(&mut f, client, "fixture-2");

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["representation"], "H[V[fixture-1]]");
    assert_eq!(workspace["nodes"][0]["layout"], "splitv");
    assert_eq!(workspace["nodes"][0]["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn moving_a_single_window_in_its_split_direction_reaps_the_old_wrapper() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move down")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splitv");
    assert_eq!(workspace["representation"], "V[fixture-1]");
    assert_eq!(workspace["nodes"][0]["layout"], "none");
    assert_eq!(workspace["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn moving_a_single_window_across_its_split_direction_rewraps_the_workspace_children() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "layout splitv")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move right")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["layout"], "splith");
    assert_eq!(workspace["representation"], "H[V[fixture-1]]");
    assert_eq!(workspace["nodes"][0]["layout"], "splitv");
    assert_eq!(workspace["nodes"][0]["nodes"][0]["app_id"], "fixture-1");
}

#[test]
fn fullscreen_floating_window_keeps_sways_raw_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "fixture-1").unwrap()["focused"],
        true
    );
}

#[test]
fn layout_commands_apply_to_children_inside_a_floating_group() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["fixture-1", "fixture-2"] {
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
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus child")[0].success);

    for (command, expected) in [
        ("layout splitv", crate::layout::tiling_tree::Layout::SplitV),
        ("layout tabbed", crate::layout::tiling_tree::Layout::Tabbed),
        (
            "layout stacking",
            crate::layout::tiling_tree::Layout::Stacked,
        ),
        (
            "layout toggle split",
            crate::layout::tiling_tree::Layout::SplitV,
        ),
        ("layout default", crate::layout::tiling_tree::Layout::SplitV),
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        let workspace = f.swayward().layout.active_workspace().unwrap();
        let (_, tree, _) = workspace.ipc_floating_trees().next().unwrap();
        assert!(
            matches!(
                tree,
                crate::layout::tiling_tree::IpcNode::Split { layout, .. } if layout == expected
            ),
            "{command}: {tree:?}"
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

    let outcome = crate::command::execute(f.niri_state(), "[con_id=__focused__] layout stacking");
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
fn wrapping_a_tiled_child_does_not_promote_it_past_an_older_floating_child() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["floating", "tiled"] {
        map_test_window(&mut f, client, app_id);
        if app_id == "floating" {
            assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "layout stacking")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    assert_eq!(workspace["focus"][0], workspace["floating_nodes"][0]["id"]);
}


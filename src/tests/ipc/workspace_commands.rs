#[test]
fn closing_last_window_focuses_workspace_node() {
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

    let mapped_id = f
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id())
                .then(|| mapped.id())
        })
        .unwrap();
    let focused_window = crate::ipc::tree::window_id(mapped_id);
    let focused_workspace =
        crate::ipc::tree::workspace_id(f.swayward().layout.active_workspace().unwrap().id().get());

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [focused_window]);

    let window = f.client(client).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    focused.clear();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused, [focused_workspace]);
}

#[test]
fn get_tree_has_one_focused_node_after_scratchpad_cycle() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["scratch", "tiled"] {
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
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("inactive".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);

    for command in [
        r#"[app_id="scratch"] move scratchpad"#,
        "scratchpad show",
        "scratchpad show",
        "scratchpad show",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(focused.len(), 1, "focused nodes: {focused:?}");
    let focused_id = focused[0];
    let focused_workspace = tree
        .nodes
        .iter()
        .flat_map(|output| &output.nodes)
        .find(|workspace| {
            workspace
                .nodes
                .iter()
                .chain(&workspace.floating_nodes)
                .any(|node| node.id == focused_id)
        })
        .unwrap();
    assert_eq!(focused_workspace.focus.first(), Some(&focused_id));

    assert!(crate::command::execute(f.niri_state(), "workspace empty")[0].success);
    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut focused = Vec::new();
    collect_focused_nodes(&tree, &mut focused);
    assert_eq!(
        focused,
        [crate::ipc::tree::workspace_id(
            swayward.layout.active_workspace().unwrap().id().get()
        )]
    );
}

#[test]
fn scratchpad_hides_focused_window_and_show_cycles_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        surfaces.push(surface);
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    let swayward = f.swayward();
    assert!(swayward.layout.focus().is_none());
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert_eq!(tree.nodes[0].nodes[0].floating_nodes.len(), 2);
    assert!(tree.nodes[0].nodes[0]
        .floating_nodes
        .iter()
        .all(|node| node.scratchpad_state.as_deref() == Some("fresh")));
    assert_eq!(tree.nodes[0].nodes[0].fullscreen_mode, 1);
    assert_eq!(
        tree.nodes[0].nodes[0].focus,
        tree.nodes[0].nodes[0]
            .floating_nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>()
    );

    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let first = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let second = f.swayward().layout.focus().unwrap().id();
    assert_ne!(first, second);
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 2);
    assert_eq!(surfaces.len(), 2);
}

#[test]
fn directional_move_emits_one_settled_sway_move_event() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    for app_id in ["left", "moved"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.set_title(app_id);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let moved_id = f.swayward().layout.focus().unwrap().id();
    let before = {
        let swayward = f.swayward();
        serde_json::to_value(crate::ipc::tree::describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap()
    };
    let before =
        super::super::ipc::server::find_node_by_id(&before, crate::ipc::tree::window_id(moved_id))
            .unwrap()["rect"]["x"]
            .as_i64()
            .unwrap();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "move");
    assert_eq!(
        event["container"]["id"],
        crate::ipc::tree::window_id(moved_id)
    );
    assert!(event["container"]["rect"]["x"].as_i64().unwrap() < before);
    let expected: Value = serde_json::from_str(sway_fixture!("events/window.move.json")).unwrap();
    assert_event_shape(&expected, &event, "$window");
}

#[test]
fn scratchpad_show_moves_visible_window_to_current_workspace_and_focuses_it() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let scratchpad = f.client(client).create_window();
    scratchpad.xdg_toplevel.set_app_id("event-one".into());
    scratchpad.set_title("event-one");
    scratchpad.commit();
    let scratchpad_surface = scratchpad.surface.clone();
    f.roundtrip(client);
    let scratchpad = f.client(client).window(&scratchpad_surface);
    scratchpad.attach_new_buffer();
    scratchpad.ack_last_and_commit();
    f.double_roundtrip(client);
    let scratchpad_id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move to scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace target")[0].success);

    let tiled = f.client(client).create_window();
    tiled.commit();
    let tiled_surface = tiled.surface.clone();
    f.roundtrip(client);
    let tiled = f.client(client).window(&tiled_surface);
    tiled.attach_new_buffer();
    tiled.ack_last_and_commit();
    f.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let focused = f.swayward().layout.focus().unwrap();
    assert_eq!(focused.id(), scratchpad_id);
    let focused_window = focused.window.clone();
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("target"));
    assert!(workspace.has_window(&focused_window));
    let mut events = Vec::new();
    for _ in 0..2 {
        let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
        assert_eq!(event_type, (1 << 31) | 3);
        events.push(serde_json::from_str::<Value>(&payload).unwrap());
    }
    assert_eq!(events[0]["change"], "move");
    assert_eq!(
        events[0]["container"]["id"],
        crate::ipc::tree::window_id(scratchpad_id)
    );
    assert_eq!(events[0]["container"]["type"], "floating_con");
    assert_eq!(events[1]["change"], "focus");
}

#[test]
fn moving_fullscreen_window_to_scratchpad_clears_its_fullscreen_state() {
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

    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    let shown = f.swayward().layout.focus().unwrap().window.clone();
    assert_eq!(f.swayward().layout.fullscreen_mode(&shown), None);

    assert!(crate::command::execute(f.niri_state(), "floating toggle")[0].success);
    assert!(!f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_floating(&shown));
}

#[test]
fn workspace_fullscreen_descendant_does_not_move_to_an_adjacent_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("right".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "split v")[0].success);
    let second = f.client(client).create_window();
    second.commit();
    let second_surface = second.surface.clone();
    f.roundtrip(client);
    let second = f.client(client).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(client);
    for command in ["focus parent", "fullscreen enable", "focus child"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    let id = f.swayward().layout.focus().unwrap().id();
    assert!(crate::command::execute(f.niri_state(), "move right")[0].success);

    let swayward = f.swayward();
    let (_, mapped) = swayward
        .layout
        .windows()
        .find(|(_, mapped)| mapped.id() == id)
        .unwrap();
    let output = swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&mapped.window))
        .and_then(|(monitor, _, _)| monitor)
        .unwrap()
        .output_name();
    assert_eq!(output, "left");
}

#[test]
fn targeted_fullscreen_toggle_replaces_another_windows_fullscreen() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    let first = f.swayward().layout.focus().unwrap().window.clone();
    map_test_window(&mut f, client, "second");
    let second = f.swayward().layout.focus().unwrap().window.clone();

    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="first"] fullscreen enable"#)[0].success
    );
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="second"] fullscreen toggle"#)[0]
            .success
    );

    assert_eq!(f.swayward().layout.fullscreen_mode(&first), None);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&second),
        Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
    );
}

#[test]
fn targeted_global_fullscreen_selects_and_focuses_the_windows_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.set_title("target");
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let target = f.swayward().layout.focus().unwrap().window.clone();
    let target_workspace = f.swayward().layout.active_workspace().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "workspace other")[0].success);
    assert_ne!(
        f.swayward().layout.active_workspace().unwrap().id(),
        target_workspace
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[title="target"] fullscreen enable global"#
        )[0]
        .success
    );

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().id(),
        target_workspace
    );
    assert_eq!(f.swayward().layout.focus().unwrap().window, target);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&target),
        Some(crate::layout::tiling_tree::FullscreenMode::Global)
    );
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["fullscreen_mode"],
        2
    );
}

#[test]
fn scratchpad_show_disables_target_workspace_and_global_fullscreen() {
    for fullscreen in ["fullscreen enable", "fullscreen enable global"] {
        let mut f = Fixture::new();
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        let mut ids = Vec::new();

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

        assert!(crate::command::execute(f.niri_state(), fullscreen)[0].success);
        assert!(f.swayward().layout.focused_fullscreen_mode().is_some());
        let first = crate::ipc::tree::window_id(ids[0]);
        assert!(
            crate::command::execute(f.niri_state(), &format!("[con_id={first}] move scratchpad"))
                [0]
            .success
        );
        assert!(
            crate::command::execute(f.niri_state(), &format!("[con_id={first}] scratchpad show"))
                [0]
            .success
        );

        assert_eq!(
            f.swayward().layout.focused_fullscreen_mode(),
            None,
            "{fullscreen}"
        );
        assert!(
            !f.swayward().layout.global_fullscreen_active(),
            "{fullscreen}"
        );
    }
}

#[test]
fn scratchpad_show_toggles_the_only_window() {
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

    for command in ["move scratchpad", "scratchpad show"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }
    assert!(f.swayward().layout.focus().is_some());
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(f.swayward().layout.focus().is_none());
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
}

#[test]
fn empty_scratch_workspace_is_always_serialized() {
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
    for command in ["move scratchpad", "scratchpad show"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert_eq!(tree.nodes[0].nodes[0].name.as_deref(), Some("__i3_scratch"));
    assert!(tree.nodes[0].nodes[0].floating_nodes.is_empty());
}

#[test]
fn get_workspaces_distinguishes_seat_focus_from_output_visibility() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(2);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.focused)
            .count(),
        1
    );
    assert_eq!(
        workspaces
            .iter()
            .filter(|workspace| workspace.visible)
            .count(),
        2
    );
    assert!(
        workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .unwrap()
            .visible
    );
}

#[test]
fn workspace_commands_create_sparse_global_identities() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let client = f.add_client();
    for command in ["workspace 1", "workspace 3", "workspace 7"] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success);
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
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.num, workspace.name.as_str(), workspace.focused))
            .collect::<Vec<_>>(),
        [(1, "1", false), (3, "3", false), (7, "7", true)]
    );
}

#[test]
fn focus_next_and_prev_follow_the_immediate_parent_layout() {
    for layout in ["splith", "splitv", "tabbed", "stacking"] {
        let mut f = Fixture::new();
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

        assert!(crate::command::execute(f.niri_state(), &format!("layout {layout}"))[0].success);
        assert!(crate::command::execute(f.niri_state(), r#"[app_id="first"] focus"#)[0].success);
        let first = f.swayward().layout.focus().unwrap().id();

        let outcome = crate::command::execute(f.niri_state(), "focus next");
        assert!(outcome[0].success, "{layout}: {outcome:?}");
        assert_ne!(f.swayward().layout.focus().unwrap().id(), first, "{layout}");

        let outcome = crate::command::execute(f.niri_state(), "focus prev");
        assert!(outcome[0].success, "{layout}: {outcome:?}");
        assert_eq!(f.swayward().layout.focus().unwrap().id(), first, "{layout}");
    }

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert!(f.swayward().layout.focus().is_none());
    let outcome = crate::command::execute(f.niri_state(), "focus next");
    assert!(outcome[0].success, "{outcome:?}");
    assert!(f.swayward().layout.focus().is_none());
}

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
    assert_eq!(node["current_border_width"], 0);
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

    for (command, style, width, has_titlebar, rendered_width) in [
        ("border none", "none", 0, false, None),
        ("border pixel 3", "pixel", 3, false, Some(3.)),
        ("border normal 5", "normal", 5, true, Some(5.)),
        ("border toggle", "none", 0, false, None),
        ("border toggle", "pixel", 1, false, Some(1.)),
        ("border toggle", "normal", 2, true, Some(2.)),
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
                width
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
        assert_eq!(node.current_border_width, i32::from(width));
    }

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    for (command, style, width) in [("border none", "none", 0), ("border pixel 7", "pixel", 7)] {
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

#[test]
fn criteria_targeted_move_workspace_moves_all_matches_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["special", "special", "ordinary"] {
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

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="ordinary"] focus"#)[0].success);
    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="special"] move workspace target"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let workspaces = f.swayward().layout.workspaces().collect::<Vec<_>>();
    let source = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() != Some("target"))
        .unwrap()
        .2;
    let target = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
        .unwrap()
        .2;
    assert_eq!(source.windows().count(), 1);
    assert_eq!(
        source.active_window().and_then(|window| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
        }),
        Some("ordinary".into())
    );
    assert_eq!(target.windows().count(), 2);
}

#[test]
fn criteria_move_workspace_to_output_uses_the_matched_workspace() {
    let mut f = Fixture::new();
    f.add_named_output_at("west".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("east".into(), (100, 100), Some((200, 0)));
    let client = f.add_client();

    assert!(
        crate::command::execute(f.niri_state(), "focus output middle, workspace target")
            .iter()
            .all(|outcome| outcome.success)
    );
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("moveme".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus output west")[0].success);

    let workspace_output = |f: &mut Fixture| {
        f.swayward()
            .layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
            .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output_name().clone()))
            .unwrap()
    };

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id="moveme"] move workspace to output right"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(workspace_output(&mut f), "east");

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[workspace="target"] move workspace to middle"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(workspace_output(&mut f), "middle");

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[workspace="target"] move workspace to output missing"#,
    );
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Can't find output with name/direction 'missing'")
    );
    assert_eq!(workspace_output(&mut f), "middle");
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "west");

    let mut f = Fixture::new();
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("east".into(), (100, 100), Some((200, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let floating = f.swayward().layout.focus().unwrap().window.clone();
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    f.swayward().layout.move_floating_window(
        Some(&floating),
        swayward_ipc::legacy::PositionChange::AdjustFixed(20.),
        swayward_ipc::legacy::PositionChange::AdjustFixed(10.),
        false,
    );
    let old_center = f.swayward().layout.window_center(&floating).unwrap();

    assert!(crate::command::execute(f.niri_state(), "move workspace to output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "east");
    let new_center = f.swayward().layout.window_center(&floating).unwrap();
    assert_eq!(new_center.x - old_center.x, 100);
    assert_eq!(new_center.y, old_center.y);
    let wrapped = crate::command::execute(f.niri_state(), "move workspace to output right");
    assert!(wrapped[0].success, "{wrapped:?}");
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "middle"
    );
}

#[test]
fn criteria_move_workspace_ignores_hidden_scratchpad_matches() {
    let mut f = Fixture::new();
    f.add_named_output_at("fake-0".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("fake-1".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();

    for (output, workspace, app_id, scratchpad) in [
        ("fake-0", "ws0", "a", false),
        ("fake-1", "ws1", "b", false),
        ("fake-1", "ws1", "c", true),
    ] {
        let command = format!("focus output {output}, workspace {workspace}");
        assert!(crate::command::execute(f.niri_state(), &command)
            .iter()
            .all(|outcome| outcome.success));
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if scratchpad {
            assert!(crate::command::execute(f.niri_state(), "move to scratchpad")[0].success);
        }
    }

    let outcome = crate::command::execute(
        f.niri_state(),
        r#"[app_id=".*"] move workspace to output fake-1"#,
    );
    assert!(outcome[0].success, "{outcome:?}");
    let swayward = f.swayward();
    assert!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .any(|workspace| workspace.name == "ws0" && workspace.output == "fake-1")
    );
}

#[test]
fn cross_workspace_swap_exchanges_positions_marks_and_fullscreen() {
    let mut f = Fixture::new();
    f.add_output_at(1, (600, 800), Some((0, 0)));
    f.add_output_at(2, (1000, 800), Some((600, 0)));
    let client = f.add_client();

    let mut windows = Vec::new();
    for (output, workspace, mark, fullscreen) in [
        ("headless-1", "one", "A", true),
        ("headless-2", "two", "B", false),
    ] {
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {output}, workspace {workspace}")
        )
        .iter()
        .all(|outcome| outcome.success));
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mapped = f.swayward().layout.focus().unwrap();
        windows.push((workspace, mapped.id(), mapped.window.clone()));
        assert!(crate::command::execute(f.niri_state(), &format!("mark {mark}"))[0].success);
        if fullscreen {
            assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
        }
    }

    let result = crate::command::execute(f.niri_state(), "[con_mark=B] swap container with mark A");
    assert!(result[0].success, "{result:?}");
    let (one_id, two_id) = {
        let layout = &f.swayward().layout;
        let one = layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("one"))
            .unwrap()
            .2
            .id();
        let two = layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("two"))
            .unwrap()
            .2
            .id();
        (one, two)
    };
    assert_eq!(
        f.swayward().layout.window_workspace_id(&windows[0].2),
        Some(two_id)
    );
    assert_eq!(
        f.swayward().layout.window_workspace_id(&windows[1].2),
        Some(one_id)
    );
    for workspace in [one_id, two_id] {
        let tree = f
            .swayward()
            .layout
            .workspaces()
            .find(|(_, _, candidate)| candidate.id() == workspace)
            .unwrap()
            .2
            .ipc_tiling_tree();
        assert_eq!(tree.nodes().len(), 2);
    }
    let first_center = f.swayward().layout.window_center(&windows[0].2).unwrap();
    let second_center = f.swayward().layout.window_center(&windows[1].2).unwrap();
    assert!(first_center.x >= 600, "{first_center:?}");
    assert!(second_center.x < 600, "{second_center:?}");
    assert_eq!(f.swayward().layout.fullscreen_mode(&windows[0].2), None);
    assert_eq!(
        f.swayward().layout.fullscreen_mode(&windows[1].2),
        Some(crate::layout::tiling_tree::FullscreenMode::Workspace)
    );
    assert!(f
        .swayward()
        .marks_by_window
        .get(&windows[0].1)
        .is_some_and(|marks| marks.as_slice() == ["A"]));
    assert!(f
        .swayward()
        .marks_by_window
        .get(&windows[1].1)
        .is_some_and(|marks| marks.as_slice() == ["B"]));
    assert_eq!(
        f.swayward()
            .marks_by_window
            .values()
            .flatten()
            .filter(|mark| *mark == "A" || *mark == "B")
            .count(),
        2
    );
}

#[test]
fn criteria_targeted_move_workspace_preserves_a_container_subtree() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace source")[0].success);
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
    assert!(crate::command::execute(f.niri_state(), r#"[app_id="first"] focus"#)[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "mark group")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace target")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "[con_mark=group] move workspace target");
    assert!(outcome[0].success, "{outcome:?}");
    let workspaces = f.swayward().layout.workspaces().collect::<Vec<_>>();
    let source = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("source"))
        .unwrap()
        .2;
    let target = workspaces
        .iter()
        .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("target"))
        .unwrap()
        .2;
    assert_eq!(source.windows().count(), 0);
    assert_eq!(target.windows().count(), 3);
    assert_eq!(target.ipc_tiling_tree().nodes().len(), 5);
    assert!(target.windows().any(|window| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
            == Some("first".into())
    }));
}

#[test]
fn criteria_targeted_scratchpad_show_toggles_every_matching_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..2 {
        let window = f.client(client).create_window();
        window.set_title("toggle-window");
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    for expected_hidden in [0, 2, 0] {
        let outcome =
            crate::command::execute(f.niri_state(), r#"[title="toggle-"] scratchpad show"#);
        assert!(outcome[0].success, "{outcome:?}");
        assert_eq!(
            f.swayward().layout.scratchpad_windows().count(),
            expected_hidden
        );
    }
    for expected_hidden in [1, 2] {
        assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
        assert_eq!(
            f.swayward().layout.scratchpad_windows().count(),
            expected_hidden
        );
    }
}

#[test]
fn criteria_targeted_scratchpad_show_toggles_each_match_from_its_own_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let mut windows = Vec::new();
    for title in ["mixed-toggle-1", "mixed-toggle-2"] {
        let window = f.client(client).create_window();
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        let mapped = f
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .active_window()
            .unwrap();
        windows.push((mapped.window.clone(), mapped.id()));
        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    }

    let [(first, first_id), (second, _)] = windows.as_slice() else {
        unreachable!()
    };
    let first_id = crate::ipc::tree::window_id(*first_id);
    assert!(
        crate::command::execute(
            f.niri_state(),
            &format!("[con_id={first_id}] scratchpad show")
        )[0]
        .success
    );
    assert!(!f.swayward().layout.is_scratchpad_hidden(first));
    assert!(f.swayward().layout.is_scratchpad_hidden(second));

    let outcome =
        crate::command::execute(f.niri_state(), r#"[title="mixed-toggle-"] scratchpad show"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert!(f.swayward().layout.is_scratchpad_hidden(first));
    assert!(!f.swayward().layout.is_scratchpad_hidden(second));
}

#[test]
fn criteria_targeted_scratchpad_commands_move_only_the_matching_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["ordinary", "special"] {
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
        crate::command::execute(f.niri_state(), r#"[app_id="special"] move scratchpad"#)[0].success
    );
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    let ordinary = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .active_window()
        .unwrap();
    assert_eq!(
        crate::utils::with_toplevel_role(ordinary.toplevel(), |role| role.app_id.clone()),
        Some("ordinary".into())
    );
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="special"] scratchpad show"#)[0].success
    );
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.windows().count(), 2);
    let active = workspace.active_window().unwrap();
    assert_eq!(
        crate::utils::with_toplevel_role(active.toplevel(), |role| role.app_id.clone()),
        Some("special".into())
    );
}

fn dialog_rect_after_parent_move(animations_off: bool) -> Value {
    let mut config = swayward_config::Config::default();
    config.animations.off = animations_off;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let first = f.client(client).create_window();
    first.commit();
    let first_surface = first.surface.clone();
    f.roundtrip(client);
    let first = f.client(client).window(&first_surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    f.double_roundtrip(client);

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
    assert!(crate::command::execute(f.niri_state(), "move left")[0].success);
    if !animations_off {
        assert!(f.swayward().layout.are_animations_ongoing(None));
    }

    let child = f.client(client).create_window();
    child.xdg_toplevel.set_app_id("dialog".into());
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn find_app(value: &Value) -> Option<&Value> {
        if value["app_id"] == "dialog" {
            return Some(value);
        }
        ["nodes", "floating_nodes"]
            .into_iter()
            .find_map(|key| value[key].as_array()?.iter().find_map(find_app))
    }
    find_app(&tree).unwrap()["rect"].clone()
}

fn fullscreen_parent_after_child_map(
    policy: swayward_config::PopupDuringFullscreen,
) -> (bool, bool) {
    let mut config = swayward_config::Config {
        popup_during_fullscreen: policy,
        ..Default::default()
    };
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    let parent = f.client(client).create_window();
    let parent_surface = parent.surface.clone();
    let parent_toplevel = parent.xdg_toplevel.clone();
    parent.commit();
    f.roundtrip(client);
    let parent = f.client(client).window(&parent_surface);
    parent.attach_new_buffer();
    parent.ack_last_and_commit();
    f.double_roundtrip(client);
    let parent_id = f.swayward().layout.focus().unwrap().id();
    let parent_window = f.swayward().layout.focus().unwrap().window.clone();
    f.swayward().layout.set_fullscreen(&parent_window, true);

    let child = f.client(client).create_window();
    child.set_parent(Some(&parent_toplevel));
    let child_surface = child.surface.clone();
    child.commit();
    f.roundtrip(client);
    let child = f.client(client).window(&child_surface);
    child.attach_new_buffer();
    child.ack_last_and_commit();
    f.double_roundtrip(client);

    (
        f.swayward()
            .layout
            .fullscreen_mode(&parent_window)
            .is_some(),
        f.swayward().layout.focus().unwrap().id() != parent_id,
    )
}

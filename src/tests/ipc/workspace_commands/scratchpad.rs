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
fn fullscreen_with_a_focused_floating_window_does_not_target_the_tiling_parent() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "tiled");
    map_test_window(&mut f, client, "floating");
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus floating")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(find_json_node_with_app_id(&tree, "floating").unwrap()["fullscreen_mode"], 1);
    assert_eq!(find_json_parent_of_app_id(&tree, "tiled").unwrap()["fullscreen_mode"], 0);

    assert!(crate::command::execute(f.niri_state(), "fullscreen disable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus tiling")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "fullscreen enable")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(find_json_parent_of_app_id(&tree, "tiled").unwrap()["focused"], true);
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
            .rev()
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
fn sticky_state_survives_moving_a_window_back_to_scratchpad() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    map_test_window(&mut f, client, "sticky-scratchpad");
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    assert!(crate::command::execute(f.niri_state(), "sticky enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    assert!(tree.nodes[0].nodes[0].floating_nodes[0].sticky);
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
    let expected: Value = serde_json::from_str(&sway_fixture!("events/window.move.json")).unwrap();
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

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    let mut command = UnixStream::connect(&socket).unwrap();
    let reply = query_ipc_with_payload(
        &mut f,
        &mut command,
        MessageType::RunCommand,
        "scratchpad show",
    );
    assert_eq!(reply[0]["success"], true);
    let focused = f.swayward().layout.focus().unwrap();
    assert_eq!(focused.id(), scratchpad_id);
    let focused_window = focused.window.clone();
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("target"));
    assert!(workspace.has_window(&focused_window));
    let mut events = Vec::new();
    let mut remainder = Vec::new();
    for _ in 0..2 {
        let ((event_type, payload), next) =
            read_ipc_reply_with_remainder(&mut f, &mut subscriber, remainder);
        remainder = next;
        assert_eq!(event_type, (1 << 31) | 3);
        events.push(serde_json::from_str::<Value>(&payload).unwrap());
    }
    assert_eq!(events[0]["change"], "focus");
    assert_eq!(
        events[0]["container"]["id"],
        crate::ipc::tree::window_id(scratchpad_id)
    );
    assert_eq!(events[0]["container"]["type"], "floating_con");
    assert_eq!(events[1]["change"], "move");
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


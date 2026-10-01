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
fn directional_move_to_output_inserts_before_destination_focus() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (100, 100), Some((0, 0)));
    f.add_named_output_at("right".into(), (100, 100), Some((100, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
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

    assert!(crate::command::execute(f.niri_state(), "move right")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output left, move right")[0].success);

    let apps = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles()
        .filter_map(|tile| {
            crate::utils::with_toplevel_role(tile.window().toplevel(), |role| role.app_id.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(apps, ["first", "second"]);
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

// random seed 274 step 10 (sway-1.12-random): moving a window out of a
// fullscreen split container moves only the window. The container stays
// behind and is reaped, so the moved window does not arrive fullscreen
// (`container_move_to_workspace`, sway/commands/move.c:220-229).
#[test]
fn moving_a_window_out_of_a_fullscreen_container_leaves_fullscreen_behind() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for app_id in ["moved", "other"] {
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
    for command in [
        "splitv",
        "splith",
        "focus left",
        "fullscreen toggle",
        "splith",
        "move left",
        "splith",
        "move container to workspace 2",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }

    let swayward = f.swayward();
    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "moved").unwrap()["fullscreen_mode"],
        0
    );
}

#[test]
fn live_ipc_move_to_an_empty_workspace_preserves_the_container_layout() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1270, 1408));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
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
    f.swayward().layout.nest_or_unnest_window_left(None);
    let mut stream = UnixStream::connect(socket).unwrap();
    for command in [
        "focus parent",
        "mark group",
        "workspace target",
        "[con_mark=group] move workspace target",
    ] {
        let outcome = query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, command);
        assert_eq!(outcome[0]["success"], true, "{command}: {outcome}");
    }
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let workspaces = tree["nodes"][1]["nodes"].as_array().unwrap();
    let target = workspaces
        .iter()
        .find(|workspace| workspace["name"] == "target")
        .unwrap();
    assert_eq!(target["layout"], "splitv");
    assert_eq!(target["representation"], "V[second first]");
    assert_eq!(target["nodes"].as_array().unwrap().len(), 2);
    let app_ids = target["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(app_ids, ["first", "second"].into());
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
    f.swayward().layout.nest_or_unnest_window_left(None);
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

// sway/tree/container.c:977-980 removes the container from the scratchpad
// before returning it to tiling. root_scratchpad_remove_container emits
// `move` (sway/tree/root.c:150-154) and container_set_floating then emits
// `floating` (sway/tree/container.c:1015).
#[test]
fn unfloating_a_scratchpad_window_emits_move_then_floating() {
    let (mut f, socket) = ipc_fixture();
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
    let id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    f.niri_state().refresh_and_flush_clients();

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut f, &mut subscriber);

    assert!(crate::command::execute(f.niri_state(), "floating disable")[0].success);
    let mut events = Vec::new();
    let mut remainder = Vec::new();
    for _ in 0..2 {
        let ((event_type, payload), rest) =
            read_ipc_reply_with_remainder(&mut f, &mut subscriber, remainder);
        remainder = rest;
        assert_eq!(event_type, EVENT_WINDOW);
        events.push(serde_json::from_str::<Value>(&payload).unwrap());
    }
    for event in &events {
        assert_eq!(event["container"]["id"], crate::ipc::tree::window_id(id));
        assert_eq!(event["container"]["type"], "con");
        assert_eq!(event["container"]["scratchpad_state"], "none");
    }
    assert_eq!(events[0]["change"], "move");
    assert_eq!(events[1]["change"], "floating");
}

#[test]
fn floating_toggle_after_moving_scratchpad_window_between_workspaces_does_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for _ in 0..5 {
        windows::map_window(&mut f, client, windows::WindowSpec::default());
    }

    for (command, expected_success) in [
        ("focus parent", true),
        ("move scratchpad", true),
        ("scratchpad show", true),
        ("move container to workspace 2", true),
        ("floating toggle", false),
        ("move container to workspace 2", false),
        ("workspace 2", true),
        ("floating toggle", true),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            outcome[0].success, expected_success,
            "{command}: {outcome:?}"
        );
        f.swayward().layout.verify_invariants();
    }
}

#[test]
fn moving_a_window_away_refocuses_the_most_recent_container_under_its_parent() {
    // Oracle random seeds 217 step 8 and 124 step 8: sway refocuses
    // `seat_get_focus_inactive(old_parent)` after a move (sway/commands/move.c:598-608),
    // and its focus stack holds a split whenever a descendant was focused
    // (sway/input/seat.c:1178-1190), so a vacated split can be refocused as a container.
    // Seed 474 step 15: when the move reaps a container, sway raises the most recent view
    // under its parent instead (sway/input/seat.c:273-323).
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    let map = |f: &mut Fixture, app_id: &str| {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    };
    let focused = |f: &mut Fixture| {
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        let node = find_json_node(&tree, "con", true).unwrap();
        (node["layout"].clone(), node["app_id"].clone())
    };

    map(&mut f, "first");
    map(&mut f, "second");
    for command in ["layout splitv", "focus up", "move right"] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    assert!(crate::command::execute(f.niri_state(), "move container to workspace 2")[0].success);
    assert_eq!(
        focused(&mut f),
        (serde_json::json!("splitv"), serde_json::Value::Null),
        "the vacated split was focused through its window, so sway refocuses the split"
    );
}

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
            parse_error: Some(false),
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
fn layout_default_without_previous_split_is_a_parse_error() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let outcome = crate::command::execute(f.niri_state(), "layout default");
    assert!(!outcome[0].success);
    assert_eq!(outcome[0].parse_error, Some(true));
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
    let (mut f, _) = ipc_fixture();
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

    let tree = get_tree(&mut f);
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

/// Criteria walk resident floating-group leaves through the group's IPC
/// snapshot. Run app_id criteria at every step of a group's lifecycle: while
/// it floats, while it is hidden in the scratchpad, after it is shown, and as
/// its leaves unmap one by one. None of these may panic the compositor.
#[test]
fn criteria_over_floating_group_leaves_survive_group_lifecycle() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let mut surfaces = Vec::new();
    for _ in 0..3 {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id("grouped".into());
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
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    let matched = |f: &mut Fixture| {
        let outcome = crate::command::execute(f.niri_state(), "[app_id=^grouped$] nop");
        let remaining = f.swayward().layout.windows().count();
        assert_eq!(
            outcome[0].success,
            remaining > 0,
            "{remaining}: {outcome:?}"
        );
    };
    matched(&mut f);
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    matched(&mut f);
    assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
    matched(&mut f);
    for surface in surfaces {
        let window = f.client(client).window(&surface);
        window.attach_null();
        window.commit();
        f.double_roundtrip(client);
        matched(&mut f);
    }
}

/// Oracle: state/criteria_order_split_before_child and
/// state/criteria_order_scratchpad_last. Plain `mark n` moves the mark to
/// each match in turn, so the target left holding it is the last in match
/// order. Sway walks parents before children, outputs before the hidden
/// scratchpad (`sway/sway/criteria.c:500-512`,
/// `sway/sway/tree/root.c:246-261`).
#[test]
fn criteria_commands_apply_to_matches_in_sways_walk_order() {
    fn marks_of<'a>(tree: &'a Value, app_id: Option<&str>) -> Vec<&'a Value> {
        let mut found = Vec::new();
        fn walk<'a>(node: &'a Value, app_id: Option<&str>, found: &mut Vec<&'a Value>) {
            let is_split = node["type"] == "con" && node["app_id"].is_null();
            if app_id.map_or(is_split, |app_id| node["app_id"] == app_id) {
                found.push(&node["marks"]);
            }
            for key in ["nodes", "floating_nodes"] {
                for child in node[key].as_array().into_iter().flatten() {
                    walk(child, app_id, found);
                }
            }
        }
        walk(tree, app_id, &mut found);
        found
    }
    let ipc_tree = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap()
    };
    let run = |f: &mut Fixture, command: &str| {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome.iter().all(|o| o.success), "{command}: {outcome:?}");
    };

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "fixture-1");
    run(&mut f, "splitv");
    map_test_window(&mut f, client, "fixture-2");
    run(&mut f, "splith");
    map_test_window(&mut f, client, "fixture-3");
    run(&mut f, "mark m-child");
    run(&mut f, "focus parent");
    run(&mut f, "mark m-parent");
    run(&mut f, "[con_mark=\"^m-\"] mark n");
    let tree = ipc_tree(&mut f);
    assert_eq!(marks_of(&tree, Some("fixture-3")), [&serde_json::json!(["n"])]);
    assert_eq!(marks_of(&tree, None), [&serde_json::json!([])]);

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "fixture-1");
    run(&mut f, "mark m-a");
    run(&mut f, "move scratchpad");
    map_test_window(&mut f, client, "fixture-2");
    run(&mut f, "mark m-b");
    run(&mut f, "[con_mark=\"^m-\"] mark n");
    let tree = ipc_tree(&mut f);
    assert_eq!(marks_of(&tree, Some("fixture-1")), [&serde_json::json!(["n"])]);
    assert_eq!(marks_of(&tree, Some("fixture-2")), [&serde_json::json!([])]);
}

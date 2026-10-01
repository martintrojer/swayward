#[test]
fn bare_directional_move_crosses_each_adjacent_output_without_wrapping() {
    let mut f = Fixture::new();
    for (name, position) in [
        ("top-left", (0, 0)),
        ("top-right", (800, 0)),
        ("bottom-right", (800, 600)),
        ("bottom-left", (0, 600)),
    ] {
        f.add_named_output_at(name.into(), (800, 600), Some(position));
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {name}, workspace {name}-workspace")
        )
        .iter()
        .all(|outcome| outcome.success));
    }

    assert!(crate::command::execute(f.niri_state(), "workspace top-left-workspace")[0].success);
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let window_id = f.swayward().layout.focus().unwrap().id();

    for (command, expected_output) in [
        ("move right", "top-right"),
        ("move down", "bottom-right"),
        ("move left", "bottom-left"),
        ("move up", "top-left"),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.id() == window_id)
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected_output,
            "{command}"
        );
    }
}

#[test]
fn criteria_directional_move_crosses_outputs_without_changing_focus() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    let moved = f.client(client).create_window();
    moved.xdg_toplevel.set_app_id("moved".into());
    moved.commit();
    let surface = moved.surface.clone();
    f.roundtrip(client);
    let moved = f.client(client).window(&surface);
    moved.attach_new_buffer();
    moved.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let focused = f.client(client).create_window();
    focused.commit();
    let surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    let outcome = crate::command::execute(f.niri_state(), r#"[app_id="moved"] move right"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert!(f
        .swayward()
        .layout
        .windows()
        .find(
            |(_, mapped)| crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
                role.app_id.as_deref() == Some("moved")
            })
        )
        .unwrap()
        .0
        .is_some_and(|monitor| monitor.output_name() == "right"));
}

#[test]
fn criteria_move_output_right_uses_layout_positions_during_workspace_animation() {
    let mut f = Fixture::new();
    for (name, position) in [
        ("top-left", (0, 0)),
        ("top-right", (800, 0)),
        ("bottom-left", (0, 600)),
        ("bottom-right", (800, 600)),
    ] {
        f.add_named_output_at(name.into(), (800, 600), Some(position));
    }
    for (output, workspace) in [
        ("top-left", "top-left-workspace"),
        ("top-right", "top-right-workspace"),
        ("bottom-left", "bottom-left-workspace"),
        ("bottom-right", "bottom-right-workspace"),
    ] {
        assert!(crate::command::execute(
            f.niri_state(),
            &format!("focus output {output}, workspace {workspace}")
        )
        .iter()
        .all(|outcome| outcome.success));
    }

    let client = f.add_client();
    for workspace in ["top-left-workspace", "bottom-left-workspace"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
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
    }

    assert!(f.swayward().layout.are_animations_ongoing(None));
    let bottom = f
        .swayward()
        .layout
        .windows()
        .find(|(monitor, mapped)| {
            monitor.is_some_and(|monitor| monitor.output_name() == "bottom-left")
                && crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
                    role.app_id.as_deref() == Some("moveme")
                })
        })
        .map(|(_, mapped)| mapped.window.clone())
        .unwrap();
    assert!(f.swayward().layout.window_center(&bottom).unwrap().y >= 600);
    assert!(
        crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output right"#)[0]
            .success
    );
    let workspace_counts = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace
                .name()
                .map(|name| (name.to_owned(), workspace.windows().count()))
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(workspace_counts["top-right-workspace"], 1);
    assert_eq!(workspace_counts["bottom-right-workspace"], 1);
}

#[test]
fn move_output_direction_uses_the_windows_output_and_stops_at_the_edge() {
    let mut f = Fixture::new();
    f.add_named_output_at("right".into(), (100, 100), Some((200, 100)));
    f.add_named_output_at("middle".into(), (100, 100), Some((100, 0)));
    f.add_named_output_at("left".into(), (100, 100), Some((0, 100)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("moveme".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let window_id = f.swayward().layout.focus().unwrap().id();

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    for expected in ["middle", "left"] {
        assert!(
            crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output left"#)[0]
                .success
        );
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .find(|(_, mapped)| mapped.id() == window_id)
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected
        );
    }

    let outcome =
        &crate::command::execute(f.niri_state(), r#"[app_id="moveme"] move output left"#)[0];
    assert!(outcome.success, "{outcome:?}");
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == window_id)
            .unwrap()
            .0
            .unwrap()
            .output_name(),
        "right"
    );
}

#[test]
fn move_split_container_to_output_preserves_the_subtree() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
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
        if app_id == "second" {
            assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move container to output right");
    assert!(outcome[0].success, "{outcome:?}");
    let counts = f.swayward().layout.windows().fold(
        std::collections::HashMap::<_, usize>::new(),
        |mut counts, (monitor, _)| {
            *counts
                .entry(monitor.unwrap().output_name().clone())
                .or_default() += 1;
            counts
        },
    );
    assert_eq!(counts.get("right"), Some(&2));
    assert_eq!(counts.get("left"), Some(&1));
}

#[test]
fn moving_a_workspace_fails_like_sway() {
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

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert_eq!(
        crate::command::execute(f.niri_state(), "move right"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Cannot move workspaces in a direction".into()),
            parse_error: Some(false),
        }]
    );
}

#[test]
fn move_split_container_direction_crosses_output_as_a_subtree() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
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
        if app_id == "second" {
            assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
        }
    }
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);

    let outcome = crate::command::execute(f.niri_state(), "move right");
    assert!(outcome[0].success, "{outcome:?}");
    let counts = f.swayward().layout.windows().fold(
        std::collections::HashMap::<_, usize>::new(),
        |mut counts, (monitor, _)| {
            *counts
                .entry(monitor.unwrap().output_name().clone())
                .or_default() += 1;
            counts
        },
    );
    assert_eq!(counts.get("right"), Some(&2));
    assert_eq!(counts.get("left"), Some(&1));
}

#[test]
fn unscoped_move_output_wraps_from_the_edge() {
    let mut f = Fixture::new();
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
    assert!(
        crate::command::execute(f.niri_state(), "focus output right, workspace right-ws")[0]
            .success
    );
    assert!(
        crate::command::execute(f.niri_state(), "focus output left, workspace left-ws")[0].success
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

    for expected in ["right", "left"] {
        assert!(
            crate::command::execute(f.niri_state(), "move container to output right")[0].success
        );
        assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
        assert_eq!(
            f.swayward()
                .layout
                .windows()
                .next()
                .unwrap()
                .0
                .unwrap()
                .output_name(),
            expected
        );
    }
}

#[test]
fn live_ipc_move_from_an_output_without_geometry_returns_a_failure() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    let reference = f.swayward().layout.active_output().unwrap().clone();
    f.swayward().global_space.unmap_output(&reference);
    let mut stream = UnixStream::connect(socket).unwrap();

    let outcome = query_ipc_with_payload(
        &mut f,
        &mut stream,
        MessageType::RunCommand,
        "move workspace output right",
    );

    assert_eq!(outcome.as_array().unwrap().len(), 1);
    assert_eq!(outcome[0]["success"], false);
    assert_eq!(outcome[0]["error"], "Reference output has no geometry");
    assert!(query_ipc(&mut f, &mut stream, MessageType::GetTree).is_object());
}

#[test]
fn move_output_accepts_direction_name_current_and_workspace_forms() {
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

    assert!(crate::command::execute(f.niri_state(), "move output current")[0].success);
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

#[test]
fn sticky_accepts_sway_boolean_words_and_reports_tree_state() {
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

    assert!(crate::command::execute(f.niri_state(), "sticky enable")[0].success);
    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    assert_eq!(find_json_node(&tree, "con", false).unwrap()["sticky"], true);

    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);

    for (value, expected) in [
        ("enable", true),
        ("toggle", false),
        ("enabled", true),
        ("off", false),
        ("yes", true),
        ("0", false),
        ("1", true),
        ("no", false),
        ("on", true),
        ("disable", false),
        ("active", true),
        ("unknown", false),
    ] {
        assert!(crate::command::execute(f.niri_state(), &format!("sticky {value}"))[0].success);
        let swayward = f.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap();
        assert_eq!(
            find_json_node(&tree, "floating_con", false).unwrap()["sticky"],
            expected
        );
    }
}

#[test]
fn sticky_on_a_focused_split_marks_the_container_like_sway() {
    // Oracle random seeds 57, 60 and 69 (sticky on a split) and 143, 317 and 413 (sticky with
    // the workspace focused): sway stores `is_sticky` on the focused container
    // (sway/commands/sticky.c:21-26), and a focused workspace has no container.
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

    let tree = |f: &mut Fixture| {
        let swayward = f.swayward();
        serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        ))
        .unwrap()
    };
    for command in [
        "layout splitv",
        "layout splith",
        "focus parent",
        "sticky toggle",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    let tree = tree(&mut f);
    let split = find_json_node(&tree, "con", true).unwrap();
    assert_eq!(split["layout"], "splith");
    assert_eq!(split["sticky"], true);
    assert_eq!(split["nodes"][0]["sticky"], false);

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert_eq!(
        crate::command::execute(f.niri_state(), "sticky toggle")[0]
            .error
            .as_deref(),
        Some("No current container")
    );
}

#[test]
fn sticky_without_a_container_matches_sway_failure() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    assert_eq!(
        crate::command::execute(f.niri_state(), "sticky enable")[0]
            .error
            .as_deref(),
        Some("No current container")
    );
}

#[test]
fn workspace_criteria_uses_sparse_and_named_sway_identities() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for workspace in ["1", "7", "mail"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(workspace.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    for (workspace, mark) in [("7", "sparse"), ("mail", "named")] {
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!(r#"[workspace="^{workspace}$"] mark {mark}"#)
            )[0]
            .success
        );
    }

    let swayward = f.swayward();
    let marked_apps = swayward
        .layout
        .windows()
        .filter_map(|(_, window)| {
            swayward.marks_by_window.get(&window.id()).map(|marks| {
                let app_id = crate::utils::with_toplevel_role(window.toplevel(), |role| {
                    role.app_id.clone().unwrap()
                });
                (app_id, marks.clone())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        marked_apps,
        [
            ("7".into(), vec!["sparse".into()]),
            ("mail".into(), vec!["named".into()])
        ]
    );
}

#[test]
fn workspace_next_and_prev_on_output_wrap_in_stored_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let client = f.add_client();
    for workspace in ["1", "5", "6:a", "6:b"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
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

    assert!(crate::command::execute(f.niri_state(), "workspace next_on_output")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("1".into())
    );
    assert!(crate::command::execute(f.niri_state(), "workspace prev_on_output")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("6:b".into())
    );
}

#[test]
fn workspace_next_and_prev_follow_global_output_order_for_equal_numbers() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let first_output = f.niri_output(1).name();
    let second_output = f.niri_output(2).name();
    let client = f.add_client();
    for (workspace, output) in [
        ("1", &first_output),
        ("2", &second_output),
        ("6:c", &second_output),
        ("5", &first_output),
        ("6:a", &first_output),
        ("6:b", &first_output),
    ] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!("workspace {workspace} output {output}")
            )[0]
            .success
        );
    }

    assert!(crate::command::execute(f.niri_state(), "workspace 5")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace next")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("6:a".into())
    );

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("6:c".into())
    );
}

#[test]
fn workspace_next_and_prev_cross_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let first_output = f.niri_output(1).name();
    let second_output = f.niri_output(2).name();
    let client = f.add_client();
    for (workspace, output) in [("1", &first_output), ("2", &second_output)] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        assert!(
            crate::command::execute(
                f.niri_state(),
                &format!("workspace {workspace} output {output}")
            )[0]
            .success
        );
    }
    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        first_output
    );
    assert!(crate::command::execute(f.niri_state(), "workspace next")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        second_output
    );
}

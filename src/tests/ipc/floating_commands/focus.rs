#[test]
fn live_ipc_rectangle_roles_match_sway_relationships() {
    let expected = nested_fixture_tree();
    let actual = nested_live_tree();
    assert_rectangle_roles_match_fixture(&expected, &actual, "$tree");
}

#[test]
fn nested_tiling_rectangles_match_sway_roles() {
    let tree = nested_live_tree();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let top = &workspace["nodes"][0];
    let nested = &workspace["nodes"][1];
    let bottom_left = &nested["nodes"][0];
    let bottom_right = &nested["nodes"][1];

    assert_eq!(
        nested["rect"]["x"],
        bottom_left["rect"]["x"].as_i64().unwrap()
            - bottom_left["deco_rect"]["x"].as_i64().unwrap()
    );
    assert!(nested["rect"]["x"].as_i64().unwrap() > top["rect"]["x"].as_i64().unwrap());
    assert_eq!(
        nested["rect"]["y"],
        bottom_left["rect"]["y"].as_i64().unwrap()
            - bottom_left["deco_rect"]["height"].as_i64().unwrap()
    );
    assert_eq!(nested["rect"]["width"], bottom_left["rect"]["width"]);
    assert_eq!(
        nested["rect"]["height"],
        bottom_right["rect"]["y"].as_i64().unwrap()
            + bottom_right["rect"]["height"].as_i64().unwrap()
            - nested["rect"]["y"].as_i64().unwrap()
    );
    for window in [top, bottom_left, bottom_right] {
        assert!(window["deco_rect"]["height"].as_i64().unwrap() > 0);
        assert_eq!(window["current_border_width"], 4);
        assert_eq!(window["window_rect"]["x"], 4);
        assert_eq!(window["window_rect"]["y"], 0);
        assert_eq!(
            window["window_rect"]["width"].as_i64().unwrap(),
            window["rect"]["width"].as_i64().unwrap() - 8
        );
        assert_eq!(
            window["window_rect"]["height"].as_i64().unwrap(),
            window["rect"]["height"].as_i64().unwrap() - 4
        );
    }
}

#[test]
fn border_none_zeroes_deco_and_uses_the_whole_rect_for_window() {
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
    assert!(crate::command::execute(f.niri_state(), "border none")[0].success);

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let window = &tree["nodes"][1]["nodes"][0]["nodes"][0];
    assert_eq!(
        window["deco_rect"],
        serde_json::json!({"x": 0, "y": 0, "width": 0, "height": 0})
    );
    assert_eq!(window["window_rect"]["x"], 0);
    assert_eq!(window["window_rect"]["y"], 0);
    assert_eq!(window["window_rect"]["width"], window["rect"]["width"]);
    assert_eq!(window["window_rect"]["height"], window["rect"]["height"]);
}

#[test]
fn live_ipc_percent_matches_sway_parent_shares() {
    let expected = nested_fixture_tree();
    let representation = nested_representation_live_tree();
    assert_eq!(
        expected["nodes"][1]["nodes"][0]["representation"],
        representation["nodes"][1]["nodes"][0]["representation"],
        "workspace representation at $tree.nodes[1].nodes[0]"
    );
    let actual = nested_live_tree();
    assert_percent_value_matches_fixture(
        &expected["nodes"][1]["nodes"][0]["nodes"][1],
        &actual["nodes"][1]["nodes"][0]["nodes"][1],
        "$tree.nodes[1].nodes[0].nodes[1]",
    );
    assert_percent_matches_fixture(&expected, &actual, "$tree");
}

#[test]
fn root_focus_lists_outputs_once_in_global_mru_order() {
    let mut f = Fixture::new();
    for output in 1..=3 {
        f.add_output(output, (1280, 720));
    }
    let client = f.add_client();
    for output in [1, 2, 3] {
        f.niri_focus_output(output);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
    }

    let root_focus = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .focus
    };
    let output_ids = |f: &mut Fixture| {
        let swayward = f.swayward();
        describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &Default::default(),
            &Default::default(),
        )
        .nodes
        .into_iter()
        .filter(|node| node.name.as_deref() != Some("__i3"))
        .map(|node| node.id)
        .collect::<Vec<_>>()
    };

    let ids = output_ids(&mut f);
    assert_eq!(root_focus(&mut f), [ids[2], ids[1], ids[0]]);
    f.niri_focus_output(1);
    assert_eq!(root_focus(&mut f), [ids[0], ids[2], ids[1]]);
    let focus = root_focus(&mut f);
    assert_eq!(focus.len(), ids.len());
    assert_eq!(
        focus.iter().collect::<std::collections::HashSet<_>>().len(),
        ids.len()
    );
}

#[test]
fn criteria_focus_output_ignores_hidden_scratchpad_match_and_uses_seat_output() {
    let mut f = Fixture::new();
    f.add_named_output_at("left-head".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right-head".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);

    let command = format!(r#"[con_id="{hidden}"] focus output right-head"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "left-head"
    );
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "right-head"
    );
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn criteria_focus_output_succeeds_without_an_output_and_keeps_scratchpad_hidden() {
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
    let hidden = crate::ipc::tree::window_id(f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let output = f.swayward().layout.active_output().unwrap().clone();

    let command = format!(r#"[con_id="{hidden}"] focus output left"#);
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(f.swayward().layout.active_output(), Some(&output));
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 1);
    assert!(crate::command::execute(f.niri_state(), "nop")[0].success);
}

#[test]
fn focus_output_prefers_a_name_over_a_direction_and_resolves_directions() {
    let mut f = Fixture::new();
    f.add_named_output_at("origin".into(), (1280, 720), Some((0, 0)));
    f.add_named_output_at("left".into(), (1280, 720), Some((1280, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
    assert!(crate::command::execute(f.niri_state(), "focus output origin")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "left");
}

#[test]
fn focus_output_uses_nearest_geometry_then_wraps_to_farthest_opposite() {
    let mut f = Fixture::new();
    f.add_named_output_at("west".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("center".into(), (800, 600), Some((800, 0)));
    f.add_named_output_at("east".into(), (800, 600), Some((1600, 0)));
    f.add_named_output_at("far-east".into(), (800, 600), Some((2400, 0)));
    let client = f.add_client();

    let mut ids = std::collections::HashMap::new();
    for output in ["west", "east", "far-east", "center"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0].success
        );
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("{output}-window"));
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        f.niri_state().update_keyboard_focus();
        ids.insert(output, f.swayward().layout.focus().unwrap().id());
    }

    assert!(crate::command::execute(f.niri_state(), "focus output east")[0].success);
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);
    let latest = f.client(client).create_window();
    latest.xdg_toplevel.set_app_id("east-latest".into());
    latest.commit();
    let latest_surface = latest.surface.clone();
    f.roundtrip(client);
    let latest = f.client(client).window(&latest_surface);
    latest.attach_new_buffer();
    latest.ack_last_and_commit();
    f.double_roundtrip(client);
    f.niri_state().update_keyboard_focus();
    ids.insert("east", f.swayward().layout.focus().unwrap().id());
    assert!(crate::command::execute(f.niri_state(), "focus output center")[0].success);

    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "east");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["east"]);

    assert!(crate::command::execute(f.niri_state(), "focus output far-east")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus output right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "west");
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids["west"]);
}

#[test]
fn directional_focus_prefers_an_adjacent_output_over_local_wrapping() {
    for (target_position, layout, command, wrapping, expected_output) in [
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (0, 600),
            "layout stacking",
            "focus down",
            swayward_config::FocusWrapping::Yes,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Force,
            "source",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::No,
            "target",
        ),
        (
            (800, 0),
            "layout splith",
            "focus right",
            swayward_config::FocusWrapping::Workspace,
            "source",
        ),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = wrapping;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("target".into(), (800, 600), Some(target_position));
        let client = f.add_client();

        for output in ["target", "source", "source"] {
            assert!(
                crate::command::execute(f.niri_state(), &format!("focus output {output}"))[0]
                    .success
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
        assert!(crate::command::execute(f.niri_state(), layout)[0].success);

        assert!(crate::command::execute(f.niri_state(), command)[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected_output
        );
    }
}

#[test]
fn no_wrapping_crosses_an_adjacent_output_but_does_not_wrap_outputs() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::No;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));

    assert!(crate::command::execute(f.niri_state(), "focus output left")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(f.swayward().layout.active_output().unwrap().name(), "right");
}

#[test]
fn workspace_wrapping_uses_local_wrap_instead_of_an_adjacent_output() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();
    let mut ids = Vec::new();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
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

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "source"
    );
    assert_eq!(f.swayward().layout.focus().unwrap().id(), ids[0]);
}

#[test]
fn workspace_wrapping_allows_output_focus_from_a_focused_workspace_node() {
    let mut config = swayward_config::Config::default();
    config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
    let mut f = Fixture::with_config(config);
    f.add_named_output_at("source".into(), (800, 600), Some((0, 0)));
    f.add_named_output_at("target".into(), (800, 600), Some((800, 0)));
    let client = f.add_client();

    assert!(crate::command::execute(f.niri_state(), "focus output source")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .is_workspace_focused());

    assert!(crate::command::execute(f.niri_state(), "focus right")[0].success);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "target"
    );
}

#[test]
fn global_fullscreen_blocks_directional_output_focus_but_workspace_fullscreen_does_not() {
    for (global, direction, source, expected) in [
        (false, "right", "left", "right"),
        (false, "left", "right", "left"),
        (true, "right", "left", "left"),
        (true, "left", "right", "right"),
    ] {
        let mut config = swayward_config::Config::default();
        config.layout.focus_wrapping = swayward_config::FocusWrapping::Workspace;
        let mut f = Fixture::with_config(config);
        f.add_named_output_at("left".into(), (800, 600), Some((0, 0)));
        f.add_named_output_at("right".into(), (800, 600), Some((800, 0)));
        assert!(
            crate::command::execute(f.niri_state(), &format!("focus output {source}"))[0].success
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
        let mode = if global { " global" } else { "" };
        assert!(
            crate::command::execute(f.niri_state(), &format!("fullscreen enable{mode}"))[0].success
        );

        assert!(crate::command::execute(f.niri_state(), &format!("focus {direction}"))[0].success);
        assert_eq!(
            f.swayward().layout.active_output().unwrap().name(),
            expected,
            "global={global} direction={direction}"
        );
    }
}

#[test]
fn focus_output_reports_sway_errors() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    let output = f.swayward().layout.active_output().unwrap().clone();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output missing"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("There is no output with that name.".into()),
            parse_error: Some(true),
        }]
    );
    assert_eq!(f.swayward().layout.active_output(), Some(&output));

    let mut f = Fixture::new();
    assert_eq!(
        crate::command::execute(f.niri_state(), "focus output right"),
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("No focused workspace to base directions off of.".into()),
            parse_error: Some(false),
        }]
    );
}

#[test]
fn move_output_reports_the_missing_target() {
    let mut f = Fixture::new();
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

    assert_eq!(
        crate::command::execute(f.niri_state(), "move output missing")[0]
            .error
            .as_deref(),
        Some("Can't find output with name/direction 'missing'")
    );
}

#[test]
fn move_output_accepts_direction_name_and_workspace_forms() {
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

// sway/tree/container.c:977-980: returning a container to tiling removes it
// from the scratchpad, so a later `scratchpad show` finds nothing to toggle.
#[test]
fn unfloating_a_shown_scratchpad_window_removes_it_from_the_scratchpad() {
    for unfloat in ["floating disable", "floating toggle"] {
        let mut f = Fixture::new();
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
        let id = f.swayward().layout.focus().unwrap().window.clone();

        assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
        assert!(crate::command::execute(f.niri_state(), "scratchpad show")[0].success);
        assert!(f.swayward().layout.is_scratchpad_window(&id));

        let outcome = crate::command::execute(f.niri_state(), unfloat);
        assert!(outcome[0].success, "{unfloat}: {outcome:?}");
        assert!(!f.swayward().layout.is_scratchpad_window(&id), "{unfloat}");
        assert!(
            !f.swayward().layout.focus().unwrap().is_floating(),
            "{unfloat}"
        );
        assert_eq!(
            crate::command::execute(f.niri_state(), "scratchpad show")[0]
                .error
                .as_deref(),
            Some("Scratchpad is empty"),
            "{unfloat}"
        );
        assert_eq!(
            f.swayward().layout.focus().map(|w| w.window.clone()),
            Some(id)
        );
    }
}

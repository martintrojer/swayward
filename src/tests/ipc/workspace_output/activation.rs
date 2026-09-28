#[test]
fn xdg_activation_policy_matches_sway_for_tokens_and_visibility() {
    use swayward_config::FocusOnWindowActivation::{Focus, None, Smart, Urgent};

    for (policy, focused_token, visible, expect_focus, expect_urgent) in [
        (Smart, true, true, true, false),
        (Smart, true, false, false, true),
        (Urgent, true, true, false, true),
        (Focus, true, false, true, false),
        (None, true, true, false, false),
        (Smart, false, true, false, true),
        (Urgent, false, true, false, true),
        (Focus, false, true, false, true),
        (None, false, true, false, false),
    ] {
        let config = swayward_config::Config {
            focus_on_window_activation: policy,
            ..Default::default()
        };
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1920, 1080));
        let client = f.add_client();
        map_test_window(&mut f, client, "target");
        let target = f.swayward().layout.focus().unwrap().id();
        let target_surface = f.client(client).state.windows[0].surface.clone();
        if visible {
            map_test_window(&mut f, client, "other");
        } else {
            assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
            map_test_window(&mut f, client, "other");
        }
        let requesting_surface = focused_token.then_some(target_surface.clone());
        let token = f
            .client(client)
            .request_activation_token_with_focus(requesting_surface.as_ref());
        f.double_roundtrip(client);
        let token = token.lock().unwrap().take().unwrap();
        f.client(client).activate(token, &target_surface);
        f.double_roundtrip(client);

        assert_eq!(
            f.swayward().layout.focus().map(|w| w.id()) == Some(target),
            expect_focus,
            "policy={policy:?} focused_token={focused_token} visible={visible}"
        );
        assert_eq!(
            test_window_is_urgent(&mut f, "target"),
            expect_urgent,
            "policy={policy:?} focused_token={focused_token} visible={visible}"
        );
    }

    // Focus mode reveals a hidden scratchpad target before focusing it.
    let config = swayward_config::Config {
        focus_on_window_activation: Focus,
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    let target = f.swayward().layout.focus().unwrap().id();
    let target_window = f.swayward().layout.focus().unwrap().window.clone();
    let surface = f.client(client).state.windows[0].surface.clone();
    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let token = f
        .client(client)
        .request_activation_token_with_focus(Some(&surface));
    f.double_roundtrip(client);
    let token = token.lock().unwrap().take().unwrap();
    f.client(client).activate(token, &surface);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().layout.focus().map(|w| w.id()), Some(target));
    assert!(!f.swayward().layout.is_scratchpad_hidden(&target_window));
}

#[test]
fn focusing_an_urgent_window_clears_urgency_immediately() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["target", "focused"] {
        map_test_window(&mut f, client, app_id);
    }
    set_test_window_urgent(&mut f, "target");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let workspaces = serde_json::to_value(describe_workspaces(
        &swayward.layout,
        &swayward.global_space,
    ))
    .unwrap();
    assert_eq!(
        find_json_node_with_app_id(&tree, "target").unwrap()["urgent"],
        true
    );
    assert!(workspaces
        .as_array()
        .unwrap()
        .iter()
        .any(|ws| ws["urgent"] == true));

    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "target"));
    set_test_window_urgent(&mut f, "target");
    assert!(!test_window_is_urgent(&mut f, "target"));
}

#[test]
fn urgent_criteria_selects_windows_by_urgency_timestamp() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    for app_id in ["oldest", "latest", "focused"] {
        map_test_window(&mut f, client, app_id);
    }
    set_test_window_urgent_at(&mut f, "oldest", Duration::from_millis(1));
    set_test_window_urgent_at(&mut f, "latest", Duration::from_millis(2));

    assert!(crate::command::execute(f.niri_state(), "[urgent=oldest] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "oldest"));
    assert!(crate::command::execute(f.niri_state(), "[urgent=latest] focus")[0].success);
    f.double_roundtrip(client);
    assert!(!test_window_is_urgent(&mut f, "latest"));
}

#[test]
fn cross_workspace_focus_delays_urgency_clear_without_restarting_timer() {
    let config = swayward_config::Config {
        urgent_timeout_ms: 60_000,
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "other");
    set_test_window_urgent(&mut f, "target");

    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert!(test_window_is_urgent(&mut f, "target"));
    let target_id = f
        .swayward()
        .layout
        .windows()
        .find_map(|(_, window)| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| {
                (role.app_id.as_deref() == Some("target")).then_some(window.id())
            })
        })
        .unwrap();
    let first_timer = *f.swayward().urgency_timers.get(&target_id).unwrap();
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert_eq!(
        f.swayward().urgency_timers.get(&target_id),
        Some(&first_timer)
    );
    assert!(f.swayward().fire_urgency_timer_for_test(target_id));
    assert!(!test_window_is_urgent(&mut f, "target"));
}

#[test]
fn closing_a_window_cancels_its_pending_urgency_timer() {
    let config = swayward_config::Config {
        urgent_timeout_ms: 60_000,
        ..Default::default()
    };
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "target");
    let target_surface = f.client(client).state.windows[0].surface.clone();
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "other");
    set_test_window_urgent(&mut f, "target");
    assert!(crate::command::execute(f.niri_state(), "[app_id=target] focus")[0].success);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().urgency_timers.len(), 1);

    f.client(client).window(&target_surface).attach_null();
    f.client(client).window(&target_surface).commit();
    f.double_roundtrip(client);
    assert!(f.swayward().urgency_timers.is_empty());
}

#[test]
fn targeted_focus_selects_the_requested_unfocused_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for app_id in ["first", "middle", "last"] {
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

    assert!(crate::command::execute(f.niri_state(), r#"[app_id="middle"] focus"#)[0].success);

    let focused_app_id = f.swayward().layout.focus().map(|window| {
        crate::utils::with_toplevel_role(window.toplevel(), |role| role.app_id.clone())
    });
    assert_eq!(focused_app_id, Some(Some("middle".into())));
}

#[test]
fn workspace_auto_back_and_forth_honors_global_and_command_settings() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = true;

    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }

    // Enabled + active target + previous workspace: bounce.
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );

    // A different target switches normally instead of bouncing.
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(2)
    );

    // The command-level override suppresses the enabled global option.
    assert!(
        crate::command::execute(f.niri_state(), "workspace --no-auto-back-and-forth 2",)[0].success
    );
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(2)
    );

    // With the option disabled, selecting the active workspace remains there.
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = false;
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );
}

#[test]
fn move_no_auto_back_and_forth_changes_the_same_workspace_destination() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.swayward()
        .config
        .borrow_mut()
        .input
        .workspace_auto_back_and_forth = true;

    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace 1")[0].success);

    let client = f.add_client();
    for app_id in ["normal", "suppressed"] {
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
        crate::command::execute(f.niri_state(), r#"[app_id="normal"] move workspace 1"#)[0].success
    );
    assert!(
        crate::command::execute(
            f.niri_state(),
            r#"[app_id="suppressed"] move --no-auto-back-and-forth window to workspace 1"#,
        )[0]
        .success
    );

    let workspace_apps = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace.number().map(|number| {
                let apps = workspace
                    .windows()
                    .filter_map(|window| {
                        crate::utils::with_toplevel_role(window.toplevel(), |role| {
                            role.app_id.clone()
                        })
                    })
                    .collect::<Vec<_>>();
                (number, apps)
            })
        })
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(workspace_apps[&2], ["normal"]);
    assert_eq!(workspace_apps[&1], ["suppressed"]);
}


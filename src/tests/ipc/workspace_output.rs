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

#[test]
fn workspace_back_and_forth_without_history_uses_sway_error() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "workspace back_and_forth",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(
        reply,
        r#"[{"success":false,"error":"There is no previous workspace"}]"#
    );
}

#[test]
fn workspace_back_and_forth_recreates_a_reaped_previous_workspace() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let mut stream = UnixStream::connect(socket).unwrap();

    for command in ["workspace 1", "workspace 2"] {
        stream
            .write_all(&swayward_ipc::wire::encode(
                MessageType::RunCommand,
                command,
            ))
            .unwrap();
        let (_, reply) = read_ipc_reply(&mut f, &mut stream);
        assert_eq!(reply, r#"[{"success":true}]"#);
    }

    f.swayward().clock.set_complete_instantly(true);
    f.swayward().layout.advance_animations();
    f.swayward().clock.set_complete_instantly(false);
    assert!(f
        .swayward()
        .layout
        .workspaces()
        .all(|(_, _, workspace)| workspace.number() != Some(1)));

    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "workspace back_and_forth",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut stream);
    assert_eq!(reply, r#"[{"success":true}]"#);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().number(),
        Some(1)
    );
}

#[test]
fn move_workspace_back_and_forth_targets_the_previous_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();

    for workspace in ["1", "2"] {
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

    assert!(crate::command::execute(f.niri_state(), "move workspace back_and_forth")[0].success);

    let workspace_apps = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| {
            workspace.number().map(|number| {
                let apps = workspace
                    .windows()
                    .map(|window| {
                        crate::utils::with_toplevel_role(window.toplevel(), |role| {
                            role.app_id.clone().unwrap()
                        })
                    })
                    .collect::<Vec<_>>();
                (number, apps)
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workspace_apps,
        [(1, vec!["1".into(), "2".into()]), (2, vec![])]
    );
}

#[test]
fn move_workspace_current_keeps_the_window_on_its_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "move workspace current")[0].success);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    // Find workspace 7 rather than assuming it is first: sway sorts numbered
    // workspaces numerically (sway/sway/tree/output.c:387-405), so the startup
    // workspace 1 precedes it. The point of this test is that the window stays
    // on the workspace it was on, not where that workspace sorts.
    let seven = workspaces
        .iter()
        .find(|workspace| workspace.num == 7)
        .expect("workspace 7 exists");
    assert_eq!(seven.focus.len(), 1);
}

#[test]
fn move_to_workspace_creates_the_target_and_moves_the_window() {
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

    assert!(crate::command::execute(f.niri_state(), "move to workspace 7")[0].success);

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    let workspace = workspaces
        .iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.focus.len(), 1);
    assert!(!workspace.focused);
}

#[test]
fn move_workspace_focuses_the_moved_window_in_the_destination_reply() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "destination-window");
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    map_test_window(&mut f, client, "moved-window");

    let mut stream = UnixStream::connect(socket).unwrap();
    assert_eq!(
        query_ipc_with_payload(
            &mut f,
            &mut stream,
            MessageType::RunCommand,
            "move workspace 1",
        ),
        serde_json::json!([{"success": true}])
    );
    assert_eq!(
        query_ipc_with_payload(&mut f, &mut stream, MessageType::RunCommand, "workspace 1",),
        serde_json::json!([{"success": true}])
    );
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);

    assert_eq!(
        find_json_node_with_app_id(&tree, "moved-window").unwrap()["focused"],
        true
    );
    assert_eq!(
        find_json_node_with_app_id(&tree, "destination-window").unwrap()["focused"],
        false
    );
}

#[test]
fn workspace_output_assignment_moves_the_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let target_output = f.niri_output(2).name();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let command = format!("workspace 7 output {target_output}");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    let swayward = f.swayward();
    let workspace = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.output, target_output);
}

/// Sway accepts a LIST of outputs and uses the first that resolves
/// (`sway/sway/commands/workspace.c:153-155`;
/// `sway/sway/tree/workspace.c:244-250`). The list must not be joined into one
/// name, which is what a single-output parser would do.
#[test]
fn workspace_output_assignment_uses_the_first_available_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let present = f.niri_output(2).name();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    // First two names do not resolve, so the third wins.
    let command = format!("workspace 7 output missing-a missing-b {present}");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    let swayward = f.swayward();
    let workspace = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.output, present);
}

/// A list naming only absent outputs resolves to nothing and must fail rather
/// than silently doing nothing.
#[test]
fn workspace_output_assignment_fails_when_no_output_resolves() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);

    let result =
        &crate::command::execute(f.niri_state(), "workspace 7 output missing-a missing-b")[0];
    assert!(!result.success);
}

#[test]
fn ipc_output_reports_sways_clockwise_transform_names() {
    for (transform, expected) in [
        (smithay::utils::Transform::Normal, "normal"),
        (smithay::utils::Transform::_90, "270"),
        (smithay::utils::Transform::_180, "180"),
        (smithay::utils::Transform::_270, "90"),
        (smithay::utils::Transform::Flipped, "flipped"),
        (smithay::utils::Transform::Flipped90, "flipped-270"),
        (smithay::utils::Transform::Flipped180, "flipped-180"),
        (smithay::utils::Transform::Flipped270, "flipped-90"),
    ] {
        assert_eq!(crate::ipc::tree::sway_transform(transform), expected);
    }
}

#[test]
fn get_outputs_reads_the_live_transform_and_subpixel_layout() {
    let (mut f, socket) = ipc_fixture();
    let output = smithay::output::Output::new(
        "test-output".into(),
        smithay::output::PhysicalProperties {
            size: (0, 0).into(),
            subpixel: smithay::output::Subpixel::HorizontalRgb,
            make: "test".into(),
            model: "test".into(),
            serial_number: "test".into(),
        },
    );
    let mode = smithay::output::Mode {
        size: (1280, 720).into(),
        refresh: 60_000,
    };
    output.change_current_state(Some(mode), None, None, None);
    output.set_preferred(mode);
    output.user_data().insert_if_missing(|| OutputName {
        connector: "test-output".into(),
        make: Some("test".into()),
        model: Some("test".into()),
        serial: Some("test".into()),
    });
    f.swayward().add_output(output.clone(), None, false);
    output.change_current_state(None, Some(smithay::utils::Transform::_90), None, None);
    f.niri_state().ipc_refresh_layout();

    let mut stream = UnixStream::connect(socket).unwrap();
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(outputs[0]["transform"], "270");
    assert_eq!(outputs[0]["subpixel_hinting"], "rgb");
}

#[test]
fn get_tree_ids_are_unique_across_workspaces() {
    fn collect_ids(node: &swayward_ipc::Node, ids: &mut Vec<i64>) {
        ids.push(node.id);
        for child in node.nodes.iter().chain(&node.floating_nodes) {
            collect_ids(child, ids);
        }
    }

    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for index in 0..3 {
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
            if index == 1 {
                assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
            }
        }
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut ids = Vec::new();
    collect_ids(&tree, &mut ids);
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

#[test]
fn ipc_output_reports_sways_subpixel_names() {
    for (subpixel, expected) in [
        (smithay::output::Subpixel::Unknown, "unknown"),
        (smithay::output::Subpixel::None, "none"),
        (smithay::output::Subpixel::HorizontalRgb, "rgb"),
        (smithay::output::Subpixel::HorizontalBgr, "bgr"),
        (smithay::output::Subpixel::VerticalRgb, "vrgb"),
        (smithay::output::Subpixel::VerticalBgr, "vbgr"),
    ] {
        assert_eq!(crate::ipc::tree::sway_subpixel_hinting(subpixel), expected);
    }
}

#[test]
fn ipc_output_rects_use_global_positions() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let swayward = f.swayward();
    let outputs = describe_outputs(&swayward.layout, &swayward.global_space);
    let rects = outputs.iter().map(|output| output.rect).collect::<Vec<_>>();
    assert_eq!(rects[0].x, 0);
    assert_eq!(rects[0].width, 1280);
    assert_eq!(rects[1].x, 1280);
    assert_eq!(rects[1].width, 1920);

    let root = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(root.rect.width, 3200);
    assert_eq!(root.rect.height, 1080);
    assert_eq!(
        root.nodes[1].percent,
        Some((1280 * 720) as f64 / (3200 * 1080) as f64)
    );
    assert_eq!(root.nodes[2].percent, Some(1920. / 3200.));
}

#[test]
fn stale_tree_leaf_is_omitted_without_panicking() {
    let tree = IpcNode::Leaf {
        id: NodeId(1),
        window: (),
        percent: Some(1.),
        focused: false,
        fullscreen_mode: 0,
        rect: Default::default(),
        deco_rect: None,
        border: (swayward_ipc::command::BorderStyle::Normal, 2),
        border_edges: crate::utils::ResizeEdge::all(),
    };
    assert!(crate::ipc::tree::describe_tiling(
        tree,
        &|_| None,
        Default::default(),
        &Default::default(),
        &Default::default(),
        crate::layout::workspace::WorkspaceId::specific(1)
    )
    .is_none());

    let tree = IpcNode::Split {
        id: NodeId(0),
        layout: TreeLayout::SplitH,
        title: None,
        percent: None,
        rect: Default::default(),
        focus: vec![NodeId(1)],
        focused: false,
        fullscreen_mode: 0,
        children: vec![IpcNode::Leaf {
            id: NodeId(1),
            window: (),
            percent: Some(1.),
            focused: false,
            fullscreen_mode: 0,
            rect: Default::default(),
            deco_rect: None,
            border: (swayward_ipc::command::BorderStyle::Normal, 2),
            border_edges: crate::utils::ResizeEdge::all(),
        }],
    };
    let node = crate::ipc::tree::describe_tiling(
        tree,
        &|_| None,
        Default::default(),
        &Default::default(),
        &Default::default(),
        crate::layout::workspace::WorkspaceId::specific(1),
    )
    .unwrap();
    assert!(node.nodes.is_empty());
}

#[test]
fn live_ipc_focus_matches_sway_mru_arrays() {
    assert_focus_matches_fixture(&nested_fixture_tree(), &nested_live_tree(), "$tree");
}

#[test]
fn workspace_focus_spans_tiled_and_floating_children() {
    let expected = mixed_fixture_tree();
    let actual = mixed_live_tree();
    assert_focus_matches_fixture(&expected, &actual, "$tree");

    let workspace = &actual["nodes"][1]["nodes"][0];
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(workspace["floating_nodes"].as_array().unwrap().len(), 1);
    assert_eq!(workspace["focus"].as_array().unwrap().len(), 2);
}

fn floating_order(tree: &Value) -> (Vec<&str>, Vec<&str>) {
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] != "__i3")
        .unwrap()["nodes"][0]
        .as_object()
        .unwrap();
    let floating = workspace["floating_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let focus = workspace["focus"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|id| {
            workspace["floating_nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["id"] == *id)
                .and_then(|node| node["app_id"].as_str())
        })
        .collect::<Vec<_>>();
    (floating, focus)
}

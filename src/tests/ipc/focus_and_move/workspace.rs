#[test]
fn killing_focused_workspace_closes_tiled_and_floating_windows() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 9")[0].success);
    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
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

    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(f.niri_state(), "kill")[0].success);
    f.double_roundtrip(client);

    assert_eq!(
        f.client(client)
            .state
            .windows
            .iter()
            .filter(|window| window.close_requested)
            .count(),
        2
    );
    let closed = f
        .client(client)
        .state
        .windows
        .iter()
        .filter(|window| window.close_requested)
        .map(|window| window.surface.clone())
        .collect::<Vec<_>>();
    for surface in closed {
        let window = f.client(client).window(&surface);
        window.attach_null();
        window.commit();
    }
    f.double_roundtrip(client);
    let workspace = f.swayward().layout.active_workspace().unwrap();
    assert_eq!(workspace.number(), Some(7));
    assert_eq!(workspace.windows().count(), 0);
    let mut numbers = f
        .swayward()
        .layout
        .workspaces()
        .filter_map(|(_, _, workspace)| workspace.number())
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    // Workspace 1 is gone, not missing. It was created empty with the output,
    // and focus left it for workspace 9 without ever placing a window on it,
    // so sway destroys it. Measured on real sway 1.11 (headless, one output):
    // focusing an empty workspace 7 then switching away leaves
    // get_workspaces reporting ['1', '2', '9'] with no 7, while an empty
    // workspace that still holds focus is reported. See
    // workspace_consider_destroy, sway/tree/workspace.c:313-330, reached from
    // seat_set_focus, sway/input/seat.c:1244.
    assert_eq!(numbers, [7, 9]);
}

#[test]
fn closing_last_window_removes_inactive_named_workspace_from_ipc() {
    let mut config = swayward_config::Config::default();
    config.animations.off = true;
    let mut f = Fixture::with_config(config);
    let handle = f.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    f.swayward().ipc_server = Some(ipc_server);
    f.niri_state().ipc_keyboard_layouts_changed();
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
    assert!(crate::command::execute(f.niri_state(), "workspace active")[0].success);
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);
    let window = f.client(client).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let mut stream = UnixStream::connect(socket).unwrap();
    let workspaces = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    let names = workspaces
        .as_array()
        .unwrap()
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["active"]);
    let tree = query_ipc(&mut f, &mut stream, MessageType::GetTree);
    let mut tree_workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut tree_workspaces);
    let names = tree_workspaces
        .iter()
        .map(|workspace| workspace["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["__i3_scratch", "active"]);

    let (event_type, payload) = read_ipc_reply(&mut f, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let actual = serde_json::from_str::<Value>(&payload).unwrap();
    let expected =
        serde_json::from_str::<Value>(sway_fixture!("events/workspace.empty.json")).unwrap();
    assert_eq!(
        actual.as_object().unwrap().keys().collect::<BTreeSet<_>>(),
        expected
            .as_object()
            .unwrap()
            .keys()
            .collect::<BTreeSet<_>>()
    );
    assert_same_shape(
        &expected["current"],
        &actual["current"],
        "$workspace.current",
    );
    assert_eq!(actual["change"], "empty");
    assert_eq!(actual["current"]["name"], "7");
    assert_eq!(actual["current"]["focused"], false);
    assert_eq!(actual["current"]["nodes"], serde_json::json!([]));

    assert!(crate::command::execute(f.niri_state(), "workspace prev")[0].success);
    let after_prev = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(
        after_prev
            .as_array()
            .unwrap()
            .iter()
            .find(|workspace| workspace["focused"] == true)
            .unwrap()["name"],
        "active"
    );

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let recreated = query_ipc(&mut f, &mut stream, MessageType::GetWorkspaces);
    assert_eq!(
        recreated
            .as_array()
            .unwrap()
            .iter()
            .filter(|workspace| workspace["num"] == 7)
            .count(),
        1
    );
}

#[test]
fn initial_workspace_name_comes_from_the_first_available_default_mode_binding() {
    for (config, expected) in [
        (
            r#"binds {
                code:24 { command "workspace keycode-first"; }
                X { command "workspace keysym-second"; }
            }"#,
            "keycode-first",
        ),
        (
            r#"binds {
                X { command "workspace keysym-first"; }
                code:24 { command "workspace keycode-second"; }
            }"#,
            "keysym-first",
        ),
        (
            r#"binds {
                X { command "workspace next"; }
                Y { command "workspace prev"; }
                Z { command "workspace next_on_output"; }
                A { command "workspace prev_on_output"; }
                B { command "workspace back_and_forth"; }
                C { command "workspace current"; }
                D { command "workspace number"; }
                code:24 { command "workspace number 7: eggs"; }
            }"#,
            "7: eggs",
        ),
        (
            r#"binds {
                X { focus-workspace "typed"; }
                Y { command "workspace string-second"; }
            }"#,
            "typed",
        ),
        (
            r#"binds {
                X { focus-workspace 7; }
            }
            mode "other" {
                Y { command "workspace ignored-mode"; }
            }"#,
            "7",
        ),
        (
            r#"binds {
                X { command "workspace   3"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace 3; exec foo"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace 3"; }
            }"#,
            "3",
        ),
        (
            r#"binds {
                X { command "workspace --no-auto-back-and-forth number 3:three"; }
            }"#,
            "3:three",
        ),
    ] {
        let config = swayward_config::Config::parse_mem(config).unwrap();
        let mut f = Fixture::with_config(config);
        f.add_output(1, (1920, 1080));
        assert_eq!(
            f.swayward().layout.active_workspace().unwrap().sway_name(),
            Some(expected.to_owned())
        );
    }

    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    for (command, expected) in [
        ("workspace foobar", "foobar"),
        ("workspace   3", "3"),
        ("workspace 3; exec foo", "3"),
        ("workspace 3", "3"),
        (
            "workspace --no-auto-back-and-forth number 3:three",
            "3:three",
        ),
    ] {
        let config = swayward_config::Config::parse_mem(&format!(
            "binds {{\n    X {{ command {command:?}; }}\n}}"
        ))
        .unwrap();
        f.swayward()
            .layout
            .initialize_workspaces_from_bindings(&config);
        assert_eq!(
            f.swayward().layout.active_workspace().unwrap().sway_name(),
            Some(expected.to_owned())
        );
    }

    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace taken"; }
            code:24 { command "workspace fresh"; }
        }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("taken".to_owned())
    );
    f.add_output(2, (1920, 1080));
    f.niri_focus_output(2);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("fresh".to_owned())
    );
}

#[test]
fn configured_workspace_is_destroyed_when_empty_and_inactive() {
    let config = swayward_config::Config::parse_mem(r#"workspace "configured" {}"#).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    assert!(
        crate::command::execute(f.niri_state(), "rename workspace configured to renamed")[0]
            .success
    );
    assert!(crate::command::execute(f.niri_state(), "workspace 2")[0].success);
    f.swayward().clock.set_complete_instantly(true);
    f.swayward().layout.advance_animations();
    f.swayward().clock.set_complete_instantly(false);

    assert!(!f
        .swayward()
        .layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("renamed")));
}

#[test]
fn named_workspace_has_no_number_and_active_empty_workspace_remains_visible() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "workspace mail")[0].success);
    f.niri_state().ipc_refresh_layout();

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(workspaces.len(), 1);
    let named = workspaces
        .iter()
        .find(|workspace| workspace.name == "mail")
        .unwrap();
    assert_eq!(named.num, -1);
    assert!(named.visible);
    assert!(named.focused);
}

#[test]
fn negative_and_unnumbered_workspace_names_report_minus_one_without_affecting_order() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    for workspace in ["mail", "-42: negative", "7: numbered"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
    }
    let rename = crate::command::execute(f.niri_state(), "rename workspace mail to inbox");
    assert!(!rename[0].success, "{rename:?}");
    assert_eq!(rename[0].parse_error, Some(true));
    f.niri_state().ipc_refresh_layout();

    let swayward = f.swayward();
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
    assert_eq!(
        workspaces
            .iter()
            .map(|workspace| (workspace.name.as_str(), workspace.num))
            .collect::<Vec<_>>(),
        [("7: numbered", 7)]
    );
}

#[test]
fn relative_move_includes_empty_active_workspace_and_uses_direction() {
    for (source, direction) in [(1, "next"), (3, "prev")] {
        let mut f = Fixture::new();
        for output in 1..=3 {
            f.add_output(output, (1920, 1080));
        }
        let outputs = [
            f.niri_output(1).name(),
            f.niri_output(2).name(),
            f.niri_output(3).name(),
        ];
        let client = f.add_client();

        assert!(crate::command::execute(f.niri_state(), &format!("workspace {source}"))[0].success);
        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        for workspace in 1..=3 {
            assert!(
                crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0]
                    .success
            );
            assert!(
                crate::command::execute(
                    f.niri_state(),
                    &format!("workspace {workspace} output {}", outputs[workspace - 1])
                )[0]
                .success
            );
        }

        assert!(crate::command::execute(
            f.niri_state(),
            &format!("workspace {source}, move workspace {direction}")
        )
        .iter()
        .all(|outcome| outcome.success));

        let swayward = f.swayward();
        let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
        let window_counts = workspaces
            .iter()
            .map(|workspace| (workspace.num, workspace.focus.len()))
            .collect::<Vec<_>>();
        assert_eq!(window_counts, [(1, 0), (2, 1), (3, 0)]);
    }
}

#[test]
fn targeted_focus_reveals_a_hidden_scratchpad_window() {
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

    assert!(crate::command::execute(f.niri_state(), "move scratchpad")[0].success);
    let outcome = crate::command::execute(f.niri_state(), r#"[title="target"] focus workspace"#);
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(f.swayward().layout.scratchpad_windows().count(), 0);
    assert!(f.swayward().layout.focus().is_some());
}

fn set_test_window_urgent_at(f: &mut Fixture, app_id: &str, now: Duration) {
    f.swayward().layout.with_windows_mut(|window, _| {
        if crate::utils::with_toplevel_role(window.toplevel(), |role| {
            role.app_id.as_deref() == Some(app_id)
        }) {
            window.set_urgent_for_test(true, now);
        }
    });
}

fn set_test_window_urgent(f: &mut Fixture, app_id: &str) {
    set_test_window_urgent_at(f, app_id, crate::utils::get_monotonic_time());
}

fn test_window_is_urgent(f: &mut Fixture, app_id: &str) -> bool {
    f.swayward()
        .layout
        .windows()
        .find(|(_, window)| {
            crate::utils::with_toplevel_role(window.toplevel(), |role| {
                role.app_id.as_deref() == Some(app_id)
            })
        })
        .unwrap()
        .1
        .is_urgent()
}

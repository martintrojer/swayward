#[test]
fn reload_rereads_config_and_emits_the_sway_workspace_event() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let path = std::env::temp_dir().join(format!(
        "swayward-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, "layout { gaps 7; }").unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "reload")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, 1 << 31);
    let expected: Value =
        serde_json::from_str(&sway_fixture!("events/workspace.reload.json")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&payload).unwrap(), expected);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 7.);
    subscriber.set_nonblocking(true).unwrap();
    fixture.dispatch();
    let mut byte = [0];
    assert!(matches!(
        subscriber.read(&mut byte),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));

    std::fs::remove_file(path).unwrap();
}

#[test]
fn reload_reports_malformed_config_in_the_command_reply() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let mut fixture = Fixture::new();
    let path = std::env::temp_dir().join(format!(
        "swayward-bad-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, "binds { Mod+H { command; }; }").unwrap();
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path.clone()),
        Vec::new(),
    );

    let outcome = crate::command::execute(fixture.niri_state(), "reload");
    assert_eq!(
        outcome,
        [swayward_ipc::CommandOutcome {
            success: false,
            error: Some("Error(s) reloading config.".into()),
            parse_error: Some(false),
        }]
    );

    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_config_reload_keeps_the_compositor_responsive() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let malformed =
        swayward_config::Config::parse_mem("binds { Mod+H { command; }; }").map_err(|error| {
            assert!(format!("{error:?}").contains("expected command"));
        });
    assert!(malformed.is_err());

    fixture.niri_state().reload_config(malformed);

    let outcome = crate::command::execute(fixture.niri_state(), "nop");
    assert_eq!(outcome.len(), 1);
    assert!(outcome[0].success, "{outcome:?}");
}

#[test]
fn reload_replaces_map_time_rules_while_windows_are_mapped() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let first = fixture.client(client).create_window();
    first.xdg_toplevel.set_app_id("special".into());
    first.commit();
    let surface = first.surface.clone();
    fixture.roundtrip(client);
    let first = fixture.client(client).window(&surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let first_id = fixture.swayward().layout.focus().unwrap().id();

    super::i3_conformance::reload_test_config(
        &mut fixture,
        r#"for_window [app_id="special"] mark reloaded"#,
    )
    .unwrap();

    let second = fixture.client(client).create_window();
    second.xdg_toplevel.set_app_id("special".into());
    second.commit();
    let surface = second.surface.clone();
    fixture.roundtrip(client);
    let second = fixture.client(client).window(&surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let second_id = fixture.swayward().layout.focus().unwrap().id();

    let marks = &fixture.swayward().marks_by_window;
    assert!(marks.get(&first_id).is_none_or(Vec::is_empty));
    assert_eq!(
        marks.get(&second_id).map(Vec::as_slice),
        Some(["reloaded".to_owned()].as_slice())
    );
}

#[test]
fn reload_updates_runtime_for_window_execution_state() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "for_window [con_mark=trigger] mark --add fired",
        )[0]
        .success
    );

    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let mapped = fixture.swayward().layout.focus().unwrap().id();

    fixture.swayward().executed_for_window.insert((
        mapped,
        "[con_mark=trigger]".into(),
        "mark --add fired".into(),
    ));
    fixture.niri_state().reload_config(Err(()));
    assert!(
        !fixture.swayward().executed_for_window.is_empty(),
        "a failed reload must preserve criteria from the active config"
    );
    fixture.swayward().executed_for_window.clear();

    assert!(crate::command::execute(fixture.niri_state(), "mark trigger")[0].success);
    assert!(fixture
        .swayward()
        .executed_for_window
        .iter()
        .any(|(window, criteria, _)| *window == mapped && criteria == "[con_mark=trigger]"));
    assert!(!fixture.swayward().runtime_for_window.is_empty());

    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(fixture.swayward().executed_for_window.is_empty());
    assert!(fixture.swayward().for_window.is_empty());
    assert!(fixture.swayward().runtime_for_window.is_empty());
}

#[test]
fn title_format_updates_get_tree_and_titlebar_after_client_title_change() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("format-app".into());
    window.set_title("before");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    assert!(crate::command::execute(fixture.niri_state(), "border normal")[0].success);
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[app_id="format-app"] title_format [%app_id|%shell|%class|%instance|%sandbox_engine|%sandbox_app_id|%sandbox_instance_id] %title"#,
    );
    assert!(outcome[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let (event_type, event) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let event: Value = serde_json::from_str(&event).unwrap();
    assert_eq!(event["change"], "title");
    assert_eq!(event["container"]["name"], "before");
    subscriber.set_nonblocking(true).unwrap();
    let mut byte = [0];
    assert!(matches!(
        subscriber.read(&mut byte),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "before"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert_eq!(
        workspace.tiling().titlebar_titles(),
        ["[format-app|xdg_shell|||||] before"]
    );

    let window = fixture.client(client).window(&surface);
    window.set_title("%app_id");
    window.commit();
    fixture.double_roundtrip(client);

    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "%app_id"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert_eq!(
        workspace.tiling().titlebar_titles(),
        ["[format-app|xdg_shell|||||] %app_id"]
    );

    assert!(crate::command::execute(fixture.niri_state(), "title_format %title")[0].success);
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    assert_eq!(
        find_json_node(&tree, "con", true).unwrap()["name"],
        "%app_id"
    );
}

#[test]
fn title_format_updates_a_split_container_representation() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for title in ["one", "two", "three"] {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(format!("app-{title}"));
        window.set_title(title);
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    fixture.swayward().layout.nest_or_unnest_window_left(None);
    assert!(crate::command::execute(fixture.niri_state(), "focus parent")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "mark formatted-split")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        r#"[con_mark=formatted-split] title_format group: %title %app_id"#,
    );
    assert!(outcome[0].success, "{outcome:?}");

    let mut stream = UnixStream::connect(socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
    let split = find_json_node_with_mark(&tree, "formatted-split").unwrap();
    assert_eq!(split["name"], Value::Null);
    let workspace_node = tree["nodes"][1]["nodes"][0].as_object().unwrap();
    assert_eq!(
        workspace_node["representation"],
        "T[app-one V[app-three app-two]]"
    );
    let workspace = fixture.swayward().layout.active_workspace().unwrap();
    assert!(workspace
        .tiling()
        .titlebar_titles()
        .iter()
        .any(|title| title == "group: V[three two] %app_id"));
}

#[test]
fn translated_for_window_nop_has_no_observable_window_effect() {
    fn mapped_leaf(config: Option<&str>) -> Value {
        let mut fixture = Fixture::new();
        fixture.add_output(1, (1920, 1080));
        if let Some(config) = config {
            super::i3_conformance::reload_test_config(&mut fixture, config).unwrap();
        }
        let client = fixture.add_client();
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id("nop-target".into());
        window.set_title("unchanged");
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);

        let swayward = fixture.swayward();
        let tree = serde_json::to_value(describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        ))
        .unwrap();
        let mut leaf = find_json_node(&tree, "con", true).unwrap().clone();
        let leaf = leaf.as_object_mut().unwrap();
        leaf.remove("id");
        leaf.remove("foreign_toplevel_identifier");
        Value::Object(leaf.clone())
    }

    let baseline = mapped_leaf(Some(
        r#"for_window [app_id="^does-not-match$"] nop arbitrary comment text"#,
    ));
    let with_nop = mapped_leaf(Some(
        r#"for_window [app_id="^nop-target$"] nop arbitrary comment text"#,
    ));
    assert_eq!(with_nop, baseline);
}


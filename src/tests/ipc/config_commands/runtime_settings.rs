fn add_two_tiled_windows(fixture: &mut Fixture) {
    let client = fixture.add_client();
    for app_id in ["left", "right"] {
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}

fn tiled_window_rects(fixture: &mut Fixture) -> Vec<Value> {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn collect(value: &Value, rects: &mut Vec<Value>) {
        if value["type"] == "con" && value["app_id"].is_string() {
            rects.push(value["rect"].clone());
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    collect(child, rects);
                }
            }
        }
    }
    let mut rects = Vec::new();
    collect(&tree, &mut rects);
    rects.sort_by_key(|rect| rect["x"].as_i64().unwrap());
    rects
}

/// Tiled window rects on one named workspace.
///
/// [`tiled_window_rects`] flattens every workspace, so a test that creates a
/// second workspace cannot use it: the two sets interleave under the sort.
fn tiled_window_rects_on(fixture: &mut Fixture, workspace: &str) -> Vec<Value> {
    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    fn collect(value: &Value, rects: &mut Vec<Value>) {
        if value["type"] == "con" && value["app_id"].is_string() {
            rects.push(value["rect"].clone());
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    collect(child, rects);
                }
            }
        }
    }
    fn find_ws<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        if value["type"] == "workspace" && value["name"] == name {
            return Some(value);
        }
        for key in ["nodes", "floating_nodes"] {
            if let Some(children) = value[key].as_array() {
                for child in children {
                    if let Some(found) = find_ws(child, name) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
    let ws = find_ws(&tree, workspace).expect("workspace not in tree");
    let mut rects = Vec::new();
    collect(ws, &mut rects);
    rects.sort_by_key(|rect| rect["x"].as_i64().unwrap());
    rects
}

#[test]
fn reloaded_gap_defaults_do_not_change_an_existing_workspace() {
    static NEXT_CONFIG: AtomicU64 = AtomicU64::new(0);

    let initial = swayward_config::Config::parse_mem(
        r#"layout {
            gaps 10
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(initial);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);
    assert_eq!(before[0]["y"], 8);
    assert_eq!(before[1]["y"], 8);

    let path = std::env::temp_dir().join(format!(
        "swayward-gap-reload-test-{}-{}.kdl",
        std::process::id(),
        NEXT_CONFIG.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(
        &path,
        r#"layout {
            gaps 16
            outer-gaps { left -2; right -2; top -2; bottom -2; }
            border { off; }
        }"#,
    )
    .unwrap();
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
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::from_str::<Value>(sway_fixture!("events/workspace.reload.json")).unwrap()
    );
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 16.);
    assert_eq!(tiled_window_rects(&mut fixture), before);

    std::fs::remove_file(path).unwrap();
}

#[test]
fn runtime_gaps_all_changes_existing_workspaces() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.outer_gaps_configured = true;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner all set 16")[0].success);
    let after = tiled_window_rects(&mut fixture);
    assert_ne!(after, before);
    assert_eq!(after[0]["x"], 14);
    assert_eq!(after[0]["y"], 14);
    assert_eq!(after[1]["y"], 14);
}

/// Sway's two-argument form sets the default for workspaces created later and
/// leaves existing ones alone (`sway/sway/commands/gaps.c:48-91`), because a
/// live workspace reads its own gaps, not the global default
/// (`sway/sway/tree/workspace.c:224-225`).
#[test]
fn gaps_defaults_form_does_not_disturb_an_existing_workspace() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.outer_gaps = swayward_config::layout::OuterGaps::all(-2.);
    config.layout.outer_gaps_configured = true;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);
    assert_eq!(before[0]["x"], 8);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 40")[0].success);

    // The default moved...
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 40.);
    // ...and the existing workspace did not.
    assert_eq!(tiled_window_rects(&mut fixture), before);
}

/// The converse, and the other half of the "must not fight" requirement: the
/// runtime form mutates the live workspace and must leave the default alone, so
/// a workspace created afterwards still inherits the configured default.
#[test]
fn runtime_gaps_form_does_not_overwrite_the_defaults() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 30")[0].success);

    // The live workspace moved.
    let after = tiled_window_rects(&mut fixture);
    assert_eq!(after[0]["x"], 30);
    // The default did not, so a later workspace still gets 10.
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 10.);

    // Prove it by creating one and reading its gaps back.
    assert!(crate::command::execute(fixture.niri_state(), "workspace fresh")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "fresh")[0]["x"], 10);
}

/// Both directions in one session: setting the default, then a runtime change,
/// then another default write, must not let either clobber the other.
#[test]
fn gaps_defaults_and_runtime_forms_hold_separate_state() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);

    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 25")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner current set 5")[0].success);

    // Runtime change won for the live workspace; the default is still 25.
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 25.);

    // A further default write still does not touch the live workspace.
    assert!(crate::command::execute(fixture.niri_state(), "gaps inner 50")[0].success);
    assert_eq!(tiled_window_rects(&mut fixture)[0]["x"], 5);
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 50.);
}

/// `workspace <name> gaps <kind> <px>` is a per-workspace-name default applied
/// at creation (`sway/sway/tree/workspace.c:226-243`), so it must affect a
/// workspace of that name created afterwards and not the current one.
#[test]
fn workspace_gaps_apply_to_a_later_workspace_of_that_name() {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 10.;
    config.layout.border.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    add_two_tiled_windows(&mut fixture);
    let before = tiled_window_rects(&mut fixture);

    assert!(
        crate::command::execute(fixture.niri_state(), "workspace roomy gaps inner 45")[0].success
    );

    // The current workspace is untouched, as in sway.
    assert_eq!(tiled_window_rects(&mut fixture), before);
    // The global default is untouched too: this is per-name state.
    assert_eq!(fixture.swayward().config.borrow().layout.gaps, 10.);

    // A workspace with that name picks the value up.
    assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "roomy")[0]["x"], 45);

    // A differently named workspace still gets the global default.
    assert!(crate::command::execute(fixture.niri_state(), "workspace plain")[0].success);
    add_two_tiled_windows(&mut fixture);
    assert_eq!(tiled_window_rects_on(&mut fixture, "plain")[0]["x"], 10);
}


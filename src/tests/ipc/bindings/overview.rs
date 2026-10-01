/// Waybar tracks the focused workspace from the `workspace` event stream, not
/// by polling GET_WORKSPACES. Jumping to a workspace from the overview changes
/// the active workspace through `toggle_overview_to_workspace`, which is not a
/// command dispatch, so nothing on that path told the event stream anything
/// had happened and every bar kept highlighting the workspace the user left.
#[test]
fn overview_workspace_jump_emits_a_workspace_focus_event() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture_with_config(config);
    fixture.add_output(1, (1280, 720));

    for command in ["workspace 2", "workspace 1"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((msg_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let before = active_idx(&mut fixture);

    // Exactly what a click on another workspace in the overview does.
    assert!(fixture.swayward().layout.open_overview());
    fixture
        .swayward()
        .layout
        .toggle_overview_to_workspace(before + 1);
    fixture.niri_state().refresh_and_flush_clients();

    let after = active_idx(&mut fixture);
    assert_eq!(
        after,
        before + 1,
        "the overview jump must change the active workspace"
    );

    let ((event_type, payload), _) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, EVENT_WORKSPACE, "expected a workspace event");
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(
        event["change"], "focus",
        "a bar learns the workspace changed only from this event: {event}"
    );
}

/// Waybar answers a workspace event by immediately re-reading GET_TREE and
/// rendering whatever that reply says
/// (Waybar/src/modules/sway/workspaces.cpp:107-113,146-172). So the tree the
/// server is holding at the moment it emits the event is the tree the bar
/// draws.
///
/// ipc_refresh_layout emits from ipc_refresh_workspaces first and only then
/// calls refresh_query_state, so for that window query_state.tree still
/// describes the workspace the user left.
#[test]
fn query_state_tree_is_current_when_a_workspace_event_is_emitted() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture_with_config(config);
    fixture.add_output(1, (1280, 720));
    for command in ["workspace 2", "workspace 1"] {
        crate::command::execute(fixture.niri_state(), command);
    }
    fixture.niri_state().refresh_and_flush_clients();

    // Subscribe first: this client is the bar, and it must not see a stale
    // tree after being told the workspace changed.
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let before = fixture
        .swayward()
        .layout
        .monitor_for_output(&output)
        .unwrap()
        .active_workspace_idx();

    assert!(fixture.swayward().layout.open_overview());
    fixture
        .swayward()
        .layout
        .toggle_overview_to_workspace(before + 1);

    fixture.niri_state().ipc_refresh_layout();

    // Wait for the event, then query exactly as waybar does on receiving it.
    let ((_, payload), _) = read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "focus", "expected a focus event: {event}");

    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut workspaces);
    let focused = workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "the cached GET_TREE served to a bar that just saw the event still \
         names the old workspace: {tree:#}"
    );
}

/// Arrows inside the overview change the active workspace while the overview
/// is still open. A bar must track that immediately, not only once the
/// overview closes.
#[test]
fn overview_arrow_emits_focus_event_before_the_overview_closes() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture_with_config(config);
    fixture.add_output(1, (1280, 720));
    for command in ["workspace 2", "workspace 1"] {
        crate::command::execute(fixture.niri_state(), command);
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    key_event(&mut fixture, 116, true);
    key_event(&mut fixture, 116, false);
    fixture.niri_state().refresh_and_flush_clients();

    let mut changes = Vec::new();
    while let Some(((_, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        changes.push((
            event["change"].as_str().unwrap_or_default().to_owned(),
            event["current"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    assert!(
        changes
            .iter()
            .any(|(change, name)| change == "focus" && name == "2"),
        "the bar must learn about the new workspace while the overview is \
         still open, got {changes:?}"
    );

    // And GET_TREE, which is what waybar actually renders, must agree.
    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut workspaces);
    let focused = workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "GET_TREE must mark the arrowed-to workspace focused while the \
         overview is open: {tree:#}"
    );
}

/// The keyboard route into the same jump: open the overview, arrow to another
/// workspace, then Escape to leave. A bar must end up highlighting the
/// workspace the user landed on.
#[test]
fn overview_arrow_then_escape_emits_workspace_focus_events() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture_with_config(config);
    fixture.add_output(1, (1280, 720));

    for command in ["workspace 2", "workspace 1"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let before = active_idx(&mut fixture);

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();

    // Down arrow, then Escape to close the overview.
    for key in [116, 1] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
    }
    fixture.niri_state().refresh_and_flush_clients();

    assert_eq!(
        active_idx(&mut fixture),
        before + 1,
        "arrow then escape must leave the new workspace active"
    );

    let mut changes = Vec::new();
    while let Some(((event_type, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        assert_eq!(event_type, EVENT_WORKSPACE);
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        changes.push((
            event["change"].as_str().unwrap_or_default().to_owned(),
            event["current"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    assert!(
        changes
            .iter()
            .any(|(change, name)| change == "focus" && name == "2"),
        "a bar must be told workspace 2 is focused, got {changes:?}"
    );

    // Waybar ignores the event payload entirely and re-reads GET_TREE, then
    // reads `focused` and `visible` off the workspace nodes
    // (Waybar/src/modules/sway/workspaces.cpp:88-113,146-172,307-321,363-371).
    // The event is only the trigger; GET_TREE is what the bar renders.
    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut tree_workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut tree_workspaces);
    let tree_focused = tree_workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        tree_focused,
        vec!["2".to_string()],
        "GET_TREE is what waybar renders, and it must mark the new workspace \
         focused: {tree:#}"
    );

    let mut query = UnixStream::connect(&socket).unwrap();
    let workspaces = query_ipc(&mut fixture, &mut query, MessageType::GetWorkspaces);
    let focused = workspaces
        .as_array()
        .unwrap()
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "GET_WORKSPACES must agree with the focus event: {workspaces}"
    );
    let visible = workspaces
        .as_array()
        .unwrap()
        .iter()
        .filter(|ws| ws["visible"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        vec!["2".to_string()],
        "the workspace the user landed on must be the visible one: {workspaces}"
    );
}

#[test]
fn overview_arrow_keys_move_between_workspaces() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 2", "workspace 1", "split vertical"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    for _ in 0..2 {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let first_workspace = active_workspace_idx(&mut fixture);
    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    assert!(
        fixture.swayward().keyboard_focus.is_overview(),
        "overview opened with keyboard focus {:?}",
        fixture.swayward().keyboard_focus
    );

    for (key, expected_workspace) in [(116, first_workspace + 1), (111, first_workspace)] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
        assert_eq!(
            active_workspace_idx(&mut fixture),
            expected_workspace,
            "keycode {key} did not focus workspace index {expected_workspace}"
        );
    }
}

#[test]
fn overview_arrow_keys_wrap_at_the_ends() {
    // The overview shows the whole stack at once, so an arrow that stops dead
    // at the last workspace reads as a broken key rather than as an edge. With
    // only two workspaces one of the two arrows always looked dead, which is
    // how this was reported. Sway's own `workspace next` wraps.
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 1", "workspace 2"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    // The monitor also holds a trailing unnamed workspace, so the wrap target
    // is the last index rather than the last *named* one. Drive to index 0
    // first so the wrap is unambiguous.
    crate::command::execute(fixture.niri_state(), "workspace 1");
    // Let the workspace-switch animation finish: the wrapping helpers defer to
    // the plain clamped ones while a switch is in flight.
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.swayward().layout.advance_animations();
    fixture.swayward().clock.set_complete_instantly(false);
    assert_eq!(active_workspace_idx(&mut fixture), 0);
    let last = fixture.swayward().layout.workspaces().count() - 1;

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();

    // Up from the first workspace wraps to the last, and Down from the last
    // wraps back to the first.
    for (key, expected) in [(111, last), (116, 0)] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
        // Settle the switch animation: a wrap issued mid-switch falls back to
        // the clamped helper and would test the wrong thing.
        fixture.swayward().clock.set_complete_instantly(true);
        fixture.swayward().layout.advance_animations();
        fixture.swayward().clock.set_complete_instantly(false);
        assert_eq!(
            active_workspace_idx(&mut fixture),
            expected,
            "keycode {key} did not wrap to workspace index {expected}"
        );
    }
}

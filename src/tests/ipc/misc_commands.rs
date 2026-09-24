#[test]
fn get_tree_hides_windows_on_background_workspaces() {
    // Sway reports `visible` per window, not per workspace: the captured
    // tests/fixtures/sway/two_workspaces.tree.json has visible:false on the
    // window sitting on the background workspace. Waybar's hasFlag recurses
    // into child nodes, so a window that always claims visibility lights up
    // every workspace button on the bar.
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));

    let client = fixture.add_client();
    for command in ["workspace 1", "workspace 2"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let mut stream = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);

    fn windows(node: &Value, workspace: Option<&str>, out: &mut Vec<(String, bool)>) {
        let workspace = if node["type"] == "workspace" {
            node["name"].as_str()
        } else {
            workspace
        };
        if node["type"] == "con" && node["nodes"].as_array().is_none_or(|n| n.is_empty()) {
            out.push((workspace.unwrap_or("?").to_owned(), node["visible"] == true));
        }
        for key in ["nodes", "floating_nodes"] {
            for child in node[key].as_array().into_iter().flatten() {
                windows(child, workspace, out);
            }
        }
    }

    let mut found = Vec::new();
    windows(&tree, None, &mut found);
    found.sort();
    assert_eq!(
        found,
        [("1".to_owned(), false), ("2".to_owned(), true)],
        "only the active workspace's window is visible"
    );
}

#[test]
fn overview_keys_work_with_num_lock_on() {
    // Num Lock is a state, not a chord. hardcoded_overview_bind used to
    // require the modifier set to be completely empty, so a keyboard with Num
    // Lock on -- which `input { keyboard { numlock } }` makes the default --
    // rejected every overview key while the mouse still worked.
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { numlock; }; }
workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 1", "workspace 2"] {
        assert!(crate::command::execute(fixture.niri_state(), command)[0].success);
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

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    assert!(fixture.swayward().keyboard_focus.is_overview());

    // Assert directly on the predicate: the harness does not latch Num Lock
    // from a keycode, and going through key_event would silently test the
    // unlocked path instead.
    let locked = smithay::input::keyboard::ModifiersState {
        num_lock: true,
        ..Default::default()
    };
    assert!(
        crate::input::hardcoded_overview_bind(smithay::input::keyboard::Keysym::Up, locked)
            .is_some(),
        "a bare Up was rejected while Num Lock was on"
    );

    let before = active_workspace_idx(&mut fixture);
    key_event(&mut fixture, 111, true);
    key_event(&mut fixture, 111, false);
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.swayward().layout.advance_animations();
    fixture.swayward().clock.set_complete_instantly(false);

    assert_ne!(
        active_workspace_idx(&mut fixture),
        before,
        "an overview arrow was rejected while Num Lock was on"
    );
}

#[test]
fn every_message_type_replies_and_leaves_the_connection_usable() {
    // AGENTS.md: every SWAYSOCK reply is sway-shaped or a structured failure,
    // and it never hangs. docs/IPC_ORACLE_COVERAGE.md recorded that this was
    // asserted but not enumerated, so nothing proved it for the numbers a
    // buggy or future client actually sends.
    //
    // Sway's own range is 0..=12 plus 100 and 101 (sway/include/ipc.h). 11 is
    // IPC_SYNC, which sway declines with {"success": false} rather than
    // closing the socket (sway/sway/ipc-server.c:919-925).
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    fixture.niri_state().ipc_refresh_layout();
    let mut stream = UnixStream::connect(&socket).unwrap();

    let types: Vec<u32> = (0..=13)
        .chain([99, 100, 101, 102, 1000, u32::MAX])
        .collect();
    for raw_type in types {
        // SUBSCRIBE needs a JSON array; anything else would be a parse failure
        // rather than a test of the type dispatch.
        let payload = if raw_type == 2 { "[]" } else { "" };
        stream
            .write_all(&swayward_ipc::wire::encode_raw(raw_type, payload))
            .unwrap();
        let (reply_type, reply) = read_ipc_reply(&mut fixture, &mut stream);

        assert_eq!(
            reply_type, raw_type,
            "reply must echo the request type for {raw_type}"
        );
        let value: Value = serde_json::from_str(&reply)
            .unwrap_or_else(|e| panic!("type {raw_type} returned invalid JSON: {reply}: {e}"));

        // Either a sway-shaped payload or a structured failure. Never a bare
        // string, never empty, never a silent drop.
        assert!(
            value.is_object() || value.is_array(),
            "type {raw_type} must reply with an object or array, got {reply}"
        );
        if raw_type == 11 {
            assert_eq!(
                value,
                serde_json::json!({"success": false}),
                "IPC_SYNC must match sway's decline exactly"
            );
        }
        if !matches!(raw_type, 0..=10 | 12 | 100 | 101) {
            assert_eq!(
                value["success"], false,
                "unsupported type {raw_type} must report failure, got {reply}"
            );
        }
    }

    // The connection survived every unsupported type and still serves a real
    // request. A client that gets one bad reply and a dead socket is worse off
    // than one that gets an error.
    let version = query_ipc(&mut fixture, &mut stream, MessageType::GetVersion);
    assert_eq!(version["variant"], "swayward");
}

fn nested_split_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    for _ in 0..3 {
        let window = fixture.client(client).create_window();
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
    assert!(fixture
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .focused_container_node()
        .is_some());
    fixture
}

fn command_tree(fixture: &mut Fixture) -> Value {
    let swayward = fixture.swayward();
    serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap()
}

#[test]
fn floating_group_commands_fail_without_changing_state() {
    // Disabling floating on an already tiled split is an exact no-op and does
    // not need the missing model.
    let mut fixture = nested_split_fixture();
    let before = command_tree(&mut fixture);
    let outcome = crate::command::execute(fixture.niri_state(), "floating disable");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(command_tree(&mut fixture), before);

    // Sway applies these commands to the selected split container, and
    // floating/move-scratchpad create or preserve one floating subtree
    // (`commands/floating.c:23-55`, `commands/move.c:925-946`, and
    // `commands/sticky.c:20-42`). Swayward has no
    // floating-subtree representation, so it must refuse instead of reporting
    // success after acting on only the focused leaf.
    for command in [
        "floating enable",
        "floating toggle",
        "move scratchpad",
        "sticky enable",
    ] {
        let mut fixture = nested_split_fixture();
        let before_tree = command_tree(&mut fixture);
        let before_scratchpad = fixture.swayward().layout.scratchpad_windows().count();

        let outcomes = crate::command::execute(fixture.niri_state(), command);

        assert_eq!(outcomes.len(), 1, "{command}: {outcomes:?}");
        assert!(!outcomes[0].success, "{command}: {outcomes:?}");
        assert_eq!(
            outcomes[0].error.as_deref(),
            Some("floating container groups are not supported"),
            "{command}"
        );
        assert_eq!(
            command_tree(&mut fixture),
            before_tree,
            "{command} changed tree geometry, floating state or sticky state"
        );
        assert_eq!(
            fixture.swayward().layout.scratchpad_windows().count(),
            before_scratchpad,
            "{command} changed scratchpad membership"
        );
    }
}

#[test]
fn criteria_targeted_floating_group_commands_fail_without_changing_state() {
    for command in [
        "floating enable",
        "floating toggle",
        "move scratchpad",
        "sticky enable",
    ] {
        let mut fixture = nested_split_fixture();
        assert!(crate::command::execute(fixture.niri_state(), "mark floating-group")[0].success);
        assert!(crate::command::execute(fixture.niri_state(), "focus child")[0].success);
        let before_tree = command_tree(&mut fixture);
        let before_scratchpad = fixture.swayward().layout.scratchpad_windows().count();
        let command = format!(r#"[con_mark="floating-group"] {command}"#);

        let outcomes = crate::command::execute(fixture.niri_state(), &command);

        assert_eq!(outcomes.len(), 1, "{command}: {outcomes:?}");
        assert!(!outcomes[0].success, "{command}: {outcomes:?}");
        assert_eq!(
            outcomes[0].error.as_deref(),
            Some("floating container groups are not supported"),
            "{command}"
        );
        assert_eq!(
            command_tree(&mut fixture),
            before_tree,
            "{command} changed tree geometry, floating state, sticky state or focus"
        );
        assert_eq!(
            fixture.swayward().layout.scratchpad_windows().count(),
            before_scratchpad,
            "{command} changed scratchpad membership"
        );
    }
}

#[test]
fn floating_accepts_sways_boolean_vocabulary() {
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

    for (value, expected) in [
        ("1", true),
        ("yes", true),
        ("on", true),
        ("true", true),
        ("enable", true),
        ("enabled", true),
        ("active", true),
        ("no", false),
        ("off", false),
        ("false", false),
        ("disable", false),
        ("disabled", false),
        ("inactive", false),
        ("arbitrary", false),
    ] {
        let command = format!("floating {value}");
        let outcome = crate::command::execute(f.niri_state(), &command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let window = f.swayward().layout.focus().unwrap().window.clone();
        assert_eq!(
            f.swayward()
                .layout
                .workspaces()
                .any(|(_, _, workspace)| workspace.is_floating(&window)),
            expected,
            "{command}"
        );
    }
    for expected in [true, false] {
        let outcome = crate::command::execute(f.niri_state(), "floating toggle");
        assert!(outcome[0].success, "{outcome:?}");
        let window = f.swayward().layout.focus().unwrap().window.clone();
        assert_eq!(
            f.swayward()
                .layout
                .workspaces()
                .any(|(_, _, workspace)| workspace.is_floating(&window)),
            expected
        );
    }

    for (command, expected) in [
        (
            "floating",
            "Invalid floating command (expected 1 argument, got 0)",
        ),
        (
            "floating yes now",
            "Invalid floating command (expected 1 argument, got 2)",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true));
    }
}

#[test]
fn floating_sizes_accept_i32_values_and_reject_malformed_values() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));

    for (command, minimum, expected) in [
        ("floating_minimum_size -10 x +20", true, (-10, 20)),
        (
            "floating_maximum_size 2147483647 x -2147483648",
            false,
            (i32::MAX, i32::MIN),
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let layout = &f.swayward().config.borrow().layout;
        let size = if minimum {
            layout.floating_minimum_size
        } else {
            layout.floating_maximum_size
        };
        assert_eq!((size.width, size.height), expected, "{command}");
    }

    for (command, expected) in [
        (
            "floating_minimum_size 10px x 20",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 2147483648 x 20",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 10 by 20",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 10 x 20 extra",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size wide x 20",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size 10 x -2147483649",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size 10 X 20",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
        (
            "floating_maximum_size 10 x",
            "Expected 'floating_maximum_size <width> x <height>'",
        ),
    ] {
        let before = f.swayward().config.borrow().layout.clone();
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
        assert_eq!(f.swayward().config.borrow().layout, before, "{command}");
    }
}

#[test]
fn layout_settings_apply_at_runtime_like_sway() {
    use swayward_config::layout::{
        DefaultOrientation, FocusWrapping, HideEdgeBorders, SmartBorders, WorkspaceLayout,
    };

    // Sway serves its config file and its IPC from one command table
    // (`sway/sway/commands.c:162-173`), so these directives are live commands
    // there. swayward keeps the setting in KDL; this asserts the command
    // reaches the same state, rather than merely returning success.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let layout = |f: &mut Fixture| f.swayward().config.borrow().layout.clone();
    let before = layout(&mut f);
    assert_ne!(before.workspace_layout, WorkspaceLayout::Tabbed);

    for (command, check) in [
        (
            "workspace_layout tabbed",
            &(|l: &swayward_config::Layout| l.workspace_layout == WorkspaceLayout::Tabbed)
                as &dyn Fn(&swayward_config::Layout) -> bool,
        ),
        ("focus_wrapping no", &|l| {
            l.focus_wrapping == FocusWrapping::No
        }),
        ("focus_wrapping force", &|l| {
            l.focus_wrapping == FocusWrapping::Force
        }),
        // Deprecated in sway, kept as a boolean alias selecting between
        // force and yes (`sway/sway/commands/force_focus_wrapping.c`).
        ("force_focus_wrapping no", &|l| {
            l.focus_wrapping == FocusWrapping::Yes
        }),
        ("force_focus_wrapping yes", &|l| {
            l.focus_wrapping == FocusWrapping::Force
        }),
        ("hide_edge_borders both", &|l| {
            l.hide_edge_borders == HideEdgeBorders::Both
        }),
        // sway folds smart and smart_no_gaps into the smart-border toggle
        // rather than treating them as edge-border values.
        ("hide_edge_borders smart", &|l| {
            l.smart_borders == SmartBorders::On
        }),
        ("smart_borders no_gaps", &|l| {
            l.smart_borders == SmartBorders::NoGaps
        }),
        ("default_orientation vertical", &|l| {
            l.default_orientation == DefaultOrientation::Vertical
        }),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command} failed: {outcome:?}");
        assert!(
            check(&layout(&mut f)),
            "{command} did not change the config"
        );
    }

    // Values sway rejects must fail here too, with sway's message.
    for (command, expected) in [
        (
            "workspace_layout sideways",
            "Expected 'workspace_layout <default|stacking|tabbed>'",
        ),
        (
            "focus_follows_mouse maybe",
            "Expected 'focus_follows_mouse no|yes|always'",
        ),
        (
            "default_orientation diagonal",
            "Expected 'orientation <horizontal|vertical|auto>'",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
    }

    assert!(crate::command::execute(f.niri_state(), "show_marks no")[0].success);
    assert!(!layout(&mut f).titlebar.show_marks);
    assert!(crate::command::execute(f.niri_state(), "title_align right")[0].success);
    assert_eq!(
        layout(&mut f).titlebar.alignment,
        swayward_config::TitleAlignment::Right
    );
    assert!(crate::command::execute(f.niri_state(), "smart_gaps inverse_outer")[0].success);
    assert_eq!(
        layout(&mut f).smart_gaps,
        swayward_config::SmartGaps::InverseOuter
    );
    assert!(crate::command::execute(f.niri_state(), "smart_gaps toggle")[0].success);
    assert_eq!(layout(&mut f).smart_gaps, swayward_config::SmartGaps::Off);

    assert!(crate::command::execute(f.niri_state(), "tiling_drag no")[0].success);
    assert!(!f.swayward().config.borrow().input.tiling_drag);
    assert!(crate::command::execute(f.niri_state(), "tiling_drag toggle")[0].success);
    assert!(f.swayward().config.borrow().input.tiling_drag);
    assert!(crate::command::execute(f.niri_state(), "tiling_drag_threshold 17")[0].success);
    assert_eq!(f.swayward().config.borrow().input.tiling_drag_threshold, 17);
    assert!(crate::command::execute(f.niri_state(), "force_display_urgency_hint 700ms")[0].success);
    assert_eq!(f.swayward().config.borrow().urgent_timeout_ms, 700);

    // The primary-selection manager is created at launch. Reasserting its
    // current value succeeds; changing it must fail instead of claiming a
    // live change that cannot affect the manager (`sway/server.c:783-785`).
    assert!(crate::command::execute(f.niri_state(), "primary_selection enabled")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "primary_selection disabled");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("primary_selection can only be enabled/disabled at launch")
    );
    assert!(!f.swayward().config.borrow().clipboard.disable_primary);

    assert!(crate::command::execute(f.niri_state(), "focus_on_window_activation none")[0].success);
    assert_eq!(
        f.swayward().config.borrow().focus_on_window_activation,
        swayward_config::FocusOnWindowActivation::None
    );

    // focus_follows_mouse and workspace_auto_back_and_forth live under input.
    // Sway keeps three distinct states, and `always` is not `yes`
    // (`sway/include/sway/config.h:458-462`), so assert the stored mode and
    // not merely that the setting is enabled.
    use swayward_config::input::FocusFollowsMouseMode;
    let ffm = |f: &mut Fixture| f.swayward().config.borrow().input.focus_follows_mouse;
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse yes")[0].success);
    assert_eq!(
        ffm(&mut f).map(|v| v.mode),
        Some(FocusFollowsMouseMode::Yes)
    );
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse always")[0].success);
    assert_eq!(
        ffm(&mut f).map(|v| v.mode),
        Some(FocusFollowsMouseMode::Always),
        "`always` must be stored distinctly from `yes`"
    );
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse no")[0].success);
    assert_eq!(
        ffm(&mut f),
        None,
        "sway's FOLLOWS_NO is the field's absence"
    );
    // Sway compares these three with strcmp, so they are case-sensitive
    // (`sway/sway/commands/focus_follows_mouse.c:9-18`).
    let outcome = crate::command::execute(f.niri_state(), "focus_follows_mouse ALWAYS");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'focus_follows_mouse no|yes|always'")
    );

    assert!(
        crate::command::execute(f.niri_state(), "workspace_auto_back_and_forth yes")[0].success
    );
    assert!(
        f.swayward()
            .config
            .borrow()
            .input
            .workspace_auto_back_and_forth
    );

    // sway parses these with strtol and requires a literal x between two
    // integers, rejecting a trailing suffix because it checks the remainder
    // (`sway/sway/commands/floating_minmax_size.c`).
    assert!(crate::command::execute(f.niri_state(), "floating_minimum_size 100 x 50")[0].success);
    assert_eq!(layout(&mut f).floating_minimum_size.width, 100);
    assert_eq!(layout(&mut f).floating_minimum_size.height, 50);
    assert!(crate::command::execute(f.niri_state(), "floating_maximum_size 800 x 600")[0].success);
    assert_eq!(layout(&mut f).floating_maximum_size.width, 800);
    assert_eq!(layout(&mut f).floating_maximum_size.height, 600);
    for bad in [
        "floating_minimum_size 100 50",
        "floating_minimum_size 100 x 50px",
        "floating_minimum_size 100 by 50",
    ] {
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Expected 'floating_minimum_size <width> x <height>'")
        );
    }

    // `sway/sway/commands/font.c` strips a leading pango: prefix and joins the
    // remaining words, so the family and size survive as one string.
    assert!(crate::command::execute(f.niri_state(), "font pango:monospace 11")[0].success);
    assert_eq!(layout(&mut f).titlebar.font, "monospace 11");
    assert!(layout(&mut f).titlebar.pango_markup);
    assert!(f.swayward().layout.options().layout.titlebar.pango_markup);
    assert!(crate::command::execute(f.niri_state(), "font Sans Bold 9")[0].success);
    assert_eq!(layout(&mut f).titlebar.font, "Sans Bold 9");
    assert!(!layout(&mut f).titlebar.pango_markup);
    assert!(!f.swayward().layout.options().layout.titlebar.pango_markup);
    for (command, expected) in [
        ("font 11", "Invalid font family."),
        ("font monospace", "Invalid font size."),
    ] {
        let before = layout(&mut f).titlebar.clone();
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
        assert_eq!(layout(&mut f).titlebar, before);
    }

    // One value sets both axes; two set horizontal then vertical. Sway requires
    // both padding axes to be at least the current border thickness, and a new
    // thickness may not exceed the current vertical padding.
    assert!(crate::command::execute(f.niri_state(), "titlebar_padding 7")[0].success);
    assert_eq!(layout(&mut f).titlebar.horizontal_padding, 7.);
    assert_eq!(layout(&mut f).titlebar.vertical_padding, 7.);
    assert!(crate::command::execute(f.niri_state(), "titlebar_border_thickness 6")[0].success);
    assert_eq!(layout(&mut f).titlebar.border_thickness, 6);
    for bad in [
        "titlebar_border_thickness 8",
        "titlebar_border_thickness -1",
        "titlebar_border_thickness wide",
        "titlebar_padding 5 7",
        "titlebar_padding 7 5",
        "titlebar_padding -1",
        "titlebar_padding wide",
    ] {
        let before = layout(&mut f).titlebar.clone();
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some("Invalid size specified"));
        assert_eq!(layout(&mut f).titlebar, before);
    }
    assert!(crate::command::execute(f.niri_state(), "titlebar_padding 9 6")[0].success);
    assert_eq!(layout(&mut f).titlebar.horizontal_padding, 9.);
    assert_eq!(layout(&mut f).titlebar.vertical_padding, 6.);

    // focused_tab_title has no effective indicator or child-border colours in
    // sway, so the titlebar ring makes that class complete. The other classes
    // still use those colours on window borders and remain fail-loud.
    assert!(
        crate::command::execute(
            f.niri_state(),
            "client.focused_tab_title #123456 #abcdef #fedcba #010203 #040506"
        )[0]
        .success
    );
    let focused_tab = layout(&mut f).titlebar.focused_tab_title;
    assert_eq!(
        focused_tab.border_color.to_array_unpremul(),
        [
            0x12 as f32 / 255.,
            0x34 as f32 / 255.,
            0x56 as f32 / 255.,
            1.
        ]
    );
    assert_eq!(
        focused_tab.background_color.to_array_unpremul(),
        [
            0xab as f32 / 255.,
            0xcd as f32 / 255.,
            0xef as f32 / 255.,
            1.
        ]
    );
    assert_eq!(
        focused_tab.text_color.to_array_unpremul(),
        [
            0xfe as f32 / 255.,
            0xdc as f32 / 255.,
            0xba as f32 / 255.,
            1.
        ]
    );

    let before = layout(&mut f).titlebar;
    for command in [
        "client.focused #102030 #405060 #708090",
        "client.focused_inactive #112233 #445566 #778899",
        "client.unfocused #203040 #506070 #8090a0",
        "client.urgent #304050 #607080 #90a0b0",
        "client.focused #010203 #11223380 #44556640 #778899 #aabbcc",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].parse_error, Some(true));
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("client colour commands are unsupported because sway window-border colours are not fully rendered")
        );
        assert_eq!(layout(&mut f).titlebar, before);
        assert_eq!(f.swayward().layout.options().layout.titlebar, before);
    }

    let before = layout(&mut f).titlebar.focused;
    for (command, expected) in [
        (
            "client.focused #000000 #111111",
            "Invalid client.focused command (expected at least 3 arguments, got 2)",
        ),
        (
            "client.focused #000000 #111111 #222222 #333333 #444444 #555555",
            "Invalid client.focused command (expected at most 5 arguments, got 6)",
        ),
        (
            "client.focused #000000 #111111 #222222 nope",
            "Invalid indicator color nope",
        ),
        (
            "client.focused #000000 #111111 #222222 #333333 nope",
            "Invalid child_border color nope",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
        assert_eq!(layout(&mut f).titlebar.focused, before);
    }

    // Xwayland's mode is fixed at launch. Sway accepts the command and
    // refuses only a change (`sway/sway/commands/xwayland.c:24-28`), so
    // asking for the value already in effect succeeds and flipping it fails
    // with sway's message. The default config enables it.
    // default_border governs the border a new tiled window gets; sway keeps
    // the previous width when the command omits one
    // (`sway/sway/commands/default_border.c:22-24`). new_window and new_float
    // are the older i3 spellings of the same two settings.
    use swayward_config::layout::SwayBorderStyle;
    assert!(crate::command::execute(f.niri_state(), "default_border pixel 3")[0].success);
    assert_eq!(layout(&mut f).default_border.style, SwayBorderStyle::Pixel);
    assert_eq!(layout(&mut f).default_border.width, Some(3));
    assert!(crate::command::execute(f.niri_state(), "default_border normal")[0].success);
    assert_eq!(layout(&mut f).default_border.style, SwayBorderStyle::Normal);
    assert_eq!(
        layout(&mut f).default_border.width,
        Some(3),
        "omitting the width must keep the previous one"
    );
    assert!(crate::command::execute(f.niri_state(), "new_float none")[0].success);
    assert_eq!(
        layout(&mut f).default_floating_border.style,
        SwayBorderStyle::None,
        "new_float is the deprecated spelling of default_floating_border"
    );
    for bad in ["default_border csd", "default_border pixel wide"] {
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Expected 'default_border <none|normal|pixel>' or 'default_border <normal|pixel> <px>'")
        );
    }

    // popup_during_fullscreen shares its accepted values and error string
    // with the KDL node, so IPC and the config file cannot drift.
    assert!(crate::command::execute(f.niri_state(), "popup_during_fullscreen ignore")[0].success);
    assert_eq!(
        f.swayward().config.borrow().popup_during_fullscreen,
        swayward_config::misc::PopupDuringFullscreen::Ignore
    );
    let outcome = crate::command::execute(f.niri_state(), "popup_during_fullscreen sometimes");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'popup_during_fullscreen smart|ignore|leave_fullscreen'")
    );

    // Sway spells the modifier Mod1..Mod5; swayward names them. The modifier
    // and the inverse bit are independent fields, and this is its own setting
    // rather than the compositor `mod_key`, which must not move
    // (`sway/include/sway/config.h:509-510`).
    use swayward_config::input::{FloatingModifier, ModKey};
    let floating = |f: &mut Fixture| f.swayward().config.borrow().input.floating_modifier;
    let mod_key_before = f.swayward().config.borrow().input.mod_key;
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: false
        })
    );
    assert_eq!(
        f.swayward().config.borrow().input.mod_key,
        mod_key_before,
        "floating_modifier must not move the compositor mod key"
    );
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Alt normal")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Alt,
            inverse: false
        })
    );
    // inverse is stored, not refused: it swaps the move and resize buttons.
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: true
        })
    );
    // `none` is a value, not a key name, and sway returns before reading the
    // second argument (`sway/sway/commands/floating_modifier.c:11-14`), so a
    // trailing word is ignored and the inverse bit resets.
    for command in ["floating_modifier none", "floating_modifier NONE inverse"] {
        assert!(
            crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success
        );
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command} failed"
        );
        assert_eq!(
            floating(&mut f),
            Some(FloatingModifier {
                modifier: ModKey::None,
                inverse: false
            }),
            "{command} must disable the drag rather than name a key"
        );
    }
    // Sway validates the modifier before the mode, so an invalid modifier
    // wins over an invalid trailing word.
    for (command, expected) in [
        ("floating_modifier Mod9", "Invalid modifier"),
        ("floating_modifier Mod9 sideways", "Invalid modifier"),
        (
            "floating_modifier Mod4 sideways",
            "Usage: floating_modifier <mod> [inverse|normal]",
        ),
        (
            "floating_modifier",
            "Invalid floating_modifier command (expected at least 1 argument, got 0)",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
    }

    // Sway keeps the three warping modes apart: `output` warps only across
    // outputs, `container` warps on every qualifying focus change
    // (`sway/sway/input/seat.c:1526-1547`). This is its own policy and must
    // not be folded into the inherited `warp-mouse-to-focus` centering mode.
    use swayward_config::input::MouseWarping;
    let warping = |f: &mut Fixture| f.swayward().config.borrow().input.mouse_warping;
    let warp_to_focus_before = f.swayward().config.borrow().input.warp_mouse_to_focus;
    assert!(crate::command::execute(f.niri_state(), "mouse_warping output")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Output);
    assert!(crate::command::execute(f.niri_state(), "mouse_warping container")[0].success);
    assert_eq!(
        warping(&mut f),
        MouseWarping::Container,
        "`container` must be stored distinctly from `output`"
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::No);
    assert_eq!(
        f.swayward().config.borrow().input.warp_mouse_to_focus,
        warp_to_focus_before,
        "mouse_warping must not overwrite the inherited centering option"
    );
    // strcasecmp, unlike focus_follows_mouse
    // (`sway/sway/commands/mouse_warping.c:9-16`).
    assert!(crate::command::execute(f.niri_state(), "mouse_warping CONTAINER")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Container);
    let outcome = crate::command::execute(f.niri_state(), "mouse_warping sideways");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'mouse_warping output|container|none'")
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);

    assert!(crate::command::execute(f.niri_state(), "xwayland enable")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "xwayland disable");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("xwayland can only be enabled/disabled at launch")
    );
}

/// A KDL `workspace "name" { layout { gaps N } }` must reach the workspace
/// however it comes to exist. Sway reads the workspace config inside
/// `workspace_create` (`sway/sway/tree/workspace.c:224-243`), so this holds for
/// a workspace created on demand, not only one created eagerly at startup.
///
/// Regression test: workspaces carrying an output assignment skip eager
/// creation (`src/swayward.rs:1549-1552`), and the lazy creation path used to
/// drop the per-name layout entirely.
#[test]
fn configured_workspace_layout_applies_however_the_workspace_is_created() {
    for assignment in ["", "sway-output-assignment \"fake-1\""] {
        let config = swayward_config::Config::parse_mem(&format!(
            r#"
layout {{
    gaps 10
    border {{ off; }}
}}
workspace "roomy" {{
    {assignment}
    layout {{ gaps 45; }}
}}
"#
        ))
        .unwrap();
        let mut fixture = Fixture::with_config(config);
        fixture.add_output(1, (1280, 800));
        assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
        add_two_tiled_windows(&mut fixture);
        assert_eq!(
            tiled_window_rects_on(&mut fixture, "roomy")[0]["x"],
            45,
            "assignment: {assignment:?}"
        );
    }
}

/// Workspace names in a stable order, for rename assertions.
fn workspace_names(fixture: &mut Fixture) -> Vec<String> {
    let swayward = fixture.swayward();
    let mut names = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .map(|workspace| workspace.name)
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Put one window with `app_id` on each named workspace, leaving the last
/// created workspace focused.
fn windows_on_workspaces(fixture: &mut Fixture, plan: &[(&str, &str)]) {
    let client = fixture.add_client();
    for (workspace, app_id) in plan {
        assert!(
            crate::command::execute(fixture.niri_state(), &format!("workspace {workspace}"))[0]
                .success
        );
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id((*app_id).into());
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}

/// Sway resolves `rename workspace to <new>` from the matched container's
/// workspace rather than from focus (`sway/sway/commands.c:181-202`;
/// `sway/sway/commands/rename.c:36-37`).
#[test]
fn criteria_rename_workspace_renames_the_matched_workspace_not_the_focused_one() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "target"), ("beta", "bystander")]);
    // Focus is on beta, but the criteria match is on alpha.
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("beta".to_owned())
    );

    let outcome = crate::command::execute(
        f.niri_state(),
        "[app_id=target] rename workspace to renamed",
    );
    assert!(outcome[0].success, "{outcome:?}");

    // alpha became renamed; beta, which had focus, is untouched.
    assert_eq!(
        workspace_names(&mut f),
        vec!["beta".to_owned(), "renamed".to_owned()]
    );
}

/// Sway runs the handler once per matched container
/// (`sway/sway/commands.c:305-323`), so two matches on ONE workspace rename it
/// once: the second pass finds the new name already taken by the same
/// workspace and returns success without renaming again
/// (`sway/sway/commands/rename.c:82-89`).
#[test]
fn criteria_rename_workspace_renames_one_workspace_once_for_two_matches() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "twin"), ("beta", "bystander")]);
    // Add a second matching window to alpha.
    assert!(crate::command::execute(f.niri_state(), "workspace alpha")[0].success);
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("twin".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let outcome = crate::command::execute(f.niri_state(), "[app_id=twin] rename workspace to once");
    assert!(outcome[0].success, "{outcome:?}");

    // Renamed exactly once. A second rename would have failed with
    // "Workspace already exists" or produced a stray name.
    assert_eq!(
        workspace_names(&mut f),
        vec!["beta".to_owned(), "once".to_owned()]
    );
}

/// Two matches on DIFFERENT workspaces cannot both take one name. Sway renames
/// the first, then fails the second with `Workspace already exists`, and a
/// CMD_INVALID aborts the remaining targets (`sway/sway/commands.c:316-321`).
#[test]
fn criteria_rename_workspace_fails_when_two_matched_workspaces_want_one_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "spread"), ("beta", "spread")]);

    let outcome =
        crate::command::execute(f.niri_state(), "[app_id=spread] rename workspace to clash");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Workspace already exists")
    );

    // The first match was renamed before the clash, as in sway: the loop is not
    // transactional.
    assert_eq!(
        workspace_names(&mut f),
        vec!["beta".to_owned(), "clash".to_owned()]
    );
}

/// Zero matches must be a structured failure, not a success no-op
/// (`sway/sway/commands.c:301-303`).
#[test]
fn criteria_rename_workspace_reports_no_matching_node_for_zero_matches() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "present")]);
    let before = workspace_names(&mut f);

    let outcome =
        crate::command::execute(f.niri_state(), "[app_id=absent] rename workspace to nope");
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(outcome[0].error.as_deref(), Some("No matching node."));
    assert_eq!(workspace_names(&mut f), before);
}

/// Sway resolves the `<old>` and `number <n>` forms by name even under a
/// criteria prefix; only the bare `to` form reads the matched container
/// (`sway/sway/commands/rename.c:35-58`).
#[test]
fn criteria_rename_workspace_with_an_explicit_old_name_ignores_the_match() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("alpha", "target"), ("beta", "bystander")]);

    let outcome = crate::command::execute(
        f.niri_state(),
        "[app_id=target] rename workspace beta to moved",
    );
    assert!(outcome[0].success, "{outcome:?}");

    // beta was renamed, even though the match was on alpha.
    assert_eq!(
        workspace_names(&mut f),
        vec!["alpha".to_owned(), "moved".to_owned()]
    );
}

/// The point of runtime `set`: a variable defined over IPC must change what a
/// LATER command does, not merely be stored. Sway substitutes at dispatch
/// (`sway/sway/commands.c:283-285`), so this is observable behaviour.
#[test]
fn runtime_set_variable_changes_a_subsequent_command() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $target chosen")[0].success);
    // The variable is only useful if it expands in the NEXT command.
    assert!(crate::command::execute(f.niri_state(), "workspace $target")[0].success);

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("chosen".to_owned())
    );
}

/// Sway's symbol table is global, not per-connection, so a variable set on one
/// IPC connection is visible on the next command from any source.
#[test]
fn runtime_set_variable_survives_across_ipc_connections() {
    let (mut f, socket) = ipc_fixture();
    f.add_output(1, (1920, 1080));

    let mut setter = UnixStream::connect(&socket).unwrap();
    setter
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "set $ws first",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut f, &mut setter);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([{"success": true}])
    );
    drop(setter);

    // A different socket sees the compositor-global symbol table.
    let mut user = UnixStream::connect(&socket).unwrap();
    user.write_all(&swayward_ipc::wire::encode(
        MessageType::RunCommand,
        "workspace $ws",
    ))
    .unwrap();
    let (_, payload) = read_ipc_reply(&mut f, &mut user);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([{"success": true}])
    );
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("first".to_owned())
    );

    // Redefining on that second socket is visible on its next request too.
    user.write_all(&swayward_ipc::wire::encode(
        MessageType::RunCommand,
        "set $ws second",
    ))
    .unwrap();
    let _ = read_ipc_reply(&mut f, &mut user);
    user.write_all(&swayward_ipc::wire::encode(
        MessageType::RunCommand,
        "workspace $ws",
    ))
    .unwrap();
    let _ = read_ipc_reply(&mut f, &mut user);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("second".to_owned())
    );
}

/// Sway sorts symbols longest name first on insert
/// (`sway/sway/commands/set.c:13-15`), so a longer name is never shadowed by a
/// shorter one that prefixes it.
#[test]
fn runtime_set_prefers_the_longest_matching_variable_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    // Define the SHORT name first, so insertion order alone would mismatch.
    assert!(crate::command::execute(f.niri_state(), "set $ws short")[0].success);
    assert!(crate::command::execute(f.niri_state(), "set $ws2 long")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $ws2")[0].success);

    // Wrong answer here would be "short2".
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("long".to_owned())
    );
}

/// Sway exempts the name being defined from substitution, starting at argv[2]
/// for `set` (`sway/sway/commands.c:283`), so `set $a $b` assigns the VALUE of
/// `$b` to `$a` rather than expanding `$a` on the left.
#[test]
fn runtime_set_expands_the_value_but_not_the_name() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $source resolved")[0].success);
    assert!(crate::command::execute(f.niri_state(), "set $alias $source")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $alias")[0].success);

    // $alias holds "resolved", and the name $alias was not itself expanded.
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("resolved".to_owned())
    );
}

/// An unknown variable is left verbatim rather than becoming empty
/// (`sway/sway/config.c:935-937`).
#[test]
fn runtime_set_leaves_an_unknown_variable_verbatim() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $known value")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $unknown")[0].success);

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("$unknown".to_owned())
    );
}

/// Sway rejects a name without `$` and a command with too few arguments
/// (`sway/sway/commands/set.c:27-34`).
#[test]
fn runtime_set_rejects_sways_invalid_forms() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let bare = &crate::command::execute(f.niri_state(), "set novar value")[0];
    assert!(!bare.success);
    assert_eq!(
        bare.error.as_deref(),
        Some("variable 'novar' must start with $")
    );

    let short = &crate::command::execute(f.niri_state(), "set $onlyname")[0];
    assert!(!short.success);
    assert_eq!(
        short.error.as_deref(),
        Some("Invalid set command (expected at least 2 arguments, got 1)")
    );

    // Neither rejection may leave a variable behind.
    assert!(f.swayward().sway_variables.is_empty());
}

/// A key binding re-enters the command path at press time
/// (`sway/sway/commands/bind.c:635`), so a variable set at runtime expands for
/// a binding whose stored command still contains it.
#[test]
fn runtime_set_variable_expands_for_a_binding_at_press_time() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds {
    Mod+Shift+V { command "workspace $late"; }
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $late arrived")[0].success);
    let bound = f.swayward().config.borrow().binds.0[0].action.clone();
    let swayward_config::Action::SwayCommand(command) = bound else {
        panic!("expected a sway command binding");
    };
    // The binding still holds the unexpanded text; expansion happens on run.
    assert_eq!(command, "workspace $late");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("arrived".to_owned())
    );
}

/// Sway frees the symbol table on reload (`sway/sway/config.c:111-115`), so a
/// runtime variable does not survive one.
#[test]
fn runtime_set_variables_are_discarded_by_reload() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $gone value")[0].success);
    assert_eq!(f.swayward().sway_variables.len(), 1);

    f.niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(f.swayward().sway_variables.is_empty());

    // And the name no longer expands, so it is left verbatim.
    assert!(crate::command::execute(f.niri_state(), "workspace $gone")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("$gone".to_owned())
    );
}

/// Sway substitutes after command-list and argv splitting, so separators and
/// whitespace inside a variable value remain one argument rather than becoming
/// syntax (`sway/sway/commands.c:253-285`).
#[test]
fn runtime_set_value_cannot_inject_another_command_or_split_an_argument() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $ws a b")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace $ws")[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("a b".to_owned())
    );

    assert!(crate::command::execute(f.niri_state(), "set $literal \"semi;colon\"")[0].success);
    let outcome = crate::command::execute(f.niri_state(), "workspace $literal");
    assert_eq!(
        outcome.len(),
        1,
        "value became a second command: {outcome:?}"
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("semi;colon".to_owned())
    );
}

/// Sway executes an IPC command list in order and substitutes immediately
/// before each dispatch, so a `set` at the front affects a later command in the
/// same payload (`sway/sway/commands.c:230-334`).
#[test]
fn runtime_set_affects_a_later_command_in_the_same_payload() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let outcome = crate::command::execute(f.niri_state(), "set $ws inline; workspace $ws");
    assert_eq!(outcome.len(), 2);
    assert!(outcome.iter().all(|result| result.success), "{outcome:?}");
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("inline".to_owned())
    );
}

/// Criteria-targeted `set` is still refused rather than pretending that the
/// parser's global state matches sway's per-match sequential command loop.
#[test]
fn runtime_set_with_criteria_fails_without_changing_state() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("matched".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let before = f.swayward().sway_variables.clone();
    let outcome = crate::command::execute(f.niri_state(), "[app_id=matched] set $ws wrong");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("criteria targets are not implemented for this command yet")
    );
    assert_eq!(f.swayward().sway_variables, before);
}

/// Sway substitutes a bindsym line while loading the config and stores the
/// expanded command (`sway/sway/commands.c:403`; `sway/sway/commands/bind.c:488`),
/// so redefining the variable later cannot rewrite a binding that already
/// captured the old value.
#[test]
fn runtime_set_does_not_rewrite_a_binding_that_captured_the_old_value() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds {
    Mod+Shift+V { command "workspace old"; }
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    assert!(crate::command::execute(f.niri_state(), "set $ws new")[0].success);
    let bound = f.swayward().config.borrow().binds.0[0].action.clone();
    let swayward_config::Action::SwayCommand(command) = bound else {
        panic!("expected a sway command binding");
    };
    assert_eq!(command, "workspace old");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);
    assert_eq!(
        f.swayward().layout.active_workspace().unwrap().sway_name(),
        Some("old".to_owned())
    );
}

#[test]
fn get_tree_hides_windows_on_inactive_tabs_at_every_depth() {
    // Sway's view_is_visible walks up from a view and, at every tabbed or
    // stacked ancestor, requires the seat's active tiling child to be on its
    // path (sway/tree/view.c:1180-1193). Measured on headless sway 1.12 with
    // tabbed[A, tabbed[B, tabbed[C, D]]]: exactly the focused window is
    // visible, whichever depth it sits at. swayward reported all four.
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let client = fixture.add_client();
    let run = |fixture: &mut Fixture, command: &str| {
        assert!(
            crate::command::execute(fixture.niri_state(), command)[0].success,
            "{command}"
        );
    };

    map_test_window(&mut fixture, client, "m-A");
    run(&mut fixture, "layout tabbed");
    map_test_window(&mut fixture, client, "m-B");
    run(&mut fixture, "split v");
    run(&mut fixture, "layout tabbed");
    map_test_window(&mut fixture, client, "m-C");
    run(&mut fixture, "split v");
    run(&mut fixture, "layout tabbed");
    map_test_window(&mut fixture, client, "m-D");

    fn visible(node: &Value, out: &mut Vec<(String, bool)>) {
        if let Some(app) = node["app_id"].as_str().filter(|app| app.starts_with("m-")) {
            out.push((app.to_owned(), node["visible"] == true));
        }
        for key in ["nodes", "floating_nodes"] {
            for child in node[key].as_array().into_iter().flatten() {
                visible(child, out);
            }
        }
    }

    let mut stream = UnixStream::connect(&socket).unwrap();
    for focused in ["m-D", "m-C", "m-B", "m-A"] {
        run(&mut fixture, &format!("[app_id=\"{focused}\"] focus"));
        let tree = query_ipc(&mut fixture, &mut stream, MessageType::GetTree);
        let mut found = Vec::new();
        visible(&tree, &mut found);
        found.sort();
        let expected: Vec<_> = ["m-A", "m-B", "m-C", "m-D"]
            .into_iter()
            .map(|app| (app.to_owned(), app == focused))
            .collect();
        assert_eq!(found, expected, "focused {focused}: only it is visible");
    }
}

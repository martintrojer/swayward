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
    assert_eq!(outcome[0].parse_error, Some(true));

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

fn focused_workspace(f: &mut Fixture) -> Option<String> {
    f.swayward().layout.active_workspace().unwrap().sway_name()
}

/// Oracle: command-fuzz invalid-stops-list, invalid-stops-comma-list and
/// failure-continues-list. A runtime CMD_INVALID ends the command list, while
/// a CMD_FAILURE does not (`sway/sway/commands.c:296-299`).
#[test]
fn runtime_invalid_result_stops_the_command_list() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    for list in [
        "scratchpad show; workspace oracle-invalid",
        "scratchpad show, workspace oracle-invalid",
    ] {
        let outcome = crate::command::execute(f.niri_state(), list);
        assert_eq!(outcome.len(), 1, "{list}: {outcome:?}");
        assert_eq!(outcome[0].error.as_deref(), Some("Scratchpad is empty"));
        assert_eq!(outcome[0].parse_error, Some(true));
        assert_eq!(focused_workspace(&mut f), Some("1".to_owned()), "{list}");
    }

    let outcome =
        crate::command::execute(f.niri_state(), "sticky enable; workspace oracle-failure");
    assert_eq!(outcome.len(), 2, "{outcome:?}");
    assert_eq!(outcome[0].parse_error, Some(false));
    assert!(outcome[1].success, "{outcome:?}");
    assert_eq!(focused_workspace(&mut f), Some("oracle-failure".to_owned()));
}

/// Oracle: criteria_failure_keeps_running_later_matches. Sway runs the handler
/// for every match and reports the last failure; a CMD_FAILURE on one match
/// does not stop later ones (`sway/sway/commands.c:305-323`). The floating
/// window sits between two tiled ones, so the reply must be the tiled failure
/// whichever order the matches run in, and the floating one must still move.
#[test]
fn criteria_failure_on_one_match_still_runs_later_matches() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(
        &mut f,
        &[("1", "fixture-1"), ("1", "fixture-2"), ("1", "fixture-3")],
    );
    assert!(
        crate::command::execute(f.niri_state(), "[app_id=\"^fixture-2$\"] floating enable")[0]
            .success
    );

    let outcome = crate::command::execute(
        f.niri_state(),
        "[app_id=\"^fixture-[123]$\"] move position 10 px 20 px",
    );
    assert_eq!(
        outcome,
        vec![crate::command::failure(
            "Only floating containers can be moved to an absolute position"
        )]
    );
    // The floating match still moved. The tree rect includes the titlebar
    // above the content, so subtract it to get the requested content origin.
    let tree = command_tree(&mut f);
    let floating = find_json_node_with_app_id(&tree, "fixture-2").unwrap();
    assert_eq!(floating["type"], "floating_con");
    assert_eq!(floating["rect"]["x"], 10, "{floating:#}");
    assert_eq!(
        floating["rect"]["y"].as_i64().unwrap() - floating["deco_rect"]["height"].as_i64().unwrap(),
        20,
        "{floating:#}"
    );
}

/// Oracle: criteria_invalid_stops_later_matches. A CMD_INVALID from one match
/// stops the remaining matches and the rest of the command list
/// (`sway/sway/commands.c:316-321`).
#[test]
fn criteria_invalid_result_stops_later_matches_and_the_list() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    windows_on_workspaces(&mut f, &[("1", "fixture-1"), ("1", "fixture-2")]);

    let outcome = crate::command::execute(
        f.niri_state(),
        "[app_id=\"^fixture-[12]$\"] scratchpad show; workspace oracle-after",
    );
    assert_eq!(outcome.len(), 1, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Container is not in scratchpad.")
    );
    assert_eq!(outcome[0].parse_error, Some(true));
    assert_eq!(focused_workspace(&mut f), Some("1".to_owned()));
}

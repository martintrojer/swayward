use super::*;

#[test]
pub(super) fn child_is_reaped_when_the_control_loop_panics() {
    let child = Command::new("sleep").arg("60").spawn().unwrap();
    let pid = child.id();
    let _ = std::panic::catch_unwind(move || {
        let _child = ChildGuard::new(child);
        panic!("injected control-loop panic");
    });
    assert!(
        !PathBuf::from(format!("/proc/{pid}")).exists(),
        "child {pid} survived its guard"
    );
}

#[test]
pub(super) fn child_output_is_drained_while_the_child_runs() {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("head -c 1048576 /dev/zero >&1; head -c 1048576 /dev/zero >&2")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = ChildOutput::new(&mut child);
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::yield_now();
    }
    assert!(
        child.try_wait().unwrap().is_some(),
        "child blocked on a full output pipe"
    );
    let (stdout, stderr) = output.finish();
    assert_eq!(stdout.len(), 1_048_576);
    assert_eq!(stderr.len(), 1_048_576);
}

#[test]
pub(super) fn harness_xcb_xkb_guard_does_not_depend_on_the_host() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg(format!("-I{}", oracle_i3_dir().join("lib").display()))
        .arg("-MExtUtils::PkgConfig")
        .arg("-e")
        .arg("exit !ExtUtils::PkgConfig->atleast_version('xcb-xkb', '1.11')")
        .env("PKG_CONFIG", "/does/not/exist")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "xcb-xkb probe used the host: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
pub(super) fn harness_does_not_convert_wrong_named_assertions_into_skips() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg(format!("-I{}", oracle_i3_dir().join("lib").display()))
        .arg("-e")
        .arg(
            "use i3test; is('splith', 'tabbed', \
             'workspace layout is \"tabbed\"'); done_testing;",
        )
        .env("SWAYWARD_I3_TEST", "509-workspace_layout.t")
        .env("SWAYWARD_I3_SKIPS", r#"{"2":"different assertion"}"#)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !output.status.success(),
        "a wrong assertion must fail: {stdout}"
    );
    assert!(
        stdout.contains("not ok 1 - workspace layout is \"tabbed\""),
        "the real comparison must reach TAP: {stdout}"
    );
    assert!(
        !stdout.contains("# skip"),
        "the harness must not intercept it: {stdout}"
    );
}

#[test]
pub(super) fn harness_skips_only_i3_invalid_criteria_wording() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg(format!("-I{}", oracle_i3_dir().join("lib").display()))
        .arg("-e")
        .arg(
            "use i3test; ok(1, 'command was unsuccessful'); \
             is('sway text', 'i3 text', 'correct error is returned'); done_testing;",
        )
        .env("SWAYWARD_I3_TEST", "260-invalid-criteria.t")
        .env("SWAYWARD_I3_SKIPS", r#"{"2":"i3 error wording differs"}"#)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("ok 1 - command was unsuccessful"),
        "{stdout}"
    );
    assert!(
        stdout.contains("ok 2 # skip i3 error wording differs"),
        "{stdout}"
    );
}

#[test]
pub(super) fn conformance_run_reports_every_failed_file() {
    let mut visited = Vec::new();
    let failures = collect_test_failures(
        [("first.t", true), ("good.t", true), ("last.t", true)],
        |test, _| {
            visited.push(test.to_owned());
            if test != "good.t" {
                panic!("failure in {test}");
            }
        },
    );

    assert_eq!(visited, ["first.t", "good.t", "last.t"]);
    assert_eq!(
        failures,
        [
            ("first.t".to_owned(), "failure in first.t".to_owned()),
            ("last.t".to_owned(), "failure in last.t".to_owned()),
        ]
    );
}

#[test]
pub(super) fn failure_diagnostics_name_assertions_and_non_tap_panics() {
    let payload = std::panic::catch_unwind(|| {
        with_test_context("setup-failure.t", || panic!("setup failed"));
    })
    .unwrap_err();
    assert_eq!(
        panic_message(payload.as_ref()),
        "i3 test setup-failure.t panicked: setup failed"
    );

    let stdout = "ok 159 - setup\nnot ok 160 - No empty workspace created\n1..160\n";
    let stderr = "#   Failed test 'No empty workspace created'\n#   at test.t line 398.\n";
    assert_eq!(
        tap_failure_summary(stdout, stderr),
        "not ok 160 - No empty workspace created\n#   Failed test 'No empty workspace created'\n#   at test.t line 398."
    );
    assert_eq!(
        tap_skips("ok 1 - portable\nok 2 # skip i3-only\n"),
        ["ok 2 # skip i3-only"]
    );
}

#[test]
pub(super) fn rejection_allowlist_is_keyed_by_file_and_exact_command() {
    let stderr = "# swayward rejected `layout default`: error\n\
# swayward rejected `[con_mark=__does_not_exist] focus`: error\n";
    assert_eq!(
        rejected_commands(stderr).collect::<Vec<_>>(),
        allowed_rejections("101-focus.t")
            .iter()
            .map(|item| item.command)
            .collect::<Vec<_>>()
    );
    assert!(!rejections_match(
        "119-match.t",
        &rejected_commands(stderr).collect::<Vec<_>>()
    ));
    assert!(rejections_match(
        "111-goto.t",
        &["[con_mark=\"mark.A1b2\"] focus"]
    ));
    assert!(rejections_match(
        "294-focus-order.t",
        &[
            "[id=1] swap container with id 2",
            "[id=3] swap container with id 4",
            "[id=5] swap container with id 6",
        ]
    ));
    assert!(!rejections_match(
        "294-focus-order.t",
        &["[id=1] swap container with con_id 2"]
    ));
    assert!(!rejections_match(
        "294-focus-order.t",
        &[
            "[id=1] swap container with id 2",
            "[id=3] swap container with id 4",
        ]
    ));
    assert!(glob_matches(
        "[con_mark=\"*\"] focus",
        "[con_mark=\"mark.A1b2\"] focus"
    ));
    assert!(!glob_matches(
        "[con_mark=\"*\"] focus",
        "prefix [con_mark=\"mark.A1b2\"] focus"
    ));
    assert!(ALLOWED_REJECTIONS
        .iter()
        .all(|rejection| !rejection.reason.is_empty()));
}

#[test]
pub(super) fn headless_startup_outputs_follow_sways_backend_order() {
    let mut fixture = Fixture::new();
    let state = fixture.niri_state();
    let swayward = &mut state.swayward;
    state.backend.headless().add_startup_outputs(swayward, 3);

    let swayward = fixture.swayward();
    let actual = crate::ipc::tree::describe_outputs(&swayward.layout, &swayward.global_space);
    assert_eq!(
        actual
            .iter()
            .map(|output| (output.name.as_str(), output.rect.x))
            .collect::<Vec<_>>(),
        [
            ("headless-3", 0),
            ("headless-2", 1280),
            ("headless-1", 2560)
        ]
    );
}

#[test]
pub(super) fn fake_outputs_create_real_outputs_with_requested_geometry() {
    let outputs = fake_outputs("font monospace\nfake-outputs 1024x768+0+0P,800x600+1024+20\n")
        .unwrap()
        .unwrap();
    assert_eq!(outputs, [((0, 0), (1024, 768)), ((1024, 20), (800, 600))]);

    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 800));
    fixture.replace_outputs(outputs);
    let swayward = fixture.swayward();
    let actual = crate::ipc::tree::describe_outputs(&swayward.layout, &swayward.global_space);
    assert_eq!(
        actual
            .iter()
            .map(|output| (output.name.as_str(), output.rect))
            .collect::<Vec<_>>(),
        [
            (
                "fake-0",
                swayward_ipc::Rect {
                    x: 0,
                    y: 0,
                    width: 1024,
                    height: 768
                }
            ),
            (
                "fake-1",
                swayward_ipc::Rect {
                    x: 1024,
                    y: 20,
                    width: 800,
                    height: 600
                }
            ),
        ]
    );
    assert_eq!(
        crate::ipc::tree::describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .map(|workspace| (workspace.name.as_str(), workspace.output.as_str()))
            .collect::<Vec<_>>(),
        [("1", "fake-0"), ("2", "fake-1")]
    );
}

#[test]
pub(super) fn test_config_reload_requires_loaded_source() {
    let mut fixture = Fixture::new();
    assert_eq!(
        reload_loaded_test_config(&mut fixture, "", None).unwrap_err(),
        "no test config has been loaded"
    );
    reload_loaded_test_config(&mut fixture, "", Some("font monospace")).unwrap();
}

#[test]
pub(super) fn i3_config_translation_ignores_only_unsupported_bar_blocks() {
    translate_config("", "font monospace\nbar {\n    output primary\n}\n").unwrap();
    assert!(!only_ignorable_translation_warnings(
        "316-drag-container.t",
        "manual attention: 1 directive(s)\n  config:2: another warning\n"
    ));
    assert!(!only_ignorable_translation_warnings(
        "316-drag-container.t",
        "manual attention: 2 directive(s)\n  config:2: bar blocks are unsupported; use waybar (docs/SWAY_CONFIG_MIGRATION.md#replace-swaybar): bar { | }\n"
    ));

    let error = translate_config("", "bar { output primary }\nmystery value\n").unwrap_err();
    assert!(error.contains("manual attention: 2 directive(s)"));
    assert!(error.contains("unhandled: mystery value"));
}

#[test]
pub(super) fn i3_config_translation_ignores_provenance_warnings_only_for_271() {
    let warnings = "manual attention: 2 directive(s)\n  config:2: i3-only provenance criterion tiling_from has no sway equivalent: for_window [tiling_from=\"auto\"]\n  config:3: i3-only provenance criterion floating_from has no sway equivalent: for_window [floating_from=\"user\"]\n";
    assert!(only_ignorable_translation_warnings(
        "271-for_window_tilingfloating.t",
        warnings
    ));
    assert!(!only_ignorable_translation_warnings(
        "272-regress-focus-assign.t",
        warnings
    ));
}

/// The per-file overrides follow the file being run, not `SWAYWARD_I3_TEST`
/// in the test process. The gate leaves that variable unset, so a file that
/// loads its own config used to get the overrides only when measured alone.
#[test]
pub(super) fn a_file_loaded_config_keeps_its_per_file_overrides() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();
    let scratch = I3Scratch::new();
    let mut session = Session {
        test: "257-keypress-group1-fallback.t",
        client,
        loaded_config_source: None,
        scratch: &scratch,
        initially_floating: HashSet::new(),
    };
    let reply = load_config_source(&mut fixture, &mut session, "font monospace\n");
    assert_eq!(reply, json!({ "success": true }));
    let xkb = fixture
        .swayward()
        .config
        .borrow()
        .input
        .keyboard
        .xkb
        .clone();
    assert_eq!(xkb.layout, "us,ru");
    assert_eq!(xkb.options.as_deref(), Some("grp:alt_shift_toggle"));

    reload_loaded_test_config(
        &mut fixture,
        "257-keypress-group1-fallback.t",
        Some("font monospace\n"),
    )
    .unwrap();
    let layout = fixture
        .swayward()
        .config
        .borrow()
        .input
        .keyboard
        .xkb
        .layout
        .clone();
    assert_eq!(layout, "us,ru");
}

#[test]
pub(super) fn i3_config_translation_rejects_unhandled_directives() {
    let error = translate_config("", "font monospace\nmystery value\n").unwrap_err();
    assert!(error.contains("manual attention: 1 directive(s)"));
    assert!(error.contains("unhandled: mystery value"));
}

#[test]
pub(super) fn i3_config_translation_never_applies_a_partial_config() {
    let incomplete = translate_config("", "bindsym X\n").unwrap_err();
    assert!(incomplete.contains("manual attention: 1 directive(s)"));
    assert!(incomplete.contains("malformed bindsym: X"));
}

#[test]
pub(super) fn explicit_default_binding_mode_loads() {
    let config = translate_config("", "mode \"default\" {\n    bindsym X nop\n}\n").unwrap();
    assert_eq!(config.binds.0.len(), 1);
    assert!(!config
        .binding_modes
        .iter()
        .any(|mode| mode.name == "default"));
}

#[test]
pub(super) fn workspace_layout_config_wraps_new_windows() {
    let config = translate_config("", "workspace_layout tabbed\n").unwrap();
    assert_eq!(
        config.layout.workspace_layout,
        swayward_config::WorkspaceLayout::Tabbed
    );
}

#[test]
fn initial_floating_applies_only_to_the_requested_window() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();

    let floating = create_window(&mut fixture, client, &json!({ "initial_floating": true }));
    map_window(&mut fixture, client, floating, None, true);
    assert!(fixture.swayward().layout.focus().unwrap().is_floating());

    let tiled = create_window(&mut fixture, client, &json!({}));
    map_window(&mut fixture, client, tiled, None, false);
    assert!(!fixture.swayward().layout.focus().unwrap().is_floating());
}

#[test]
fn settling_configures_does_not_ack_an_already_acked_configure() {
    let mut config =
        prepare_test_config("", "font monospace\nno_focus [app_id=\"^notme$\"]\n").unwrap();
    config.debug.deactivate_unfocused_windows = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();
    let first = create_window(&mut fixture, client, &json!({}));
    map_window(&mut fixture, client, first, None, false);
    let second = create_window(&mut fixture, client, &json!({ "app_id": "notme" }));
    map_window(&mut fixture, client, second, None, false);
    settle_configures(&mut fixture, client);
}

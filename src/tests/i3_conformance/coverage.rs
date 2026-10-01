/// The i3 conformance classification: one entry per vendored file with its
/// assertion counts, reason code, citation and note.
///
/// This is the source of truth. The set of fully green files is *derived* from
/// it rather than listed separately, because a hand-maintained duplicate of a
/// derivable fact drifts: the old `passing.txt` and the coverage prose
/// disagreed about the green count more than once, and each had its own
/// invariant asserting it against the other.
const COVERAGE: &str = include_str!("../../../tests/i3/coverage.toml");
const COVERAGE_README: &str = include_str!("../../../tests/i3/README.md");
const PROJECT_README: &str = include_str!("../../../README.md");

#[test]
fn child_is_reaped_when_the_control_loop_panics() {
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
fn child_output_is_drained_while_the_child_runs() {
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
fn harness_xcb_xkb_guard_does_not_depend_on_the_host() {
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
fn harness_does_not_convert_wrong_named_assertions_into_skips() {
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
fn harness_skips_only_i3_invalid_criteria_wording() {
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
fn conformance_run_reports_every_failed_file() {
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
fn failure_diagnostics_name_assertions_and_non_tap_panics() {
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
fn rejection_allowlist_is_keyed_by_file_and_exact_command() {
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
    assert!(ALLOWED_REJECTIONS
        .iter()
        .all(|rejection| !rejection.reason.is_empty()));
}

#[test]
fn headless_startup_outputs_follow_sways_backend_order() {
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
fn fake_outputs_create_real_outputs_with_requested_geometry() {
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
fn test_config_reload_requires_loaded_source() {
    let mut fixture = Fixture::new();
    assert_eq!(
        reload_loaded_test_config(&mut fixture, "", None).unwrap_err(),
        "no test config has been loaded"
    );
    reload_loaded_test_config(&mut fixture, "", Some("font monospace")).unwrap();
}

#[test]
fn i3_config_translation_ignores_only_unsupported_bar_blocks() {
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
fn i3_config_translation_ignores_provenance_warnings_only_for_271() {
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
fn a_file_loaded_config_keeps_its_per_file_overrides() {
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
fn i3_config_translation_rejects_unhandled_directives() {
    let error = translate_config("", "font monospace\nmystery value\n").unwrap_err();
    assert!(error.contains("manual attention: 1 directive(s)"));
    assert!(error.contains("unhandled: mystery value"));
}

#[test]
fn i3_config_translation_never_applies_a_partial_config() {
    let incomplete = translate_config("", "bindsym X\n").unwrap_err();
    assert!(incomplete.contains("manual attention: 1 directive(s)"));
    assert!(incomplete.contains("malformed bindsym: X"));
}

#[test]
fn explicit_default_binding_mode_loads() {
    let config = translate_config("", "mode \"default\" {\n    bindsym X nop\n}\n").unwrap();
    assert_eq!(config.binds.0.len(), 1);
    assert!(!config
        .binding_modes
        .iter()
        .any(|mode| mode.name == "default"));
}

#[test]
fn workspace_layout_config_wraps_new_windows() {
    let config = translate_config("", "workspace_layout tabbed\n").unwrap();
    assert_eq!(
        config.layout.workspace_layout,
        swayward_config::WorkspaceLayout::Tabbed
    );
}

/// One entry from `coverage.toml`: the file name and the fields this runner
/// needs. Parsed with a small reader rather than a TOML crate, because the
/// document is flat and adding a runtime dependency for four integers is worse
/// than twenty lines of parsing.
struct Coverage {
    file: &'static str,
    assertions: usize,
    passing: usize,
    failing: usize,
    unreached: usize,
    documented_skip_count: usize,
    plan_unknown: bool,
}

fn coverage_entries() -> Vec<Coverage> {
    let mut entries = Vec::new();
    let mut current: Option<Coverage> = None;
    for line in COVERAGE.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[files.\"") {
            if let Some(done) = current.take() {
                entries.push(done);
            }
            let file = rest
                .split_once("\"]")
                .expect("a files entry names its file")
                .0;
            current = Some(Coverage {
                file,
                assertions: 0,
                passing: 0,
                failing: 0,
                unreached: 0,
                documented_skip_count: 0,
                plan_unknown: false,
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        // Values are bare integers or `true`; notes may contain anything, so
        // only read the keys this runner uses.
        let read = |value: &str| -> usize {
            value
                .split(|c: char| !c.is_ascii_digit())
                .find(|part| !part.is_empty())
                .unwrap_or("0")
                .parse()
                .unwrap_or(0)
        };
        if let Some(value) = line.strip_prefix("assertions = ") {
            entry.assertions = read(value);
        } else if let Some(value) = line.strip_prefix("pass = ") {
            entry.passing = read(value);
        } else if let Some(value) = line.strip_prefix("fail = ") {
            entry.failing = read(value);
        } else if let Some(value) = line.strip_prefix("unreached = ") {
            entry.unreached = read(value);
        } else if line.starts_with("plan_unknown = true") {
            entry.plan_unknown = true;
        } else if line.starts_with("{ n = ") {
            // Inline skips have one item per line. Table-array skips have one
            // heading per assertion. Count both spellings so the README census
            // follows the same source as contrib/coverage-report.
            entry.documented_skip_count += 1;
        } else if line.starts_with(&format!("[[files.\"{}\".skip]]", entry.file)) {
            entry.documented_skip_count += 1;
        }
    }
    if let Some(done) = current {
        entries.push(done);
    }
    entries
}

/// The files that pass every declared assertion.
///
/// A file with no TAP plan cannot be green: `plan_unknown` means it aborted
/// before declaring how many assertions it has, so "all of them pass" is not a
/// statement anyone can make. Treating the reached count as the plan silently
/// credited fourteen files with a plan they do not have. A zero-assertion file
/// is likewise not green; it offers no evidence either way.
fn passing_tests() -> impl Iterator<Item = &'static str> {
    coverage_entries()
        .into_iter()
        .filter(|entry| {
            entry.assertions > 0 && !entry.plan_unknown && entry.passing == entry.assertions
        })
        .map(|entry| entry.file)
        .collect::<Vec<_>>()
        .into_iter()
}

/// Keep the public in-process census tied to coverage.toml. Publishing passes
/// alone hid the documented skips, failures, and assertions that the harness
/// never reached, so the README must state all four figures together.
#[test]
fn project_readme_census_figures_match_the_manifest() {
    let entries = coverage_entries();
    let pass = entries.iter().map(|entry| entry.passing).sum::<usize>();
    let skip = entries
        .iter()
        .map(|entry| entry.documented_skip_count)
        .sum::<usize>();
    let fail = entries.iter().map(|entry| entry.failing).sum::<usize>();
    let unreached = entries.iter().map(|entry| entry.unreached).sum::<usize>();
    let count = |n: usize| {
        if n < 1_000 {
            n.to_string()
        } else {
            format!("{},{:03}", n / 1_000, n % 1_000)
        }
    };
    let claim = format!(
        "**{} passes, {} documented skips,\n{} failures, and {} unreached assertions**",
        count(pass),
        count(skip),
        count(fail),
        count(unreached),
    );
    assert_eq!(
        PROJECT_README.matches(&claim).count(),
        1,
        "README.md must state the coverage.toml pass/skip/fail/unreached census exactly once"
    );
}

/// The files that are not green and carry no documented skip: every one of
/// their non-passing assertions is swayward's own backlog, so closing it would
/// make the file green.
///
/// This is the only honest definition of the ceiling's second term, and it has
/// to be derived. The gap-only set was maintained by hand for a while and
/// listed four files that could never be green: three carried permanent
/// documented skips and one had no captured TAP plan. The arithmetic around
/// them was self-consistent, so every check passed while the published ceiling
/// was wrong in both directions at once.
fn gap_only_tests() -> Vec<&'static str> {
    coverage_entries()
        .into_iter()
        .filter(|entry| {
            entry.assertions > 0
                && !entry.plan_unknown
                && entry.passing != entry.assertions
                && entry.documented_skip_count == 0
        })
        .map(|entry| entry.file)
        .collect()
}

/// The manifest is read by the runner and reviewed by hand, and concurrent
/// merges have appended entries out of order three times. Sorted and duplicate
/// free keeps a review diff honest and stops one file being listed twice.
#[test]
fn passing_manifest_is_sorted_and_unique() {
    let files = passing_tests().collect::<Vec<_>>();
    let mut sorted = files.clone();
    sorted.sort_unstable();
    assert_eq!(files, sorted, "the derived green set is not sorted");
    let mut seen = std::collections::HashSet::new();
    for file in &files {
        assert!(
            seen.insert(file),
            "the derived green set lists {file} twice"
        );
    }
}

/// A green file's coverage row must not read as a failing one. `297-scroll-tabbed.t`
/// passed 14/14 while its row led with `diagnostic: 4 pass; 10 fail`, which
/// described a superseded reduced run rather than the file. The number is only
/// safe to quote when the leading result belongs to the manifest entry.
#[test]
fn green_coverage_rows_do_not_lead_with_a_failing_result() {
    let green = passing_tests().collect::<std::collections::HashSet<_>>();
    for line in COVERAGE_README.lines() {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let Some((file, rest)) = rest.split_once("` |") else {
            continue;
        };
        if !green.contains(file) {
            continue;
        }
        // The result cell is a terse measurement, not prose: it is the first
        // cell whose words are only counts and verdicts. Matching any cell
        // containing "fail" instead picks up citation prose such as "returns
        // sway's `No matching node.` failure".
        let Some(result) = rest.split('|').find(|cell| {
            let head = cell.split('(').next().unwrap_or(cell).trim();
            // A bare assertion count such as "14" occupies an earlier cell and
            // would always look clean, so require a verdict word too.
            head.split_whitespace()
                .any(|word| matches!(word.trim_end_matches([';', ',']), "pass" | "fail" | "skip"))
                && head.split_whitespace().all(|word| {
                    let word = word.trim_end_matches([';', ',']);
                    word.chars().all(|c| c.is_ascii_digit())
                        || matches!(
                            word,
                            "pass" | "fail" | "skip" | "finished:" | "reached" | "diagnostic:"
                        )
                })
        }) else {
            continue;
        };
        let (verdict, aside) = result.split_once('(').unwrap_or((result, ""));
        assert!(
            !verdict.contains("fail") && !verdict.contains("skip"),
            "{file} derives as green but its coverage row reports \
             {verdict:?} outside any parenthetical aside ({aside:?}); a green \
             file's leading result must be its own"
        );
    }
}

/// The harness adjusts its behaviour per test file, and an audit found those
#[test]
fn i3_scratch_defaults_off_tmpfs_and_removes_its_whole_directory() {
    let path = {
        let scratch = I3Scratch::new();
        let path = scratch.path.clone();
        assert!(path.starts_with("/var/tmp"));
        std::fs::write(scratch.path("still-open.sock"), b"socket stand-in").unwrap();
        path
    };
    assert!(!path.exists());
}


fn coverage_adaptations(test: &str) -> Value {
    let header = format!(r#"[files."{test}"]"#);
    let mut in_file = false;
    for line in COVERAGE.lines().map(str::trim) {
        if line.starts_with("[files.\"") {
            in_file = line == header;
            continue;
        }
        if in_file {
            if let Some(flags) = line.strip_prefix("adapt = [").and_then(|s| s.strip_suffix(']')) {
                return Value::Array(
                    flags.split(',')
                        .map(str::trim)
                        .filter_map(|flag| flag.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
                        .map(|flag| Value::String(flag.to_owned()))
                        .collect(),
                );
            }
        }
    }
    Value::Array(Vec::new())
}

fn coverage_skip_adaptations(test: &str) -> Value {
    let header = format!(r#"[files."{test}"]"#);
    let mut in_file = false;
    let mut current_number = None;
    let mut skips = serde_json::Map::new();
    for line in COVERAGE.lines().map(str::trim) {
        if line.starts_with("[files.\"") {
            in_file = line == header;
            current_number = None;
            continue;
        }
        if !in_file {
            continue;
        }
        if line == format!(r#"[[files."{test}".skip]]"#) {
            current_number = None;
        } else if let Some(number) = line.strip_prefix("n = ") {
            current_number = number.parse::<usize>().ok();
        } else if let (Some(number), Some(reason)) =
            (current_number, line.strip_prefix("reason = \"").and_then(|s| s.strip_suffix('"')))
        {
            skips.insert(number.to_string(), Value::String(reason.to_owned()));
        }
    }
    Value::Object(skips)
}

#[test]
fn conformance_client_binds_a_modern_xdg_wm_base_version() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();
    let handle = create_window(&mut fixture, client, &json!({}));
    let id = map_window(&mut fixture, client, handle, None, false);
    assert!(window_states(&mut fixture, client, id).is_some());
    assert!(fixture.client(client).state.xdg_wm_base_version >= Some(2));
}

#[test]
fn every_harness_adaptation_has_a_sway_citation() {
    let mut file = None;
    let mut adaptations = Vec::new();
    let mut citation = None;
    let check = |file: Option<&str>, adaptations: &[&str], citation: Option<&str>| {
        if !adaptations.is_empty() {
            assert!(
                citation.is_some_and(|citation| !citation.is_empty()),
                "{file:?} adaptations {adaptations:?} have no source citation"
            );
        }
    };
    for line in COVERAGE.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("[files.\"") {
            check(file, &adaptations, citation);
            file = rest.split_once("\"]").map(|(file, _)| file);
            adaptations.clear();
            citation = None;
        } else if let Some(flags) = line.strip_prefix("adapt = [").and_then(|s| s.strip_suffix(']')) {
            adaptations = flags
                .split(',')
                .map(str::trim)
                .filter_map(|flag| flag.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
                .collect();
        } else if let Some((_, value)) = line.split_once("citation = \"") {
            citation = value.split_once('"').map(|(citation, _)| citation);
        }
    }
    check(file, &adaptations, citation);
}

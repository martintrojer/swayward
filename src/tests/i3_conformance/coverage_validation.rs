/// The ceiling paragraph is maintained by hand and a merge once left two
/// contradictory copies, each with a different green count. Assert there is
/// exactly one and that its arithmetic matches the manifest and the explicit
/// gap-only table. Ordinary coverage-row edits do not affect this check.
#[test]
fn documented_green_ceiling_matches_the_manifest() {
    let claims = COVERAGE_README
        .lines()
        .filter_map(|line| line.trim().strip_prefix("The **current green ceiling is "))
        .collect::<Vec<_>>();
    assert_eq!(
        claims.len(),
        1,
        "tests/i3/README.md must state the green ceiling exactly once"
    );

    let ceiling: usize = claims[0]
        .split_once(" files**")
        .expect("ceiling claim states a file count")
        .0
        .parse()
        .expect("ceiling is a number");
    let stated_green: usize = claims[0]
        .rsplit_once("the ")
        .expect("ceiling claim cites the manifest size")
        .1
        .split_once(' ')
        .expect("manifest size is followed by a word")
        .0
        .parse()
        .expect("manifest size is a number");

    let green_files = passing_tests().collect::<std::collections::HashSet<_>>();
    let green = green_files.len();
    assert_eq!(
        stated_green, green,
        "the ceiling paragraph cites {stated_green} green files but coverage.toml derives {green}"
    );
    let gap_only = COVERAGE_README
        .lines()
        .find_map(|line| {
            line.trim().strip_suffix(
                " vendored files whose only obstacles are implementation or adapter gaps.",
            )
        })
        .expect("ceiling paragraph states a gap-only count")
        .parse::<usize>()
        .expect("gap-only count is a number");
    let gap_only_files = COVERAGE_README
        .lines()
        .skip_while(|line| *line != "| Gap-only file | Reached | Remaining gap |")
        .skip(2)
        .take_while(|line| line.starts_with("| `"))
        .map(|line| {
            line.strip_prefix("| `")
                .and_then(|line| line.split_once("` |"))
                .expect("gap-only table row contains a backtick-quoted filename")
                .0
        })
        .collect::<Vec<_>>();
    assert_eq!(
        gap_only_files.len(),
        gap_only,
        "the gap-only table lists {} files but the ceiling paragraph claims {gap_only}",
        gap_only_files.len()
    );
    // The membership itself, not only its size. Listing the wrong files is how
    // the published ceiling went wrong while every arithmetic check passed.
    let mut listed = gap_only_files.clone();
    listed.sort_unstable();
    let mut derived = gap_only_tests();
    derived.sort_unstable();
    assert_eq!(
        listed, derived,
        "the gap-only table does not match the set coverage.toml derives; a \
         file with a documented skip or no captured plan can never be green"
    );
    // The prose above the table restates the count. It went stale at 13 while
    // the table held 12, because every other invariant checked the ceiling
    // paragraph instead of this sentence.
    let introduced = COVERAGE_README
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("These ")
                .and_then(|line| line.strip_suffix(" files define the gap-only set:"))
        })
        .expect("the gap-only table is introduced by a sentence stating its size")
        .parse::<usize>()
        .expect("the gap-only introduction states a file count");
    assert_eq!(
        introduced, gap_only,
        "the gap-only table is introduced as {introduced} files but the ceiling \
         paragraph claims {gap_only}"
    );
    let mut unique = std::collections::HashSet::new();
    for file in &gap_only_files {
        assert!(unique.insert(file), "the gap-only table lists {file} twice");
        assert!(
            !green_files.contains(file),
            "the gap-only table also lists green file {file}"
        );
        assert!(
            oracle_i3_dir().join("t").join(file).is_file(),
            "the gap-only table lists a file absent from the pinned oracle: {file}"
        );
    }
    assert_eq!(
        green + gap_only_files.len(),
        ceiling,
        "{green} green plus {} gap-only must equal the stated ceiling {ceiling}",
        gap_only_files.len()
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
        prepare_test_config("font monospace\nno_focus [app_id=\"^notme$\"]\n").unwrap();
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

#[test]
fn i3_conformance_runner() {
    // `SWAYWARD_I3_TEST` selects a single file, including one with known
    // failures, so conformance findings stay executable without turning the
    // default gate red.
    if let Ok(selected) = std::env::var("SWAYWARD_I3_TEST") {
        run_i3_test_with_context(&selected);
        return;
    }

    let tests = passing_tests().collect::<Vec<_>>();
    assert!(
        !tests.is_empty(),
        "no fully green files derive from tests/i3/coverage.toml"
    );
    let failures = collect_test_failures(tests.iter().copied(), run_i3_test_with_context);
    assert!(
        failures.is_empty(),
        "{} of {} green i3 files failed:\n{}",
        failures.len(),
        tests.len(),
        failures
            .iter()
            .map(|(test, message)| format!("{test}: {message}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}

/// `unreached` is the label most easily abused, because an assertion moved
/// from `fail` to `unreached` looks identical in raw TAP output: the run says
/// `not ok` either way. The distinction is whether the assertion's premise
/// held, so a row carrying unreached assertions must name the premise that was
/// never established. Without this check, the label is an unfalsifiable way to
/// reduce the fail count.
///
/// This reads coverage.toml. It used to scan a prose table in
/// tests/i3/README.md, which silently stopped guarding anything when that
/// duplicated table was removed in favour of the file it had been copied from.
#[test]
fn unreached_rows_name_their_missing_premise() {
    // Walk the file the same way coverage_entries does, rather than adding a
    // TOML dependency to the test crate for one invariant.
    let mut checked = 0;
    let mut file = "";
    let mut unreached = 0usize;
    let mut notes = String::new();
    // Nine rows predate this check and do not name their premise. They are a
    // work queue, not an exemption: a row may leave this list by gaining a
    // premise, never by keeping silent. Adding to it is a test failure.
    const SILENT: &[&str] = &[
        "162-regress-dock-urgent.t",
        "211-regress-urgency-assign.t",
        "231-ipc-floating-event.t",
        "289-ipc-shutdown-event.t",
        "511-scratchpad-configure-request.t",
        "527-focus-fallback.t",
        "534-dont-warp.t",
        "551-net-wm-state-maximized.t",
        "553-popup_during_fullscreen.t",
    ];
    let mut still_silent = Vec::new();
    let mut flush = |file: &str, unreached: usize, notes: &str, checked: &mut usize| {
        if file.is_empty() || unreached == 0 {
            return;
        }
        *checked += 1;
        if SILENT.contains(&file) {
            if !names_premise(notes) {
                still_silent.push(file.to_owned());
            }
            return;
        }
        assert!(
            names_premise(notes),
            "{file} records unreached assertions but its note does not name \
             the premise that was never established; an unreached row must say \
             which input or earlier assertion is missing, or the label is \
             hiding a defect"
        );
    };
    for line in COVERAGE.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("[files.\"") {
            flush(file, unreached, &notes, &mut checked);
            file = rest
                .split_once("\"]")
                .expect("a files entry names its file")
                .0;
            unreached = 0;
            notes.clear();
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("unreached = ") {
            unreached = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = trimmed.strip_prefix("note = ") {
            notes.push_str(value);
        }
    }
    flush(file, unreached, &notes, &mut checked);
    assert!(
        checked >= 5,
        "expected at least five rows with unreached assertions, found {checked}"
    );
    // The list must shrink. A row that gained a premise has to be removed from
    // SILENT, or the list stops describing the backlog it exists to bound.
    let fixed = SILENT
        .iter()
        .filter(|file| !still_silent.iter().any(|silent| silent == *file))
        .collect::<Vec<_>>();
    assert!(
        fixed.is_empty(),
        "these rows now name their premise and must be removed from SILENT: {fixed:?}"
    );
}

/// The premise is always an input the harness cannot supply, or an earlier
/// assertion that did not establish the expected state.
fn names_premise(notes: &str) -> bool {
    {
        [
            "cannot establish",
            "depend on",
            "depends on",
            "require",
            "requires",
            "never establish",
            "unavailable",
            "no equivalent",
            "has no request",
            "cannot set",
            "cannot be set",
            "cannot express",
            "fails loud",
            "emits no plan",
            "reaches no assertions",
            "aborts",
            "timeout",
        ]
        .iter()
        .any(|phrase| notes.contains(phrase))
    }
}

/// `tests/i3/coverage.toml` is the source of truth for i3 conformance
/// classification, and `contrib/coverage-report --check` validates it: every
/// assertion classified exactly once, every non-pass carrying a reason, every
/// skip carrying a citation, and the manifest agreeing with the per-file
/// counts.
///
/// The classification used to live only in Markdown prose, which meant counting
/// it required reverse-engineering English. That produced a fail total of 880
/// when the real figure was 208, and a green ceiling of 107 when it was 105 --
/// always overstating defects, which is the expensive direction because it
/// sends people hunting for bugs that are already explained.
///
/// The check is currently allowed a known number of violations, because the
/// data was extracted from that prose and the missing reasons and citations are
/// being filled in file by file. The budget only ever ratchets down: lowering
/// it is the unit of progress, and raising it requires deleting this comment
/// and explaining why a documented gap became undocumented again.
#[test]
fn coverage_data_validates_completely() {
    // Every assertion either passes or is a documented, cited skip. This began
    // as a ratchet at 165 violations while the data was extracted from prose;
    // it is now an invariant, so any entry that claims something it has not
    // shown fails the build.
    let output = std::process::Command::new("python3")
        .arg("contrib/coverage-report")
        .arg("--check")
        .output()
        .expect("contrib/coverage-report runs");
    let report = String::from_utf8(output.stdout).expect("report is UTF-8");
    let count: usize = report
        .lines()
        .last()
        .expect("the report ends with a violation count")
        .split_once(" violation")
        .expect("the last line states a violation count")
        .0
        .parse()
        .expect("the violation count is a number");

    assert_eq!(
        count, 0,
        "coverage.toml has {count} validation violations; run \
         contrib/coverage-report --check and fix each:\n{report}"
    );
}

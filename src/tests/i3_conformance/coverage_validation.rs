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

#[test]
fn i3_conformance_runner() {
    // `SWAYWARD_I3_TEST` selects a single file, including one with known
    // failures, so conformance findings stay executable without turning the
    // default gate red.
    if let Ok(selected) = std::env::var("SWAYWARD_I3_TEST") {
        run_i3_test_with_context(&selected, false);
        return;
    }

    let tests = passing_tests().collect::<Vec<_>>();
    assert!(
        !tests.is_empty(),
        "no fully green files derive from tests/i3/coverage.toml"
    );
    let failures = collect_test_failures(
        tests.iter().map(|test| (*test, true)),
        run_i3_test_with_context,
    );
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

/// Counts from the first TAP stream of one file, by `contrib/tap-count`'s
/// rule: only unindented results are the file's own, a restart in numbering
/// begins a panic echo, and a directive starts at the first unescaped `#`.
#[derive(Debug, Default, PartialEq)]
struct TapTally {
    pass: usize,
    skip: usize,
    todo: usize,
    fail: usize,
}

fn tap_tally(stdout: &str) -> TapTally {
    let mut tally = TapTally::default();
    let mut previous = 0;
    for line in stdout.lines() {
        let (passed, rest) = if let Some(rest) = line.strip_prefix("ok ") {
            (true, rest)
        } else if let Some(rest) = line.strip_prefix("not ok ") {
            (false, rest)
        } else {
            continue;
        };
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let Ok(number) = rest[..digits].parse::<usize>() else {
            continue;
        };
        if number <= previous {
            break;
        }
        previous = number;
        let trailer = &rest[digits..];
        let directive = trailer
            .char_indices()
            .find(|&(index, c)| c == '#' && !trailer[..index].ends_with('\\'))
            .map(|(index, _)| trailer[index + 1..].trim_start().to_ascii_lowercase());
        let word = |name: &str| {
            directive.as_deref().is_some_and(|text| {
                text.strip_prefix(name).is_some_and(|after| {
                    !after.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                })
            })
        };
        if word("todo") {
            tally.todo += 1;
        } else if passed && word("skip") {
            tally.skip += 1;
        } else if passed {
            tally.pass += 1;
        } else {
            tally.fail += 1;
        }
    }
    tally
}

#[test]
fn tap_tally_follows_tap_counts_first_stream_rule() {
    let stdout = "1..6\n    ok 1 - inner\n    1..1\nok 1 - subtest\nnot ok 2 - b\n\
                  ok 3 # skip reason\nok 4 - names \\# skip\nnot ok 5 # TODO later\n\
                  ok 6 - a # skipped-ish\nok 1 - panic echo\n";
    assert_eq!(
        tap_tally(stdout),
        TapTally {
            pass: 3,
            skip: 1,
            todo: 1,
            fail: 1,
        }
    );
}

/// The default gate runs only fully green files, which leaves most recorded
/// passes unchecked: a change that turns them into failures, or a harness
/// branch that turns them into TAP skips, would keep the gate green. This
/// re-runs every other file with recorded passes and asserts that none of
/// them lost one.
///
/// Opt in with `SWAYWARD_I3_RATCHET=1`. It runs about 140 files, and
/// coverage.toml has drifted from a fresh measurement, so it is not yet part
/// of the `RUN_SLOW_TESTS` CI job; see task i3-coverage-rebaseline. A rise is
/// reported too, so the row can be updated.
#[test]
fn i3_conformance_non_green_passes_do_not_drop() {
    if std::env::var_os("SWAYWARD_I3_RATCHET").is_none() {
        return;
    }
    let entries = coverage_report()["non_green_passes"]
        .as_object()
        .expect("coverage-report emits non_green_passes")
        .iter()
        .map(|(file, passing)| (file.as_str(), passing.as_u64().unwrap() as usize))
        .collect::<Vec<_>>();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = std::sync::Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(entry) = entries.get(index) else {
                    break;
                };
                let observed =
                    std::panic::catch_unwind(|| tap_tally(&run_i3_file(entry.0).stdout).pass)
                        .unwrap_or(0);
                results.lock().unwrap().push((entry.0, entry.1, observed));
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_unstable();
    let report = |pick: fn(usize, usize) -> bool| {
        results
            .iter()
            .filter(|(_, recorded, observed)| pick(*recorded, *observed))
            .map(|(file, recorded, observed)| format!("  {file}: {recorded} -> {observed}"))
            .collect::<Vec<_>>()
    };
    let rose = report(|recorded, observed| observed > recorded);
    if !rose.is_empty() {
        eprintln!(
            "non-green files passing more than coverage.toml records; update their rows:\n{}",
            rose.join("\n")
        );
    }
    let dropped = report(|recorded, observed| observed < recorded);
    assert!(
        dropped.is_empty(),
        "{} of {} non-green i3 files pass fewer assertions than coverage.toml records \
         (recorded -> observed):\n{}",
        dropped.len(),
        results.len(),
        dropped.join("\n")
    );
}

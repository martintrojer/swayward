use super::*;

/// One classification entry per vendored i3 file.
pub(super) const COVERAGE: &str = include_str!("../../../tests/i3/coverage.toml");

pub(super) fn coverage_report() -> &'static Value {
    static REPORT: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    REPORT.get_or_init(|| {
        let output =
            Command::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("contrib/coverage-report"))
                .arg("--json")
                .output()
                .expect("run contrib/coverage-report --json");
        assert!(
            output.status.success(),
            "coverage-report failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("coverage-report emits JSON")
    })
}

pub(super) fn report_file_list(key: &str) -> impl Iterator<Item = &'static str> {
    coverage_report()[key]
        .as_array()
        .unwrap_or_else(|| panic!("coverage-report has no {key} list"))
        .iter()
        .map(|value| value.as_str().expect("coverage file name is a string"))
}

pub(super) fn passing_tests() -> impl Iterator<Item = &'static str> {
    report_file_list("green_file_names")
}

/// The manifest is read by the runner and reviewed by hand, and concurrent
/// merges have appended entries out of order three times. Sorted and duplicate
/// free keeps a review diff honest and stops one file being listed twice.
#[test]
pub(super) fn passing_manifest_is_sorted_and_unique() {
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

#[test]
pub(super) fn i3_scratch_defaults_off_tmpfs_and_removes_its_whole_directory() {
    let path = {
        let scratch = I3Scratch::new();
        let path = scratch.path.clone();
        assert!(path.starts_with("/var/tmp"));
        std::fs::write(scratch.path("still-open.sock"), b"socket stand-in").unwrap();
        path
    };
    assert!(!path.exists());
}

pub(super) fn coverage_adaptations(test: &str) -> Value {
    let header = format!(r#"[files."{test}"]"#);
    let mut in_file = false;
    for line in COVERAGE.lines().map(str::trim) {
        if line.starts_with("[files.\"") {
            in_file = line == header;
            continue;
        }
        if in_file {
            if let Some(flags) = line
                .strip_prefix("adapt = [")
                .and_then(|s| s.strip_suffix(']'))
            {
                return Value::Array(
                    flags
                        .split(',')
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

pub(super) fn coverage_skip_adaptations(test: &str) -> Value {
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
        } else if let (Some(number), Some(reason)) = (
            current_number,
            line.strip_prefix("reason = \"")
                .and_then(|s| s.strip_suffix('"')),
        ) {
            skips.insert(number.to_string(), Value::String(reason.to_owned()));
        }
    }
    Value::Object(skips)
}

#[test]
pub(super) fn conformance_client_binds_a_modern_xdg_wm_base_version() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();
    let handle = create_window(&mut fixture, client, &json!({}));
    let id = map_window(&mut fixture, client, handle, None, false);
    assert!(window_states(&mut fixture, client, id).is_some());
    assert!(fixture.client(client).state.xdg_wm_base_version >= Some(2));
}

#[test]
pub(super) fn every_harness_adaptation_has_a_sway_citation() {
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
        } else if let Some(flags) = line
            .strip_prefix("adapt = [")
            .and_then(|s| s.strip_suffix(']'))
        {
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

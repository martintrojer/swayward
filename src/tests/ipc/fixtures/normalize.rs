fn oracle_fixture(path: &str) -> String {
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache/sway-ipc-oracle");
    oracle_fixture_at(&cache, path).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn oracle_fixture_contents_are_owned() {
    fn assert_owned(_: String) {}
    assert_owned(oracle_fixture("one_window.tree.json"));
}

fn oracle_fixture_at(cache: &std::path::Path, path: &str) -> Result<String, String> {
    let expected = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/oracle.toml"))
        .lines()
        .find_map(|line| line.strip_prefix("commit = \"")?.strip_suffix('"'))
        .ok_or_else(|| "tests/oracle.toml has no commit".to_owned())?;
    let actual = std::fs::read_to_string(cache.join(".git/HEAD"))
        .map_err(|error| format!("cannot inspect sway IPC oracle cache: {error}"))?;
    if actual.trim() != expected {
        return Err(format!(
            "stale oracle cache: expected {expected}, got {}; run ./contrib/fetch-oracle",
            actual.trim()
        ));
    }

    let path = cache.join("sway-ipc/fixtures").join(path);
    std::fs::read_to_string(&path).map_err(|error| {
        format!(
            "cannot read sway IPC oracle fixture {}: {error}; run ./contrib/fetch-oracle",
            path.display()
        )
    })
}

#[test]
fn stale_oracle_cache_fails_loudly() {
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("stale-oracle-cache-{}", std::process::id()));
    std::fs::create_dir_all(cache.join(".git")).unwrap();
    std::fs::write(
        cache.join(".git/HEAD"),
        "0000000000000000000000000000000000000000\n",
    )
    .unwrap();

    let error = oracle_fixture_at(&cache, "unused.json").unwrap_err();
    std::fs::remove_dir_all(cache).unwrap();

    assert!(error.starts_with("stale oracle cache:"), "{error}");
    assert!(error.ends_with("run ./contrib/fetch-oracle"), "{error}");
}

macro_rules! sway_fixture {
    ($path:literal) => {
        oracle_fixture($path)
    };
}

fn collect_focused_nodes(node: &swayward_ipc::Node, ids: &mut Vec<i64>) {
    if node.focused {
        ids.push(node.id);
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect_focused_nodes(child, ids);
    }
}

const NORMALIZED_FIXTURE_VALUE: &str = "__swayward_normalized_fixture_value__";

fn normalize_fixture_value(value: &mut Value, path: &str) {
    match value {
        Value::Object(object) => {
            let is_output = object.get("type").and_then(Value::as_str) == Some("output");
            for (key, child) in object {
                let child_path = format!("{path}.{key}");
                // IDs and process metadata are allocated independently by each run.
                if matches!(key.as_str(), "id" | "pid" | "foreign_toplevel_identifier")
                    // Focus arrays contain those dynamic node IDs. Their membership
                    // and order have a separate position-mapped comparison.
                    || key == "focus"
                    // Rectangles vary with font metrics. Dedicated comparisons
                    // retain a 10 px tolerance and verify rectangle roles.
                    || matches!(
                        key.as_str(),
                        "rect" | "deco_rect" | "window_rect" | "geometry"
                    )
                    // Output identity, modes, refresh, and adaptive-sync support
                    // depend on the nested Wayland versus headless test backend.
                    || (key == "name" && is_output)
                    || key == "output"
                    || matches!(
                        key.as_str(),
                        "make" | "model" | "serial" | "adaptive_sync_status" | "modes"
                    )
                    || child_path.ends_with(".current_mode.refresh")
                    || child_path.ends_with(".features.adaptive_sync")
                {
                    *child = Value::String(NORMALIZED_FIXTURE_VALUE.into());
                } else {
                    normalize_fixture_value(child, &child_path);
                }
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter_mut().enumerate() {
                normalize_fixture_value(child, &format!("{path}[{index}]"));
            }
        }
        _ => {}
    }
}

fn normalized_fixture_values(expected: &Value, actual: &Value, path: &str) -> (Value, Value) {
    let mut expected = expected.clone();
    let mut actual = actual.clone();
    normalize_fixture_value(&mut expected, path);
    normalize_fixture_value(&mut actual, path);
    (expected, actual)
}

fn assert_same_values(expected: &Value, actual: &Value, path: &str) {
    let (expected, actual) = normalized_fixture_values(expected, actual, path);
    assert_eq!(expected, actual, "values at {path}");
}

fn checked_scalar_paths(value: &Value, path: &str, paths: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                checked_scalar_paths(child, &format!("{path}/{key}"), paths);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                checked_scalar_paths(child, &format!("{path}/{index}"), paths);
            }
        }
        Value::String(value) if value == NORMALIZED_FIXTURE_VALUE => {}
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            paths.push(path.into())
        }
    }
}

fn checked_array_paths(value: &Value, path: &str, paths: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                checked_array_paths(child, &format!("{path}/{key}"), paths);
            }
        }
        Value::Array(values) => {
            if path.ends_with("/modes") {
                return;
            }
            paths.push(path.into());
            for (index, child) in values.iter().enumerate() {
                checked_array_paths(child, &format!("{path}/{index}"), paths);
            }
        }
        _ => {}
    }
}

fn mutate_scalar(value: &mut Value) {
    match value {
        Value::Bool(value) => *value = !*value,
        Value::Number(value) => {
            *value = serde_json::Number::from_f64(value.as_f64().unwrap() + 1.).unwrap()
        }
        Value::String(value) => value.push_str("-wrong"),
        Value::Null => *value = Value::Bool(true),
        _ => panic!("not a scalar"),
    }
}

#[test]
fn tree_fixture_comparison_rejects_every_rectangle_coordinate() {
    let original: Value = serde_json::from_str(&sway_fixture!("one_window.tree.json")).unwrap();
    let mut paths = Vec::new();
    for rectangle in ["rect", "deco_rect", "window_rect", "geometry"] {
        for coordinate in ["x", "y", "width", "height"] {
            paths.push(format!("/nodes/1/nodes/0/nodes/0/{rectangle}/{coordinate}"));
        }
    }

    for pointer in paths {
        let mut mutated = original.clone();
        *mutated.pointer_mut(&pointer).unwrap() =
            Value::from(mutated.pointer(&pointer).unwrap().as_i64().unwrap() + 100);
        let rejected = std::panic::catch_unwind(|| {
            assert_tree_rectangles_match_fixture(&original, &mutated, "$tree");
            assert_rectangle_roles_match_fixture(&original, &mutated, "$tree");
        });
        assert!(
            rejected.is_err(),
            "rectangle mutation survived at {pointer}"
        );
    }
}

#[test]
fn tree_fixture_comparators_reject_a_missing_or_extra_child() {
    type Comparator = fn(&Value, &Value, &str);
    let comparators: [(&str, Comparator); 4] = [
        ("focus", assert_focus_matches_fixture),
        ("rectangle roles", assert_rectangle_roles_match_fixture),
        ("percent", assert_percent_matches_fixture),
        ("rectangles", assert_tree_rectangles_match_fixture),
    ];
    let original: Value = serde_json::from_str(&sway_fixture!("one_floating.tree.json")).unwrap();
    let workspace = "/nodes/1/nodes/0";
    for (name, compare) in comparators {
        compare(&original, &original, "$tree");
        for key in ["nodes", "floating_nodes"] {
            let pointer = format!("{workspace}/{key}");
            let mut missing = original.clone();
            let children = missing
                .pointer_mut(&pointer)
                .unwrap()
                .as_array_mut()
                .unwrap();
            let child = children.pop().expect("fixture workspace has the child");
            let mut extra = original.clone();
            extra
                .pointer_mut(&pointer)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(child);
            for (shape, actual) in [("missing", &missing), ("extra", &extra)] {
                let rejected = std::panic::catch_unwind(|| compare(&original, actual, "$tree"));
                assert!(rejected.is_err(), "{shape} {key} child accepted by {name}");
            }
        }
    }
}

#[test]
fn normalized_fixture_comparison_rejects_every_retained_value() {
    for (path, fixture, expected_scalars, expected_arrays) in [
        ("$tree", sway_fixture!("one_window.tree.json"), 111, 18),
        (
            "$workspaces",
            sway_fixture!("one_window.workspaces.json"),
            17,
            4,
        ),
        ("$outputs", sway_fixture!("one_window.outputs.json"), 29, 4),
    ] {
        let original: Value = serde_json::from_str(&fixture).unwrap();
        let (normalized, _) = normalized_fixture_values(&original, &original, path);
        let mut paths = Vec::new();
        checked_scalar_paths(&normalized, "", &mut paths);
        let checked = paths.len();
        for pointer in paths {
            let mut mutated = original.clone();
            mutate_scalar(mutated.pointer_mut(&pointer).unwrap());
            let (expected, actual) = normalized_fixture_values(&original, &mutated, path);
            assert_ne!(
                expected, actual,
                "mutation survived normalization at {pointer}"
            );
        }
        assert_eq!(checked, expected_scalars, "checked scalar count at {path}");

        let mut array_paths = Vec::new();
        checked_array_paths(&normalized, "", &mut array_paths);
        assert_eq!(
            array_paths.len(),
            expected_arrays,
            "checked array count at {path}"
        );
        for pointer in array_paths {
            let mut mutated = original.clone();
            let array = mutated
                .pointer_mut(&pointer)
                .unwrap()
                .as_array_mut()
                .unwrap();
            if array.is_empty() {
                array.push(Value::Null);
            } else {
                array.pop();
            }
            let (expected, actual) = normalized_fixture_values(&original, &mutated, path);
            assert_ne!(
                expected, actual,
                "array length mutation survived normalization at {pointer}"
            );
        }
    }
}

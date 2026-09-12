use std::collections::BTreeSet;

use serde_json::Value;

use super::*;
use crate::ipc::tree::{describe_outputs, describe_tree, describe_workspaces};

fn assert_same_shape(expected: &Value, actual: &Value, path: &str) {
    if expected.is_null() || actual.is_null() {
        return;
    }
    assert_eq!(
        json_type(expected),
        json_type(actual),
        "JSON type at {path}"
    );
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            let expected_keys = expected.keys().collect::<BTreeSet<_>>();
            let actual_keys = actual.keys().collect::<BTreeSet<_>>();
            assert_eq!(expected_keys, actual_keys, "keys at {path}");
            if let Some(expected_type) = expected.get("type") {
                assert_eq!(
                    Some(expected_type),
                    actual.get("type"),
                    "node type at {path}"
                );
            }
            for (key, value) in expected {
                assert_same_shape(value, &actual[key], &format!("{path}.{key}"));
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if path.ends_with(".nodes") || path.ends_with(".floating_nodes") {
                assert_eq!(expected.len(), actual.len(), "array length at {path}");
                for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                    assert_same_shape(expected, actual, &format!("{path}[{index}]"));
                }
            } else if let Some(expected) = expected.first() {
                for (index, actual) in actual.iter().enumerate() {
                    assert_same_shape(expected, actual, &format!("{path}[{index}]"));
                }
            }
        }
        _ => {}
    }
}

fn assert_node_schema_appears_in_fixtures(actual: &Value, fixtures: &[Value], path: &str) {
    let actual_keys = actual.as_object().unwrap().keys().collect::<BTreeSet<_>>();
    let actual_type = &actual["type"];
    let matches = fixtures.iter().any(|fixture| {
        fixture["type"] == *actual_type
            && fixture.as_object().unwrap().keys().collect::<BTreeSet<_>>() == actual_keys
    });
    assert!(
        matches,
        "unknown {:?} key set at {path}: {actual_keys:?}",
        actual_type
    );
    for (key, children) in [
        ("nodes", &actual["nodes"]),
        ("floating_nodes", &actual["floating_nodes"]),
    ] {
        for (index, child) in children.as_array().unwrap().iter().enumerate() {
            assert_node_schema_appears_in_fixtures(
                child,
                fixtures,
                &format!("{path}.{key}[{index}]"),
            );
        }
    }
}

fn collect_fixture_nodes(value: &Value, nodes: &mut Vec<Value>) {
    nodes.push(value.clone());
    for key in ["nodes", "floating_nodes"] {
        for child in value[key].as_array().unwrap() {
            collect_fixture_nodes(child, nodes);
        }
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[test]
fn live_ipc_descriptions_match_sway_schema() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    let window = f.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.set_title("fixture-1");
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let layout = &f.swayward().layout;
    let ours = serde_json::to_value(describe_tree(layout)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.tree.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    let fixture_trees = [
        include_str!("../../tests/fixtures/sway/empty.tree.json"),
        include_str!("../../tests/fixtures/sway/empty_named.tree.json"),
        include_str!("../../tests/fixtures/sway/fullscreen.tree.json"),
        include_str!("../../tests/fixtures/sway/marked.tree.json"),
        include_str!("../../tests/fixtures/sway/named_workspace.tree.json"),
        include_str!("../../tests/fixtures/sway/nested_h_in_v.tree.json"),
        include_str!("../../tests/fixtures/sway/numbered_sparse.tree.json"),
        include_str!("../../tests/fixtures/sway/one_floating.tree.json"),
        include_str!("../../tests/fixtures/sway/one_window.tree.json"),
        include_str!("../../tests/fixtures/sway/stacked.tree.json"),
        include_str!("../../tests/fixtures/sway/tabbed.tree.json"),
        include_str!("../../tests/fixtures/sway/two_split_h.tree.json"),
        include_str!("../../tests/fixtures/sway/two_split_v.tree.json"),
        include_str!("../../tests/fixtures/sway/two_workspaces.tree.json"),
    ];
    let mut fixture_nodes = Vec::new();
    for fixture in fixture_trees {
        collect_fixture_nodes(&serde_json::from_str(fixture).unwrap(), &mut fixture_nodes);
    }
    assert_node_schema_appears_in_fixtures(&ours, &fixture_nodes, "$tree");

    let scratch = &ours["nodes"][0];
    assert_eq!(scratch["name"], "__i3");
    assert_eq!(scratch["nodes"][0]["name"], "__i3_scratch");
    assert!(ours["nodes"][1]["nodes"][0]["nodes"][0]["app_id"].is_string());

    let ours = serde_json::to_value(describe_workspaces(layout)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.workspaces.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$workspaces");

    let ours = serde_json::to_value(describe_outputs(layout)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.outputs.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$outputs");
}

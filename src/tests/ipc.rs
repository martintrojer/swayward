use std::collections::BTreeSet;

use serde_json::Value;

use super::*;
use crate::ipc::tree::{describe_outputs, describe_tree, describe_workspaces};
use crate::layout::tiling_tree::{IpcNode, Layout as TreeLayout, NodeId};

fn assert_same_shape(expected: &Value, actual: &Value, path: &str) {
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

fn assert_focus_matches_fixture(expected: &Value, actual: &Value, path: &str) {
    let expected_children = expected["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(expected["floating_nodes"].as_array().unwrap());
    let actual_children = actual["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(actual["floating_nodes"].as_array().unwrap());
    let id_map = expected_children
        .zip(actual_children)
        .map(|(expected, actual)| (expected["id"].clone(), actual["id"].clone()))
        .collect::<Vec<_>>();
    let expected_focus = expected["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            id_map
                .iter()
                .find_map(|(expected, actual)| (expected == id).then_some(actual.clone()))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        expected_focus.as_slice(),
        actual["focus"].as_array().unwrap(),
        "focus at {path}"
    );

    for key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[key].as_array().unwrap())
            .enumerate()
        {
            assert_focus_matches_fixture(expected, actual, &format!("{path}.{key}[{index}]"));
        }
    }
}

fn assert_percent_value_matches_fixture(expected: &Value, actual: &Value, path: &str) {
    match (expected["percent"].as_f64(), actual["percent"].as_f64()) {
        (Some(expected), Some(actual)) => assert!(
            (expected - actual).abs() < 1e-9,
            "percent at {path}: expected {expected}, got {actual}"
        ),
        (None, None) => {}
        _ => panic!(
            "percent at {path}: expected {}, got {}",
            expected["percent"], actual["percent"]
        ),
    }
}

fn assert_percent_matches_fixture(expected: &Value, actual: &Value, path: &str) {
    if expected["type"] == "con" && !expected["nodes"].as_array().unwrap().is_empty() {
        assert_percent_value_matches_fixture(expected, actual, path);
    }

    for key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[key].as_array().unwrap())
            .enumerate()
        {
            assert_percent_matches_fixture(expected, actual, &format!("{path}.{key}[{index}]"));
        }
    }

    let expected_children = expected["nodes"].as_array().unwrap();
    let actual_children = actual["nodes"].as_array().unwrap();
    let expected_sum = expected_children
        .iter()
        .map(|child| child["percent"].as_f64())
        .sum::<Option<f64>>();
    if expected_sum.is_some_and(|sum| (sum - 1.).abs() < 1e-9) {
        let actual_sum = actual_children
            .iter()
            .map(|child| child["percent"].as_f64())
            .sum::<Option<f64>>();
        assert!(
            actual_sum.is_some_and(|sum| (sum - 1.).abs() < 1e-9),
            "percent sum at {path}: got {actual_sum:?}"
        );
    }

    if expected["type"] != "con" || expected_children.is_empty() {
        assert_percent_value_matches_fixture(expected, actual, path);
    }
}

fn assert_fixture_string_values(expected: &Value, actual: &Value, path: &str) {
    for key in ["floating", "scratchpad_state"] {
        assert_eq!(expected[key], actual[key], "{key} at {path}");
    }
    for child_key in ["nodes", "floating_nodes"] {
        for (index, (expected, actual)) in expected[child_key]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual[child_key].as_array().unwrap())
            .enumerate()
        {
            assert_fixture_string_values(expected, actual, &format!("{path}.{child_key}[{index}]"));
        }
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

fn nested_live_tree() -> Value {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    for title in ["fixture-1", "fixture-2", "fixture-3"] {
        let window = f.client(id).create_window();
        window.xdg_toplevel.set_app_id(title.into());
        window.set_title(title);
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(id);
        let window = f.client(id).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(id);
    }
    f.swayward().layout.consume_or_expel_window_left(None);
    let swayward = f.swayward();
    serde_json::to_value(describe_tree(&swayward.layout, &swayward.global_space)).unwrap()
}

fn nested_fixture_tree() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/nested_h_in_v.tree.json"
    ))
    .unwrap()
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

    let swayward = f.swayward();
    let layout = &swayward.layout;
    let ours = serde_json::to_value(describe_tree(layout, &swayward.global_space)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.tree.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$tree");
    assert_fixture_string_values(&fixture, &ours, "$tree");
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

    let ours = serde_json::to_value(describe_workspaces(layout, &swayward.global_space)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.workspaces.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$workspaces");

    let ours = serde_json::to_value(describe_outputs(layout, &swayward.global_space)).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/sway/one_window.outputs.json"
    ))
    .unwrap();
    assert_same_shape(&fixture, &ours, "$outputs");
}

#[test]
fn ipc_output_rects_use_global_positions() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let swayward = f.swayward();
    let outputs = describe_outputs(&swayward.layout, &swayward.global_space);
    let rects = outputs.iter().map(|output| output.rect).collect::<Vec<_>>();
    assert_eq!(rects[0].x, 0);
    assert_eq!(rects[0].width, 1280);
    assert_eq!(rects[1].x, 1280);
    assert_eq!(rects[1].width, 1920);

    let root = describe_tree(&swayward.layout, &swayward.global_space);
    assert_eq!(root.rect.width, 3200);
    assert_eq!(root.rect.height, 1080);
}

#[test]
fn stale_tree_leaf_is_omitted_without_panicking() {
    let tree = IpcNode::Leaf {
        id: NodeId(1),
        window: (),
        percent: Some(1.),
        rect: Default::default(),
    };
    assert!(crate::ipc::tree::describe_tiling(tree, &|_| None, Default::default()).is_none());

    let tree = IpcNode::Split {
        id: NodeId(0),
        layout: TreeLayout::SplitH,
        percent: None,
        focus: vec![NodeId(1)],
        children: vec![IpcNode::Leaf {
            id: NodeId(1),
            window: (),
            percent: Some(1.),
            rect: Default::default(),
        }],
    };
    let node = crate::ipc::tree::describe_tiling(tree, &|_| None, Default::default()).unwrap();
    assert!(node.nodes.is_empty());
}

#[test]
fn live_ipc_focus_matches_sway_mru_arrays() {
    assert_focus_matches_fixture(&nested_fixture_tree(), &nested_live_tree(), "$tree");
}

#[test]
fn live_ipc_percent_matches_sway_parent_shares() {
    let expected = nested_fixture_tree();
    let actual = nested_live_tree();
    assert_percent_value_matches_fixture(
        &expected["nodes"][1]["nodes"][0]["nodes"][1],
        &actual["nodes"][1]["nodes"][0]["nodes"][1],
        "$tree.nodes[1].nodes[0].nodes[1]",
    );
    assert_percent_matches_fixture(&expected, &actual, "$tree");
}

#[test]
fn workspace_output_assignment_moves_the_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let target_output = f.niri_output(2).name();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    let command = format!("workspace 7 output {target_output}");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    let swayward = f.swayward();
    let workspace = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.output, target_output);
}

/// Sway accepts a LIST of outputs and uses the first that resolves
/// (`sway/sway/commands/workspace.c:153-155`;
/// `sway/sway/tree/workspace.c:244-250`). The list must not be joined into one
/// name, which is what a single-output parser would do.
#[test]
fn workspace_output_assignment_uses_the_first_available_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));
    let present = f.niri_output(2).name();

    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    // First two names do not resolve, so the third wins.
    let command = format!("workspace 7 output missing-a missing-b {present}");
    assert!(crate::command::execute(f.niri_state(), &command)[0].success);

    let swayward = f.swayward();
    let workspace = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .find(|workspace| workspace.num == 7)
        .unwrap();
    assert_eq!(workspace.output, present);
}

/// A list naming only absent outputs resolves to nothing and must fail rather
/// than silently doing nothing.
#[test]
fn workspace_output_assignment_fails_when_no_output_resolves() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 720));
    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);

    let result =
        &crate::command::execute(f.niri_state(), "workspace 7 output missing-a missing-b")[0];
    assert!(!result.success);
}

#[test]
fn ipc_output_reports_sways_clockwise_transform_names() {
    for (transform, expected) in [
        (smithay::utils::Transform::Normal, "normal"),
        (smithay::utils::Transform::_90, "270"),
        (smithay::utils::Transform::_180, "180"),
        (smithay::utils::Transform::_270, "90"),
        (smithay::utils::Transform::Flipped, "flipped"),
        (smithay::utils::Transform::Flipped90, "flipped-270"),
        (smithay::utils::Transform::Flipped180, "flipped-180"),
        (smithay::utils::Transform::Flipped270, "flipped-90"),
    ] {
        assert_eq!(crate::ipc::tree::sway_transform(transform), expected);
    }
}

#[test]
fn get_outputs_reads_the_live_transform_and_subpixel_layout() {
    let (mut f, socket) = ipc_fixture();
    let output = smithay::output::Output::new(
        "test-output".into(),
        smithay::output::PhysicalProperties {
            size: (0, 0).into(),
            subpixel: smithay::output::Subpixel::HorizontalRgb,
            make: "test".into(),
            model: "test".into(),
            serial_number: "test".into(),
        },
    );
    let mode = smithay::output::Mode {
        size: (1280, 720).into(),
        refresh: 60_000,
    };
    output.change_current_state(Some(mode), None, None, None);
    output.set_preferred(mode);
    output.user_data().insert_if_missing(|| OutputName {
        connector: "test-output".into(),
        make: Some("test".into()),
        model: Some("test".into()),
        serial: Some("test".into()),
    });
    f.swayward().add_output(output.clone(), None, false);
    output.change_current_state(None, Some(smithay::utils::Transform::_90), None, None);
    f.niri_state().ipc_refresh_layout();

    let mut stream = UnixStream::connect(socket).unwrap();
    let outputs = query_ipc(&mut f, &mut stream, MessageType::GetOutputs);
    assert_eq!(outputs[0]["transform"], "270");
    assert_eq!(outputs[0]["subpixel_hinting"], "rgb");
}

#[test]
fn get_tree_ids_are_unique_across_workspaces() {
    fn collect_ids(node: &swayward_ipc::Node, ids: &mut Vec<i64>) {
        ids.push(node.id);
        for child in node.nodes.iter().chain(&node.floating_nodes) {
            collect_ids(child, ids);
        }
    }

    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    for workspace in ["1", "2"] {
        assert!(
            crate::command::execute(f.niri_state(), &format!("workspace {workspace}"))[0].success
        );
        for index in 0..3 {
            let window = f.client(client).create_window();
            window.commit();
            let surface = window.surface.clone();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
            if index == 1 {
                assert!(crate::command::execute(f.niri_state(), "split vertical")[0].success);
            }
        }
    }

    let swayward = f.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let mut ids = Vec::new();
    collect_ids(&tree, &mut ids);
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

#[test]
fn ipc_output_reports_sways_subpixel_names() {
    for (subpixel, expected) in [
        (smithay::output::Subpixel::Unknown, "unknown"),
        (smithay::output::Subpixel::None, "none"),
        (smithay::output::Subpixel::HorizontalRgb, "rgb"),
        (smithay::output::Subpixel::HorizontalBgr, "bgr"),
        (smithay::output::Subpixel::VerticalRgb, "vrgb"),
        (smithay::output::Subpixel::VerticalBgr, "vbgr"),
    ] {
        assert_eq!(crate::ipc::tree::sway_subpixel_hinting(subpixel), expected);
    }
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
    assert_eq!(outputs[0].percent, Some((1280. * 720.) / (3200. * 1080.)));
    assert_eq!(outputs[1].percent, Some((1920. * 1080.) / (3200. * 1080.)));

    let root = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(root.rect.width, 3200);
    assert_eq!(root.rect.height, 1080);
    assert_eq!(
        root.nodes[1].percent,
        Some((1280. * 720.) / (3200. * 1080.))
    );
    assert_eq!(
        root.nodes[2].percent,
        Some((1920. * 1080.) / (3200. * 1080.))
    );
}

#[test]
fn configured_startup_outputs_use_reported_rects_for_percentages() {
    let config = swayward_config::Config::parse_mem(
        r#"
        output "headless-1" { mode custom=true "1270x1408@60"; scale 1; }
        output "headless-2" { mode custom=true "1270x1408@60"; scale 1; }
        "#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    // The headless backend announces pre-created outputs in reverse order.
    f.add_output(2, (1280, 720));
    f.add_output(1, (1280, 720));

    let swayward = f.swayward();
    let outputs = describe_outputs(&swayward.layout, &swayward.global_space);
    let first = outputs
        .iter()
        .find(|output| output.name == "headless-1")
        .unwrap();
    let second = outputs
        .iter()
        .find(|output| output.name == "headless-2")
        .unwrap();
    assert_eq!(first.rect.x, 0);
    assert_eq!(first.rect.width, 1270);
    assert_eq!(second.rect.x, 1270);
    assert_eq!(second.rect.width, 1270);
    assert_eq!(first.percent, Some(0.5));
    assert_eq!(second.percent, Some(0.5));

    let root = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    );
    assert_eq!(root.nodes[1].percent, Some(0.5));
    assert_eq!(root.nodes[2].percent, Some(0.5));
}

#[test]
fn stale_tree_leaf_is_omitted_without_panicking() {
    let tree = IpcNode::Leaf {
        id: NodeId(1),
        window: (),
        percent: Some(1.),
        focused: false,
        fullscreen_mode: 0,
        rect: Default::default(),
        deco_rect: None,
        border: (swayward_ipc::command::BorderStyle::Normal, 2),
        border_edges: crate::utils::ResizeEdge::all(),
        sticky: false,
        mapped_under_fullscreen: false,
    };
    assert!(crate::ipc::tree::describe_tiling(
        tree,
        &|_| None,
        Default::default(),
        &Default::default(),
        &Default::default(),
        crate::layout::workspace::WorkspaceId::specific(1)
    )
    .is_none());

    let tree = IpcNode::Split {
        id: NodeId(0),
        layout: TreeLayout::SplitH,
        title: None,
        percent: None,
        rect: Default::default(),
        focus: vec![NodeId(1)],
        focused: false,
        fullscreen_mode: 0,
        sticky: false,
        children: vec![IpcNode::Leaf {
            id: NodeId(1),
            window: (),
            percent: Some(1.),
            focused: false,
            fullscreen_mode: 0,
            rect: Default::default(),
            deco_rect: None,
            border: (swayward_ipc::command::BorderStyle::Normal, 2),
            border_edges: crate::utils::ResizeEdge::all(),
            sticky: false,
            mapped_under_fullscreen: false,
        }],
    };
    let node = crate::ipc::tree::describe_tiling(
        tree,
        &|_| None,
        Default::default(),
        &Default::default(),
        &Default::default(),
        crate::layout::workspace::WorkspaceId::specific(1),
    )
    .unwrap();
    assert!(node.nodes.is_empty());
}

#[test]
fn live_ipc_focus_matches_sway_mru_arrays() {
    assert_focus_matches_fixture(&nested_fixture_tree(), &nested_live_tree(), "$tree");
}

#[test]
fn workspace_focus_spans_tiled_and_floating_children() {
    let expected = mixed_fixture_tree();
    let actual = mixed_live_tree();
    assert_focus_matches_fixture(&expected, &actual, "$tree");

    let workspace = &actual["nodes"][1]["nodes"][0];
    assert_eq!(workspace["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(workspace["floating_nodes"].as_array().unwrap().len(), 1);
    assert_eq!(workspace["focus"].as_array().unwrap().len(), 2);
}

#[test]
fn newly_focused_tiled_window_precedes_floating_children_in_workspace_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    map_test_window(&mut f, client, "first");
    map_test_window(&mut f, client, "floating");
    assert!(crate::command::execute(f.niri_state(), "floating enable")[0].success);
    map_test_window(&mut f, client, "newest");

    let swayward = f.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &Default::default(),
        &Default::default(),
    ))
    .unwrap();
    let workspace = &tree["nodes"][1]["nodes"][0];
    let children = workspace["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .chain(workspace["floating_nodes"].as_array().unwrap());
    let focus = workspace["focus"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            children
                .clone()
                .find(|node| node["id"] == *id)
                .unwrap()["app_id"]
                .as_str()
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(focus, ["newest", "floating", "first"]);
}

fn floating_order(tree: &Value) -> (Vec<&str>, Vec<&str>) {
    let workspace = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"] != "__i3")
        .unwrap()["nodes"][0]
        .as_object()
        .unwrap();
    let floating = workspace["floating_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["app_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let focus = workspace["focus"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|id| {
            workspace["floating_nodes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["id"] == *id)
                .and_then(|node| node["app_id"].as_str())
        })
        .collect::<Vec<_>>();
    (floating, focus)
}

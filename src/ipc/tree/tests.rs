use super::*;

fn split(layout: NodeLayout, nodes: Vec<Node>) -> Node {
    common_node(CommonNodeContext {
        id: 0,
        node_type: NodeType::Con,
        layout,
        orientation: "none",
        name: None,
        rect: Rect::default(),
        nodes,
        floating_nodes: vec![],
        focus: vec![],
        focused: false,
        properties: NodeProperties::None {},
    })
}

fn view(app_id: Option<&str>) -> Node {
    let mut node = split(NodeLayout::None, vec![]);
    node.properties = NodeProperties::View(ViewProperties {
        allow_tearing: false,
        app_id: app_id.map(Into::into),
        foreign_toplevel_identifier: None,
        idle_inhibitors: IdleInhibitors {
            application: "none".into(),
            user: "none".into(),
        },
        inhibit_idle: false,
        max_render_time: 0,
        pid: None,
        sandbox_app_id: None,
        sandbox_engine: None,
        sandbox_instance_id: None,
        shell: None,
        tag: None,
        visible: true,
    });
    node
}

/// Sway's `container_build_representation` (`sway/sway/tree/container.c:
/// 702-745`): one letter per layout, a view by its identifier or "(null)",
/// and a split child by its title, which defaults to its own representation
/// (`parse_title_format`, :632-638). The empty-split row pins swayward's
/// choice; sway rarely keeps an empty split long enough to report one.
#[test]
fn representation_follows_sways_container_build_representation() {
    let cases = [
        (NodeLayout::SplitH, vec![], "H[]"),
        (NodeLayout::SplitV, vec![], "V[]"),
        (
            NodeLayout::SplitH,
            vec![
                view(Some("a")),
                split(NodeLayout::SplitV, vec![view(Some("b")), view(Some("c"))]),
            ],
            "H[a V[b c]]",
        ),
        (
            NodeLayout::Tabbed,
            vec![
                view(Some("a")),
                split(NodeLayout::Stacked, vec![view(Some("b"))]),
            ],
            "T[a S[b]]",
        ),
        (NodeLayout::SplitH, vec![view(None)], "H[(null)]"),
        (
            NodeLayout::SplitV,
            vec![split(NodeLayout::SplitH, vec![])],
            "V[(null)]",
        ),
        (NodeLayout::None, vec![view(Some("a"))], "D[a]"),
    ];
    for (layout, children, expected) in cases {
        assert_eq!(tree_representation(layout, &children), expected);
    }
}

/// Every id namespace stays inside its own block, so an output, workspace,
/// container and window can never share a wire id, and none can reach the
/// root or scratch ids.
#[test]
fn id_namespaces_are_disjoint() {
    let block = |id: i64| id.div_euclid(ID_NAMESPACE_SIZE);
    for name in ["HEADLESS-1", "headless-1", "DP-1", "HDMI-A-2", "eDP-1"] {
        assert_eq!(block(output_id(name)), 1, "{name}");
    }
    for raw in [0, 1, 99_999_999] {
        assert_eq!(block(workspace_id(raw)), 2);
        assert_eq!(block(container_id(NodeId(raw))), 3);
        assert_eq!(block(window_id_from_raw(raw)), 4);
    }
    for reserved in [ROOT_ID, SCRATCH_OUTPUT_ID, SCRATCH_WORKSPACE_ID] {
        assert!(!(1..=4).contains(&block(reserved)), "{reserved}");
    }
}

#[test]
fn fullscreen_descendants_keep_percentages() {
    let leaf = |id, percent, fullscreen_mode| {
        let mut node = common_node(CommonNodeContext {
            id,
            node_type: NodeType::Con,
            layout: NodeLayout::None,
            orientation: "none",
            name: None,
            rect: Rect::default(),
            nodes: vec![],
            floating_nodes: vec![],
            focus: vec![],
            focused: false,
            properties: NodeProperties::None {},
        });
        node.percent = percent;
        node.fullscreen_mode = fullscreen_mode;
        node
    };
    let mut branch = common_node(CommonNodeContext {
        id: 1,
        node_type: NodeType::Con,
        layout: NodeLayout::SplitH,
        orientation: "horizontal",
        name: None,
        rect: Rect::default(),
        nodes: vec![leaf(2, Some(0.4), 0), leaf(3, Some(0.6), 1)],
        floating_nodes: vec![],
        focus: vec![],
        focused: false,
        properties: NodeProperties::None {},
    });
    branch.percent = Some(0.5);
    let mut nodes = vec![branch, leaf(4, Some(0.5), 0)];

    assert!(apply_fullscreen_state(&mut nodes, true));
    assert_eq!(nodes[0].percent, Some(0.5));
    assert_eq!(nodes[0].nodes[0].percent, Some(0.4));
    assert_eq!(nodes[0].nodes[1].percent, Some(0.6));
}

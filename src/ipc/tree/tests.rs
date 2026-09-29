use super::*;

#[test]
fn representation_letters_match_split_orientation() {
    assert_eq!(tree_representation(NodeLayout::SplitH, &[]), "H[]");
    assert_eq!(tree_representation(NodeLayout::SplitV, &[]), "V[]");
}

#[test]
fn ipc_border_is_total_for_command_only_styles() {
    assert_eq!(
        ipc_border(swayward_ipc::command::BorderStyle::Toggle),
        NodeBorder::None
    );
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

use super::*;

pub(super) struct WindowNodeContext<'a> {
    pub(super) mapped: &'a Mapped,
    pub(super) rect: Rect,
    pub(super) node_type: NodeType,
    pub(super) floating: &'a str,
    pub(super) parent: Option<Rect>,
    pub(super) marks: &'a std::collections::HashMap<MappedId, Vec<String>>,
    pub(super) in_scratchpad: bool,
    pub(super) visible: bool,
}

pub(super) struct CommonNodeContext<'a> {
    pub(super) id: i64,
    pub(super) node_type: NodeType,
    pub(super) layout: NodeLayout,
    pub(super) orientation: &'a str,
    pub(super) name: Option<&'a str>,
    pub(super) rect: Rect,
    pub(super) nodes: Vec<Node>,
    pub(super) floating_nodes: Vec<Node>,
    pub(super) focus: Vec<i64>,
    pub(super) focused: bool,
    pub(super) properties: NodeProperties,
}

pub(crate) fn describe_tiling<'a, I>(
    node: IpcNode<I>,
    find_window: &impl Fn(&I) -> Option<&'a Mapped>,
    workspace_rect: Rect,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<crate::layout::tiling_tree::NodeId, Vec<String>>,
) -> Option<Node> {
    match node {
        IpcNode::Split {
            id,
            layout,
            title: _,
            percent,
            rect,
            focus,
            focused,
            fullscreen_mode,
            sticky,
            children,
        } => {
            let children = children
                .into_iter()
                .filter_map(|child| {
                    let id = match &child {
                        IpcNode::Split { id, .. } | IpcNode::Leaf { id, .. } => *id,
                    };
                    describe_tiling(child, find_window, workspace_rect, marks, container_marks)
                        .map(|node| (id, node))
                })
                .collect::<Vec<_>>();
            let focus = focus
                .iter()
                .filter_map(|id| {
                    children
                        .iter()
                        .find_map(|(child_id, node)| (child_id == id).then_some(node.id))
                })
                .collect();
            let children = children.into_iter().map(|(_, node)| node).collect();
            let mut node = common_node(CommonNodeContext {
                id: container_id(id),
                node_type: NodeType::Con,
                layout: ipc_layout(layout),
                orientation: orientation(layout),
                name: None,
                rect: offset_rect(rect, workspace_rect),
                nodes: children,
                floating_nodes: vec![],
                focus,
                focused,
                properties: NodeProperties::None {},
            });
            node.floating = Some("auto_off".into());
            node.percent = percent;
            node.scratchpad_state = Some("none".into());
            node.fullscreen_mode = fullscreen_mode;
            node.sticky = sticky;
            node.marks = container_marks.get(&id).cloned().unwrap_or_default();
            Some(node)
        }
        IpcNode::Leaf {
            window,
            percent,
            focused,
            fullscreen_mode,
            rect,
            deco_rect,
            border,
            border_edges,
            sticky,
            mapped_under_fullscreen,
            ..
        } => {
            let Some(mapped) = find_window(&window) else {
                warn!("omitting stale tree leaf from IPC output");
                return None;
            };
            let mut node = describe_window(WindowNodeContext {
                mapped,
                rect: offset_rect(rect, workspace_rect),
                node_type: NodeType::Con,
                floating: "auto_off",
                parent: None,
                marks,
                in_scratchpad: false,
                visible: true,
            });
            node.border = ipc_border(border.0);
            node.current_border_width = ipc_border_width(border);
            if mapped_under_fullscreen {
                node.border = NodeBorder::None;
                node.current_border_width = 0;
                node.percent = Some(0.);
                node.rect = Rect::default();
            } else {
                node.percent = percent;
            }
            node.focused = focused;
            node.fullscreen_mode = fullscreen_mode;
            node.sticky = sticky;
            let has_titlebar = deco_rect.is_some() && fullscreen_mode == 0;
            node.deco_rect = if fullscreen_mode != 0 {
                Rect::default()
            } else {
                deco_rect.map_or_else(Rect::default, |rect| {
                    rect_from(rect.loc.x, rect.loc.y, rect.size.w, rect.size.h)
                })
            };
            let border_width = match (node.border, has_titlebar) {
                (NodeBorder::Normal | NodeBorder::Pixel, true) | (NodeBorder::Pixel, false) => {
                    node.current_border_width
                }
                _ => 0,
            };
            let left = border_width * i32::from(border_edges.contains(ResizeEdge::LEFT));
            let right = border_width * i32::from(border_edges.contains(ResizeEdge::RIGHT));
            let top = if has_titlebar {
                0
            } else {
                border_width * i32::from(border_edges.contains(ResizeEdge::TOP))
            };
            let bottom = border_width * i32::from(border_edges.contains(ResizeEdge::BOTTOM));
            if node.rect.width == 0
                && node.rect.height == 0
                && fullscreen_mode != 0
                && percent.is_none()
            {
                node.window_rect = Rect {
                    width: workspace_rect.width,
                    height: workspace_rect.height,
                    ..Rect::default()
                };
            } else if fullscreen_mode != 0 && node.rect.width > 0 && node.rect.height > 0 {
                // A fullscreen view's content is the output box, even while
                // its container reports a tiled slot (`view_autoconfigure`,
                // sway/tree/view.c:358-363).
                node.window_rect = Rect {
                    x: workspace_rect.x - node.rect.x,
                    y: workspace_rect.y - node.rect.y,
                    width: workspace_rect.width,
                    height: workspace_rect.height,
                };
            } else {
                node.window_rect = Rect {
                    x: left,
                    y: top,
                    width: (node.rect.width - left - right).max(0),
                    height: (node.rect.height - top - bottom).max(0),
                };
            }
            Some(node)
        }
    }
}

pub(super) fn empty_tiling_node(rect: Rect) -> Node {
    common_node(CommonNodeContext {
        id: 0,
        node_type: NodeType::Con,
        layout: NodeLayout::SplitH,
        orientation: "horizontal",
        name: None,
        rect,
        nodes: vec![],
        floating_nodes: vec![],
        focus: vec![],
        focused: false,
        properties: NodeProperties::None {},
    })
}

pub(super) fn ipc_border_width(border: (swayward_ipc::command::BorderStyle, u16)) -> i32 {
    i32::from(border.1)
}

pub(super) fn ipc_border(style: swayward_ipc::command::BorderStyle) -> NodeBorder {
    match style {
        swayward_ipc::command::BorderStyle::Normal => NodeBorder::Normal,
        swayward_ipc::command::BorderStyle::None => NodeBorder::None,
        swayward_ipc::command::BorderStyle::Pixel => NodeBorder::Pixel,
        swayward_ipc::command::BorderStyle::Csd => NodeBorder::Csd,
        // Toggle is an operation, not a state. Treat an invalid stored value as
        // no border rather than panicking on the live GET_TREE path.
        swayward_ipc::command::BorderStyle::Toggle => NodeBorder::None,
    }
}

pub(super) fn describe_window(context: WindowNodeContext<'_>) -> Node {
    let WindowNodeContext {
        mapped,
        rect,
        node_type,
        floating,
        parent,
        marks,
        in_scratchpad,
        visible,
    } = context;
    let properties = with_toplevel_role(mapped.toplevel(), |role| ViewProperties {
        allow_tearing: false,
        app_id: role.app_id.clone(),
        foreign_toplevel_identifier: Some(mapped.id().to_protocol_identifier()),
        idle_inhibitors: IdleInhibitors {
            application: "none".into(),
            user: "none".into(),
        },
        inhibit_idle: false,
        max_render_time: 0,
        pid: mapped
            .credentials()
            .map(|credentials| i64::from(credentials.pid)),
        sandbox_app_id: None,
        sandbox_engine: None,
        sandbox_instance_id: None,
        shell: Some("xdg_shell".into()),
        tag: None,
        visible,
    });
    let title = with_toplevel_role(mapped.toplevel(), |role| role.title.clone());
    let percent = parent.and_then(|parent| {
        (parent.width != 0 && parent.height != 0).then(|| {
            f64::from(rect.width) / f64::from(parent.width) * f64::from(rect.height)
                / f64::from(parent.height)
        })
    });
    let mut node = common_node(CommonNodeContext {
        id: window_id(mapped.id()),
        node_type,
        layout: NodeLayout::None,
        orientation: "none",
        name: title.as_deref(),
        rect,
        nodes: vec![],
        floating_nodes: vec![],
        focus: vec![],
        focused: mapped.is_focused(),
        properties: NodeProperties::View(properties),
    });
    node.border = NodeBorder::Normal;
    node.current_border_width = 2;
    node.floating = Some(floating.into());
    node.percent = percent;
    node.scratchpad_state = Some(if in_scratchpad { "fresh" } else { "none" }.into());
    node.fullscreen_mode = i32::from(mapped.pending_sizing_mode().is_fullscreen());
    node.urgent = mapped.is_urgent();
    let natural_size = mapped.natural_size();
    node.geometry = rect_from(0., 0., natural_size.w.into(), natural_size.h.into());
    node.marks = marks.get(&mapped.id()).cloned().unwrap_or_default();
    node.window_rect = Rect {
        x: 2,
        y: 0,
        width: (rect.width - 4).max(0),
        height: (rect.height - 2).max(0),
    };
    node
}

pub(super) fn common_node(context: CommonNodeContext<'_>) -> Node {
    let CommonNodeContext {
        id,
        node_type,
        layout,
        orientation,
        name,
        rect,
        nodes,
        floating_nodes,
        focus,
        focused,
        properties,
    } = context;
    Node {
        border: NodeBorder::None,
        current_border_width: 0,
        deco_rect: Rect::default(),
        floating: None,
        floating_nodes,
        focus,
        focused,
        fullscreen_mode: 0,
        geometry: Rect::default(),
        id,
        layout,
        marks: vec![],
        name: name.map(Into::into),
        nodes,
        orientation: orientation.into(),
        percent: None,
        rect,
        scratchpad_state: None,
        sticky: false,
        node_type,
        urgent: false,
        window: None,
        window_rect: Rect::default(),
        properties,
    }
}

pub(super) fn tree_representation(layout: NodeLayout, children: &[Node]) -> String {
    let prefix = match layout {
        NodeLayout::SplitH => 'H',
        NodeLayout::SplitV => 'V',
        NodeLayout::Tabbed => 'T',
        NodeLayout::Stacked => 'S',
        _ => 'D',
    };
    let children = children
        .iter()
        .map(|child| {
            if let NodeProperties::View(properties) = &child.properties {
                properties.app_id.clone().unwrap_or_else(|| "(null)".into())
            } else if child.nodes.is_empty() {
                "(null)".to_owned()
            } else {
                tree_representation(child.layout, &child.nodes)
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{prefix}[{children}]")
}

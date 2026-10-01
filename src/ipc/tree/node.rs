use super::*;

pub(super) struct WindowNodeContext<'a> {
    pub(super) mapped: &'a Mapped,
    pub(super) rect: Rect,
    pub(super) border: NodeBorder,
    pub(super) border_width: i32,
    pub(super) window_rect: Rect,
    pub(super) node_type: NodeType,
    pub(super) floating: &'a str,
    pub(super) parent: Option<Rect>,
    pub(super) marks: &'a std::collections::HashMap<MappedId, Vec<String>>,
    pub(super) in_scratchpad: bool,
    pub(super) visible: bool,
}

/// A view's container box and the border around its content.
pub(super) struct ViewFrame {
    pub(super) rect: Rect,
    pub(super) border: NodeBorder,
    pub(super) border_width: i32,
    pub(super) has_titlebar: bool,
    /// The sides that draw a border; floating views draw all four.
    pub(super) edges: ResizeEdge,
}

impl ViewFrame {
    /// The content box relative to the container, as sway reports it in
    /// `window_rect`: offset by the border, with y at 0 under a titlebar
    /// (`sway/sway/ipc-json.c:595-602`).
    pub(super) fn window_rect(&self) -> Rect {
        let border_width = match (self.border, self.has_titlebar) {
            (NodeBorder::Normal | NodeBorder::Pixel, true) | (NodeBorder::Pixel, false) => {
                self.border_width
            }
            _ => 0,
        };
        let side = |edge| border_width * i32::from(self.edges.contains(edge));
        let left = side(ResizeEdge::LEFT);
        let right = side(ResizeEdge::RIGHT);
        let top = if self.has_titlebar {
            0
        } else {
            side(ResizeEdge::TOP)
        };
        let bottom = side(ResizeEdge::BOTTOM);
        Rect {
            x: left,
            y: top,
            width: (self.rect.width - left - right).max(0),
            height: (self.rect.height - top - bottom).max(0),
        }
    }
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
            let has_titlebar = deco_rect.is_some() && fullscreen_mode == 0;
            // A view hidden under a fullscreen container has no box or border.
            let frame = if mapped_under_fullscreen {
                ViewFrame {
                    rect: Rect::default(),
                    border: NodeBorder::None,
                    border_width: 0,
                    has_titlebar,
                    edges: border_edges,
                }
            } else {
                ViewFrame {
                    rect: offset_rect(rect, workspace_rect),
                    border: ipc_border(border.0),
                    border_width: ipc_border_width(border),
                    has_titlebar,
                    edges: border_edges,
                }
            };
            let window_rect = if frame.rect.width == 0
                && frame.rect.height == 0
                && fullscreen_mode != 0
                && percent.is_none()
            {
                Rect {
                    width: workspace_rect.width,
                    height: workspace_rect.height,
                    ..Rect::default()
                }
            } else if fullscreen_mode != 0 && frame.rect.width > 0 && frame.rect.height > 0 {
                // A fullscreen view's content is the output box, even while
                // its container reports a tiled slot (`view_autoconfigure`,
                // sway/tree/view.c:358-363).
                Rect {
                    x: workspace_rect.x - frame.rect.x,
                    y: workspace_rect.y - frame.rect.y,
                    width: workspace_rect.width,
                    height: workspace_rect.height,
                }
            } else {
                frame.window_rect()
            };
            let mut node = describe_window(WindowNodeContext {
                mapped,
                rect: frame.rect,
                border: frame.border,
                border_width: frame.border_width,
                window_rect,
                node_type: NodeType::Con,
                floating: "auto_off",
                parent: None,
                marks,
                in_scratchpad: false,
                visible: true,
            });
            node.percent = if mapped_under_fullscreen {
                Some(0.)
            } else {
                percent
            };
            node.focused = focused;
            node.fullscreen_mode = fullscreen_mode;
            node.sticky = sticky;
            node.deco_rect = match deco_rect {
                Some(rect) if fullscreen_mode == 0 => {
                    rect_from(rect.loc.x, rect.loc.y, rect.size.w, rect.size.h)
                }
                _ => Rect::default(),
            };
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
        border,
        border_width,
        window_rect,
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
    node.border = border;
    node.current_border_width = border_width;
    node.window_rect = window_rect;
    node.floating = Some(floating.into());
    node.percent = percent;
    node.scratchpad_state = Some(if in_scratchpad { "fresh" } else { "none" }.into());
    node.fullscreen_mode = i32::from(mapped.pending_sizing_mode().is_fullscreen());
    node.urgent = mapped.is_urgent();
    let natural_size = mapped.natural_size();
    node.geometry = rect_from(0., 0., natural_size.w.into(), natural_size.h.into());
    node.marks = marks.get(&mapped.id()).cloned().unwrap_or_default();
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

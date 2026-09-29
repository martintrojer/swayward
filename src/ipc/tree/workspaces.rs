use super::*;

pub(super) struct WorkspaceNodeContext<'a> {
    pub(super) compositor_layout: &'a Layout<Mapped>,
    pub(super) workspace: &'a crate::layout::workspace::Workspace<Mapped>,
    pub(super) output: &'a str,
    pub(super) index: usize,
    pub(super) rect: Rect,
    pub(super) output_origin: Rect,
    pub(super) marks: &'a std::collections::HashMap<MappedId, Vec<String>>,
    pub(super) container_marks: &'a std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
}

pub fn describe_workspaces(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
) -> Vec<Workspace> {
    describe_workspaces_with_marks(
        layout,
        global_space,
        &Default::default(),
        &Default::default(),
    )
}

pub(crate) fn describe_workspaces_with_marks(
    layout: &Layout<Mapped>,
    global_space: &Space<Window>,
    marks: &std::collections::HashMap<MappedId, Vec<String>>,
    container_marks: &std::collections::HashMap<(WorkspaceId, NodeId), Vec<String>>,
) -> Vec<Workspace> {
    layout
        .monitors()
        .flat_map(|monitor| {
            monitor
                .sway_workspaces()
                .map(move |(index, workspace)| (monitor, index, workspace))
        })
        .filter_map(|(monitor, index, workspace)| {
            if !workspace.must_be_kept() && monitor.active_workspace_ref().id() != workspace.id() {
                return None;
            }
            let workspace_focused = layout
                .active_monitor_ref()
                .is_some_and(|active| active.output() == monitor.output())
                && monitor.active_workspace_idx() == index;
            let Node {
                border,
                current_border_width,
                deco_rect,
                floating,
                floating_nodes,
                focus,
                focused: _,
                fullscreen_mode,
                geometry,
                id,
                layout,
                marks,
                name,
                orientation,
                percent,
                rect,
                scratchpad_state,
                sticky,
                node_type,
                urgent,
                window,
                window_rect,
                properties,
                ..
            } = describe_workspace_node(WorkspaceNodeContext {
                compositor_layout: layout,
                workspace,
                output: monitor.output_name(),
                index,
                rect: workspace_rect(global_space, monitor.output(), workspace),
                output_origin: output_rect(global_space, monitor.output()),
                marks,
                container_marks,
            });
            let NodeProperties::Workspace(properties) = properties else {
                unreachable!()
            };
            Some(Workspace {
                border,
                current_border_width,
                deco_rect,
                floating,
                floating_nodes,
                focus,
                focused: workspace_focused,
                fullscreen_mode,
                geometry,
                id,
                layout,
                marks,
                name: name.unwrap_or_default(),
                nodes: vec![],
                num: properties.num,
                orientation,
                output: properties.output,
                percent,
                rect,
                representation: properties.representation,
                scratchpad_state,
                sticky,
                node_type,
                urgent,
                visible: monitor.active_workspace_idx() == index,
                window,
                window_rect,
            })
        })
        .collect()
}

pub(super) fn describe_workspace_node(context: WorkspaceNodeContext<'_>) -> Node {
    let WorkspaceNodeContext {
        compositor_layout,
        workspace,
        output,
        index,
        rect,
        output_origin,
        marks,
        container_marks,
    } = context;
    // Layout geometry is output-relative and already carries the working-area
    // origin: tiled leaves come from `parent_area`, which is the gap-inset
    // working area, and floating positions come from `scale_by_working_area`.
    // Children are therefore translated by the output origin, never by the
    // workspace rect, which would add the exclusive zone and the gaps twice.
    // Only the workspace node itself is reported at `rect`.
    let mut tiled = describe_tiling(
        workspace.ipc_tiling_tree(),
        &|window| workspace.windows().find(|mapped| mapped.window == *window),
        output_origin,
        marks,
        container_marks,
        workspace.id(),
    )
    .unwrap_or_else(|| empty_tiling_node(rect));
    let workspace_focused = compositor_layout
        .active_monitor_ref()
        .is_some_and(|monitor| {
            monitor.output_name() == output && monitor.active_workspace_ref().id() == workspace.id()
        });
    // Visibility is per output, not per seat: the active workspace of every
    // output is on screen, while only one of them holds keyboard focus.
    let workspace_visible = compositor_layout.monitors().any(|monitor| {
        monitor.output_name() == output && monitor.active_workspace_ref().id() == workspace.id()
    });
    if !workspace_focused || workspace.floating_is_active() {
        clear_focused(&mut tiled);
    }
    let Node {
        layout,
        orientation,
        nodes,
        focus,
        focused,
        ..
    } = &mut tiled;
    let (layout, orientation, nodes, mut focus, focused) = (
        *layout,
        orientation.clone(),
        std::mem::take(nodes),
        std::mem::take(focus),
        *focused || workspace_focused && workspace.active_window().is_none(),
    );
    let active_window = workspace_focused
        .then(|| workspace.active_window().map(|window| window.id()))
        .flatten();
    let mut floating_nodes = workspace
        .ipc_floating_trees()
        .filter_map(|(_, tree, sticky)| {
            let mut node = describe_tiling(
                tree,
                &|window| workspace.windows().find(|mapped| mapped.window == *window),
                output_origin,
                marks,
                container_marks,
                workspace.id(),
            )?;
            node.node_type = NodeType::FloatingCon;
            node.floating = Some("user_on".into());
            node.scratchpad_state = Some("none".into());
            node.sticky = sticky;
            Some(node)
        })
        .chain(
            workspace
                .tiles_with_ipc_layouts()
                .filter(|(tile, _)| workspace.is_floating_for_ipc(&tile.window().window))
                .map(|(tile, layout)| {
                    let (x, y) = layout.tile_pos_in_workspace_view.unwrap_or_default();
                    let outer_rect = offset_rect(
                        Rectangle::new(
                            (x, y).into(),
                            (layout.tile_size.0, layout.tile_size.1).into(),
                        ),
                        output_origin,
                    );
                    let mut node = describe_window(WindowNodeContext {
                        mapped: tile.window(),
                        rect: outer_rect,
                        node_type: NodeType::FloatingCon,
                        floating: "user_on",
                        parent: Some(rect),
                        marks,
                        in_scratchpad: compositor_layout
                            .is_scratchpad_window(&tile.window().window),
                        visible: true,
                    });
                    node.focused = active_window == Some(tile.window().id());
                    let border = tile.sway_border();
                    node.border = ipc_border(border.0);
                    node.current_border_width = i32::from(border.1);
                    let deco_rect = workspace.floating().ipc_decoration_rect(tile, &layout);
                    let has_titlebar = deco_rect.is_some();
                    node.deco_rect = deco_rect
                        .map_or_else(Rect::default, |rect| offset_rect(rect, output_origin));
                    let border_width = match (node.border, has_titlebar) {
                        (NodeBorder::Normal | NodeBorder::Pixel, true)
                        | (NodeBorder::Pixel, false) => node.current_border_width,
                        _ => 0,
                    };
                    let top = if has_titlebar { 0 } else { border_width };
                    node.rect = outer_rect;
                    node.window_rect = Rect {
                        x: border_width,
                        y: top,
                        width: (outer_rect.width - border_width * 2).max(0),
                        height: (outer_rect.height - border_width - top).max(0),
                    };
                    node.sticky = workspace.is_window_sticky(&tile.window().window);
                    node
                }),
        )
        .collect::<Vec<_>>();
    floating_nodes.reverse();
    let floating_focus = floating_nodes.iter().rev().map(|node| node.id);
    if workspace.floating_is_active() {
        focus.splice(0..0, floating_focus);
    } else {
        focus.extend(floating_focus);
    }
    if !workspace.floating_is_active() {
        let stale_tiling = workspace
            .ipc_tiling_tree()
            .nodes()
            .into_iter()
            .filter_map(|(id, _)| {
                workspace
                    .tiling_ipc_focus_is_stale(id)
                    .then_some(container_id(id))
            })
            .collect::<std::collections::HashSet<_>>();
        let focus_timestamps = workspace
            .windows()
            .filter_map(|window| {
                window
                    .focus_timestamp()
                    .map(|timestamp| (window_id(window.id()), timestamp))
            })
            .collect::<std::collections::HashMap<_, _>>();
        let children = nodes.iter().chain(&floating_nodes).collect::<Vec<_>>();
        focus.sort_by_key(|id| {
            Reverse((
                !stale_tiling.contains(id),
                children
                    .iter()
                    .find(|child| child.id == *id)
                    .and_then(|child| newest_focus_timestamp(child, &focus_timestamps)),
            ))
        });
    }
    let representation = workspace
        .tiling_has_had_window()
        .then(|| tree_representation(ipc_layout(workspace.tiling_representation_layout()), &nodes));
    let mut nodes = nodes;
    set_tabbed_percentages(layout, &mut nodes, rect);
    let tiling_fullscreen = apply_fullscreen_state(&mut nodes, workspace_visible);
    if !tiling_fullscreen {
        set_child_windows_visible(layout, &focus, &mut nodes, workspace_visible);
    }
    for node in &mut floating_nodes {
        set_windows_visible(node, workspace_visible && !tiling_fullscreen);
        if tiling_fullscreen {
            clear_focused(node);
        }
    }
    let mut node = common_node(CommonNodeContext {
        id: workspace_id(workspace.id().get()),
        node_type: NodeType::Workspace,
        layout,
        orientation: &orientation,
        name: Some(&workspace.sway_display_name(index)),
        rect,
        nodes,
        floating_nodes,
        focus,
        focused,
        properties: NodeProperties::Workspace(swayward_ipc::WorkspaceProperties {
            num: workspace.sway_display_number(index),
            output: output.into(),
            representation,
        }),
    });
    // Sway reports 1 for every workspace node, independent of whether a child
    // is fullscreen (`ipc_json_describe_workspace`, sway 1.12).
    node.fullscreen_mode = 1;
    node.urgent = workspace.is_urgent();
    node
}

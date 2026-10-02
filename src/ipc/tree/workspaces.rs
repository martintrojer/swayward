use super::*;
use crate::layout::floating_tree::StackSlot;

pub(super) struct WorkspaceNodeContext<'a> {
    pub(super) compositor_layout: &'a Layout<Mapped>,
    pub(super) workspace: &'a crate::layout::workspace::Workspace<Mapped>,
    pub(super) output: &'a str,
    pub(super) index: usize,
    pub(super) rect: Rect,
    pub(super) output_origin: Rect,
    pub(super) marks: &'a WindowMarks,
    pub(super) container_marks: &'a ContainerMarks,
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
    marks: &WindowMarks,
    container_marks: &ContainerMarks,
) -> Vec<Workspace> {
    layout
        .monitors()
        .flat_map(|monitor| {
            monitor
                .sway_workspaces()
                .map(move |(index, workspace)| (monitor, index, workspace))
        })
        .filter(|(monitor, _, workspace)| {
            workspace.must_be_kept() || monitor.active_workspace_ref().id() == workspace.id()
        })
        .map(|(monitor, index, workspace)| {
            let focused = layout
                .active_monitor_ref()
                .is_some_and(|active| active.output() == monitor.output())
                && monitor.active_workspace_idx() == index;
            let (node, properties) = describe_workspace(WorkspaceNodeContext {
                compositor_layout: layout,
                workspace,
                output: monitor.output_name(),
                index,
                rect: workspace_rect(global_space, monitor.output(), workspace),
                output_origin: output_rect(global_space, monitor.output()),
                marks,
                container_marks,
            });
            workspace_reply(
                node,
                properties,
                focused,
                monitor.active_workspace_idx() == index,
            )
        })
        .collect()
}

/// GET_WORKSPACES entry from a workspace's GET_TREE node. Sway serialises both
/// from the same node and drops the children (`sway/sway/ipc-server.c:836-845`).
fn workspace_reply(
    node: Node,
    properties: swayward_ipc::WorkspaceProperties,
    focused: bool,
    visible: bool,
) -> Workspace {
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
        ..
    } = node;
    Workspace {
        border,
        current_border_width,
        deco_rect,
        floating,
        floating_nodes,
        focus,
        focused,
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
        visible,
        window,
        window_rect,
    }
}

pub(super) fn describe_workspace_node(context: WorkspaceNodeContext<'_>) -> Node {
    let (mut node, properties) = describe_workspace(context);
    node.properties = NodeProperties::Workspace(properties);
    node
}

/// Whether the workspace is the active one on its output, and whether that
/// output also holds the seat focus.
struct WorkspaceState {
    focused: bool,
    visible: bool,
}

fn workspace_state(context: &WorkspaceNodeContext<'_>) -> WorkspaceState {
    let on_output = |monitor: &crate::layout::monitor::Monitor<Mapped>| {
        monitor.output_name() == context.output
            && monitor.active_workspace_ref().id() == context.workspace.id()
    };
    WorkspaceState {
        focused: context
            .compositor_layout
            .active_monitor_ref()
            .is_some_and(on_output),
        // Visibility is per output, not per seat: the active workspace of
        // every output is on screen, while only one of them holds keyboard
        // focus.
        visible: context.compositor_layout.monitors().any(on_output),
    }
}

/// The workspace node with its properties split out; the node's own
/// `properties` field is left as `None {}`.
fn describe_workspace(
    context: WorkspaceNodeContext<'_>,
) -> (Node, swayward_ipc::WorkspaceProperties) {
    let state = workspace_state(&context);
    let workspace = context.workspace;
    let mut tiled = tiled_part(&context, &state);
    let mut floating_nodes = floating_part(&context, &state);
    let mut focus = std::mem::take(&mut tiled.focus);
    order_focus(workspace, &mut focus, &tiled.nodes, &floating_nodes);
    let representation = workspace.tiling().has_had_tile().then(|| {
        tree_representation(
            ipc_layout(workspace.tiling().representation_layout()),
            &tiled.nodes,
        )
    });
    set_tabbed_percentages(tiled.layout, &mut tiled.nodes, context.rect);
    apply_workspace_visibility(
        tiled.layout,
        &focus,
        &mut tiled.nodes,
        &mut floating_nodes,
        state.visible,
    );
    let mut node = common_node(CommonNodeContext {
        id: workspace_id(workspace.id().get()),
        node_type: NodeType::Workspace,
        layout: tiled.layout,
        orientation: &tiled.orientation,
        name: Some(&workspace.sway_display_name(context.index)),
        rect: context.rect,
        nodes: tiled.nodes,
        floating_nodes,
        focus,
        focused: tiled.focused || state.focused && workspace.active_window().is_none(),
        properties: NodeProperties::None {},
    });
    // Sway reports 1 for every workspace node, independent of whether a child
    // is fullscreen (`ipc_json_describe_workspace`, sway 1.12).
    node.fullscreen_mode = 1;
    node.urgent = workspace.is_urgent();
    let properties = swayward_ipc::WorkspaceProperties {
        num: workspace.sway_display_number(context.index),
        output: context.output.into(),
        representation,
    };
    (node, properties)
}

/// The tiling tree's root, whose layout, children and focus the workspace
/// node adopts.
fn tiled_part(context: &WorkspaceNodeContext<'_>, state: &WorkspaceState) -> Node {
    let workspace = context.workspace;
    // Layout geometry is output-relative and already carries the working-area
    // origin: tiled leaves come from `parent_area`, which is the gap-inset
    // working area, and floating positions come from `scale_by_working_area`.
    // Children are therefore translated by the output origin, never by the
    // workspace rect, which would add the exclusive zone and the gaps twice.
    // Only the workspace node itself is reported at `rect`.
    let mut tiled = describe_tiling(
        workspace.ipc_tiling_tree(),
        &|window| workspace.windows().find(|mapped| mapped.window == *window),
        context.output_origin,
        context.marks,
        context.container_marks,
    )
    .unwrap_or_else(|| empty_tiling_node(context.rect));
    if !state.focused || workspace.floating_is_active() {
        clear_focused(&mut tiled);
    }
    tiled
}

/// Floating groups first, then single floating windows, topmost first.
fn floating_part(context: &WorkspaceNodeContext<'_>, state: &WorkspaceState) -> Vec<Node> {
    let workspace = context.workspace;
    let active_window = state
        .focused
        .then(|| workspace.active_window().map(|window| window.id()))
        .flatten();
    let mut floating_nodes = workspace
        .ipc_floating_trees()
        .filter_map(|(root, tree, sticky)| {
            // `container_replace` hands a scratchpad view's membership to the
            // container that `container_split` wraps it in
            // (sway/sway/tree/container.c:1471-1564), so a group holding a
            // scratchpad window is itself the scratchpad container.
            let in_scratchpad =
                tree.any_window(&|window| context.compositor_layout.is_scratchpad_window(window));
            let mut node = describe_tiling(
                tree,
                &|window| workspace.windows().find(|mapped| mapped.window == *window),
                context.output_origin,
                context.marks,
                context.container_marks,
            )?;
            node.node_type = NodeType::FloatingCon;
            node.floating = Some("user_on".into());
            node.scratchpad_state = Some(if in_scratchpad { "fresh" } else { "none" }.into());
            node.sticky = sticky;
            // A floating group's own tree keeps its internal focus while another layer is
            // active; sway reports a container focused only when it holds the seat focus.
            let holds_focus = state.focused
                && workspace.active_window().is_some_and(|active| {
                    workspace.floating_tree_root_for_window(&active.window) == Some(root)
                });
            if !state.focused || !workspace.floating_is_active() || !holds_focus {
                clear_focused(&mut node);
            }
            Some((StackSlot::Tree(root), node))
        })
        .chain(
            workspace
                .tiles_with_ipc_layouts()
                .filter(|(tile, _)| workspace.is_floating_for_ipc(&tile.window().window))
                .map(|(tile, layout)| {
                    let mut node = describe_floating_window(context, tile, &layout);
                    node.focused = active_window == Some(tile.window().id());
                    (StackSlot::Window(tile.window().window.clone()), node)
                }),
        )
        .collect::<Vec<_>>();
    // Sway lists the workspace's floating containers bottom to top, a group
    // and a single window in one list (`workspace->floating`,
    // sway/ipc-json.c:532-540). A fullscreen tile that restores to floating
    // is not in the floating stack yet; it is listed on top.
    let stacking = workspace.floating().stacking();
    let depth = |slot: &StackSlot<_>| {
        stacking
            .iter()
            .position(|candidate| candidate == slot)
            .map_or(0, |index| index + 1)
    };
    floating_nodes.sort_by_key(|(slot, _)| std::cmp::Reverse(depth(slot)));
    floating_nodes.into_iter().map(|(_, node)| node).collect()
}

fn describe_floating_window(
    context: &WorkspaceNodeContext<'_>,
    tile: &crate::layout::tile::Tile<Mapped>,
    layout: &swayward_ipc::WindowLayout,
) -> Node {
    let workspace = context.workspace;
    let (x, y) = layout.tile_pos_in_workspace_view.unwrap_or_default();
    let rect = offset_rect(
        Rectangle::new(
            (x, y).into(),
            (layout.tile_size.0, layout.tile_size.1).into(),
        ),
        context.output_origin,
    );
    let border = tile.sway_border_thickness();
    let deco_rect = workspace.floating().ipc_decoration_rect(tile, layout);
    let frame = ViewFrame {
        rect,
        border: ipc_border(border.0),
        border_width: i32::from(border.1),
        has_titlebar: deco_rect.is_some(),
        edges: ResizeEdge::all(),
    };
    let mut node = describe_window(WindowNodeContext {
        mapped: tile.window(),
        rect,
        border: frame.border,
        border_width: frame.border_width,
        window_rect: frame.window_rect(),
        node_type: NodeType::FloatingCon,
        floating: "user_on",
        parent: Some(context.rect),
        marks: context.marks,
        in_scratchpad: context
            .compositor_layout
            .is_scratchpad_window(&tile.window().window),
        visible: true,
    });
    node.deco_rect = deco_rect.map_or_else(Rect::default, |rect| {
        offset_rect(rect, context.output_origin)
    });
    node.sticky = workspace.is_window_sticky(&tile.window().window);
    node
}

/// Merge floating ids into the tiled focus list, then order it most recent
/// first unless the floating layer is active.
fn order_focus(
    workspace: &crate::layout::workspace::Workspace<Mapped>,
    focus: &mut Vec<i64>,
    nodes: &[Node],
    floating_nodes: &[Node],
) {
    let floating_focus = floating_nodes.iter().rev().map(|node| node.id);
    if workspace.floating_is_active() {
        focus.splice(0..0, floating_focus);
        return;
    }
    focus.extend(floating_focus);
    if workspace.tiling().ipc_focus_follows_history() {
        return;
    }
    let stale_tiling = workspace
        .ipc_tiling_tree()
        .nodes()
        .into_iter()
        .filter_map(|(id, _)| {
            workspace
                .tiling()
                .ipc_focus_is_stale(id)
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
    let children = nodes.iter().chain(floating_nodes).collect::<Vec<_>>();
    // A node that was never focused joins the tail of sway's focus stack
    // (`seat_node_from_node`, sway/input/seat.c:327-349). So a wrapper that
    // was never focused sorts behind every focused node. A never-focused
    // window beside it was created after the wrapper took every workspace
    // child, so it joined the tail later and sorts behind the wrapper.
    let timestamp_of = |id: &i64| {
        children
            .iter()
            .find(|child| child.id == *id)
            .and_then(|child| newest_focus_timestamp(child, &focus_timestamps))
    };
    // The tiled list already follows sway's focus stack, which raises a
    // container whenever focus enters it (`seat_set_raw_focus`,
    // sway/input/seat.c). A container keeps that place after the focused
    // view leaves it, so a tiled entry ranks as recent as the last time
    // focus entered it, and at least as recent as every tiled entry after
    // it. Timestamps then only interleave floating windows.
    let tiled_ids = nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let last_entered = workspace
        .tiling()
        .last_entered_windows()
        .filter_map(|(node, window)| {
            let mapped = workspace
                .windows()
                .find(|mapped| &mapped.window == window)?;
            Some((container_id(node), mapped.focus_timestamp()?))
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut effective = std::collections::HashMap::new();
    let mut newer = None;
    for id in focus.iter().rev().filter(|id| tiled_ids.contains(id)) {
        newer = newer
            .max(timestamp_of(id))
            .max(last_entered.get(id).copied());
        effective.insert(*id, newer);
    }
    focus.sort_by_key(|id| {
        let timestamp = timestamp_of(id);
        let rank = match (stale_tiling.contains(id), timestamp.is_some()) {
            (false, true) => 2,
            (true, _) => 1,
            (false, false) => 0,
        };
        Reverse((rank, effective.get(id).copied().unwrap_or(timestamp)))
    });
}

/// A workspace fullscreen container hides every view outside it, across the
/// tiling and floating layers (`view_is_visible`,
/// `sway/sway/tree/view.c:1187-1193`).
fn apply_workspace_visibility(
    layout: NodeLayout,
    focus: &[i64],
    nodes: &mut [Node],
    floating_nodes: &mut [Node],
    workspace_visible: bool,
) {
    let floating_fullscreen = floating_nodes.iter().any(contains_fullscreen);
    let tiling_fullscreen = if floating_fullscreen {
        for node in nodes.iter_mut() {
            set_windows_visible(node, false);
        }
        false
    } else {
        apply_fullscreen_state(nodes, workspace_visible)
    };
    if !tiling_fullscreen && !floating_fullscreen {
        set_child_windows_visible(layout, focus, nodes, workspace_visible);
    }
    for node in floating_nodes {
        let shown = !tiling_fullscreen && (!floating_fullscreen || contains_fullscreen(node));
        set_windows_visible(node, workspace_visible && shown);
        if tiling_fullscreen {
            clear_focused(node);
        }
    }
}

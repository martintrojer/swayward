use super::query_state::{
    binding_modes, binding_state, clear_workspace_focus, describe_input, find_focused_node,
    find_parent_of_node, find_workspace_by_id, find_workspace_by_tree_id,
    refresh_input_query_state, refresh_query_state,
};
use super::*;

#[derive(Default)]
pub(super) struct WorkspaceEventTransaction {
    pub(super) events: Vec<Event>,
    pub(super) suppress_workspace_moves: bool,
    pub(super) scratchpad: Option<ScratchpadEventOrder>,
}

#[derive(Clone, Copy)]
pub(crate) enum ScratchpadEventOrder {
    Hide,
    Show,
}

fn make_ipc_window(
    mapped: &Mapped,
    workspace_id: Option<WorkspaceId>,
    layout: WindowLayout,
) -> swayward_ipc::Window {
    let title = mapped.formatted_title();
    with_toplevel_role(mapped.toplevel(), |role| swayward_ipc::Window {
        id: mapped.id().get(),
        title: Some(title),
        app_id: role.app_id.clone(),
        pid: mapped.credentials().map(|c| c.pid),
        workspace_id: workspace_id.map(|id| id.get()),
        is_focused: mapped.is_focused(),
        is_floating: mapped.is_floating(),
        is_urgent: mapped.is_urgent(),
        layout,
        focus_timestamp: mapped.get_focus_timestamp().map(Timestamp::from),
    })
}

impl State {
    pub(crate) fn ipc_input_changed(
        &mut self,
        change: &'static str,
        device: crate::input::IpcInputDevice,
    ) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let input = describe_input(&self.swayward, &device);
        refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
        server.send_event(Event::SwayInputChanged {
            change: change.into(),
            input,
        });
    }

    fn ipc_keyboard_input_changed(&mut self, change: &'static str) {
        let devices = self
            .swayward
            .ipc_input_devices
            .values()
            .filter(|device| device.device_type == "keyboard")
            .cloned()
            .collect::<Vec<_>>();
        for device in devices {
            self.ipc_input_changed(change, device);
        }
    }

    pub fn ipc_keyboard_layouts_changed(&mut self) {
        let Some(keyboard_layouts) = keyboard_layouts(self) else {
            if let Some(server) = &self.swayward.ipc_server {
                server
                    .event_stream_state
                    .borrow_mut()
                    .keyboard_layouts
                    .keyboard_layouts = None;
                refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
            }
            return;
        };

        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        {
            let mut event_state = server.event_stream_state.borrow_mut();
            let state = &mut event_state.keyboard_layouts;
            let event = Event::KeyboardLayoutsChanged { keyboard_layouts };
            state.apply(event.clone());
            server.send_event(event);
        }
        refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
        self.ipc_keyboard_input_changed("xkb_keymap");
    }

    pub fn ipc_refresh_keyboard_layout_index(&mut self) {
        let Some(keyboard_layouts) = keyboard_layouts(self) else {
            return;
        };
        let idx = keyboard_layouts.current_idx;

        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        {
            let mut event_state = server.event_stream_state.borrow_mut();
            let state = &mut event_state.keyboard_layouts;
            if state
                .keyboard_layouts
                .as_ref()
                .is_none_or(|layouts| layouts.current_idx == idx)
            {
                return;
            }
            let event = Event::KeyboardLayoutSwitched { idx };
            state.apply(event.clone());
            server.send_event(event);
        }
        refresh_input_query_state(&self.swayward, &mut server.query_state.borrow_mut());
        self.ipc_keyboard_input_changed("xkb_layout");
    }

    pub(crate) fn ipc_refresh_config(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let mut query_state = server.query_state.borrow_mut();
        query_state.binding_modes = binding_modes(&self.swayward.config.borrow());
        query_state.binding_state = binding_state(&self.swayward.binding_mode);
        refresh_input_query_state(&self.swayward, &mut query_state);
    }

    pub fn ipc_refresh_layout(&mut self) {
        if self
            .swayward
            .ipc_server
            .as_ref()
            .is_none_or(|server| !server.has_event_streams())
        {
            return;
        }
        self.ipc_initialize_event_state();
    }

    /// Open an event transaction, or join the one an outer command opened.
    ///
    /// Sway buffers nothing here: a nested `for_window` command runs inside the
    /// outer command's handler and its events interleave there. Joining keeps
    /// the outer command's reordering and suppression in force until the
    /// outermost commit.
    pub(crate) fn ipc_begin_workspace_transaction(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        server
            .workspace_event_depth
            .set(server.workspace_event_depth.get().saturating_add(1));
        if server.has_event_streams() && server.workspace_events.borrow().is_none() {
            *server.workspace_events.borrow_mut() = Some(WorkspaceEventTransaction::default());
        }
    }

    pub(crate) fn ipc_order_scratchpad_events(&mut self, order: ScratchpadEventOrder) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        if let Some(transaction) = server.workspace_events.borrow_mut().as_mut() {
            transaction.scratchpad = Some(order);
        }
    }

    pub(crate) fn ipc_suppress_workspace_moves(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        if let Some(transaction) = server.workspace_events.borrow_mut().as_mut() {
            transaction.suppress_workspace_moves = true;
        }
    }

    pub(crate) fn ipc_commit_workspace_transaction(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let depth = server.workspace_event_depth.get().saturating_sub(1);
        server.workspace_event_depth.set(depth);
        if depth > 0 {
            return;
        }
        let Some(transaction) = server.workspace_events.borrow_mut().take() else {
            return;
        };
        let current_tree = crate::ipc::tree::describe_tree(
            &self.swayward.layout,
            &self.swayward.global_space,
            &self.swayward.marks_by_window,
            &self.swayward.marks_by_container,
        );
        let mut events = transaction.events;
        if let Some(order) = transaction.scratchpad {
            fn typed(container: &serde_json::Value) -> Option<swayward_ipc::Node> {
                serde_json::from_value(container.clone()).ok()
            }
            fn store(container: &mut serde_json::Value, node: swayward_ipc::Node) {
                if let Ok(value) = serde_json::to_value(node) {
                    *container = value;
                }
            }

            // Pair the visible and hidden snapshots by container id. A criteria
            // command can move several independent windows in one transaction;
            // taking the first snapshot made every event describe one window.
            let mut visible = std::collections::HashMap::new();
            let mut hidden = std::collections::HashMap::new();
            for event in &events {
                let Event::SwayWindowChanged { change, container } = event else {
                    continue;
                };
                if !matches!(change.as_str(), "floating" | "move" | "focus") {
                    continue;
                }
                let Some(node) = typed(container) else {
                    continue;
                };
                match node.scratchpad_state.as_deref() {
                    Some("fresh") => {
                        hidden.insert(node.id, node);
                    }
                    Some("none") => {
                        visible.insert(node.id, node);
                    }
                    _ => {}
                }
            }

            let mut reordered = events
                .iter()
                .filter(|event| {
                    matches!(event,
                    Event::SwayWindowChanged { change, .. }
                    if matches!(change.as_str(), "floating" | "move" | "focus"))
                })
                .cloned()
                .collect::<Vec<_>>();
            reordered.sort_by_key(|event| match (&order, event) {
                (ScratchpadEventOrder::Hide, Event::SwayWindowChanged { change, .. })
                    if change == "floating" =>
                {
                    0
                }
                (ScratchpadEventOrder::Show, Event::SwayWindowChanged { change, .. })
                    if change == "focus" =>
                {
                    0
                }
                _ => 1,
            });
            let mut reordered = reordered.into_iter();
            for slot in &mut events {
                if !matches!(slot, Event::SwayWindowChanged { change, .. }
                    if matches!(change.as_str(), "floating" | "move" | "focus"))
                {
                    continue;
                }
                let Some(mut event) = reordered.next() else {
                    break;
                };
                if let Event::SwayWindowChanged { change, container } = &mut event {
                    let Some(mut node) = typed(container) else {
                        *slot = event;
                        continue;
                    };
                    match order {
                        ScratchpadEventOrder::Hide if change == "floating" => {
                            if let Some(snapshot) = visible.get(&node.id) {
                                node = snapshot.clone();
                            }
                            node.scratchpad_state = Some("none".into());
                            node.focused = true;
                            if let swayward_ipc::NodeProperties::View(view) = &mut node.properties {
                                view.visible = true;
                            }
                        }
                        ScratchpadEventOrder::Hide if change == "move" => {
                            if let Some(snapshot) = hidden.get(&node.id) {
                                node = snapshot.clone();
                            }
                            node.focused = false;
                            if let swayward_ipc::NodeProperties::View(view) = &mut node.properties {
                                view.visible = false;
                            }
                        }
                        _ => {}
                    }
                    store(container, node);
                }
                *slot = event;
            }
        }

        // Sway emits these at their tree mutation sites. Swayward's fallback
        // diff may discover an adjacent pair in the opposite order, so restore
        // the mutation order before flushing the operation-local transaction.
        let mut index = 0;
        while index + 1 < events.len() {
            let reverse_workspace_move = matches!(events[index], Event::WorkspaceMoved { .. })
                && matches!(events[index + 1], Event::WorkspaceEmptied { .. });
            let reverse_urgency = matches!(events[index], Event::WorkspaceUrgencyChanged { .. })
                && matches!(events[index + 1], Event::SwayWindowChanged { ref change, .. } if change == "urgent");
            if reverse_workspace_move || reverse_urgency {
                events.swap(index, index + 1);
            }
            index += 1;
        }

        let has_workspace_move = events
            .iter()
            .any(|event| matches!(event, Event::WorkspaceMoved { .. }));
        let sticky_move = events.iter().find_map(|event| match event {
            Event::SwayWindowChanged { change, container }
                if change == "move" && container["sticky"] == true =>
            {
                Some(container.clone())
            }
            _ => None,
        });
        let sticky_root = sticky_move
            .as_ref()
            .and_then(|container| {
                if container["type"] == "floating_con" {
                    return serde_json::from_value(container.clone()).ok();
                }
                let id = container["id"].as_i64()?;
                fn parent(node: &swayward_ipc::Node, id: i64) -> Option<&swayward_ipc::Node> {
                    node.nodes
                        .iter()
                        .chain(&node.floating_nodes)
                        .find_map(|child| {
                            (child.id == id)
                                .then_some(node)
                                .or_else(|| parent(child, id))
                        })
                }
                parent(&current_tree, id).cloned()
            })
            .or_else(|| {
                fn sticky_root(node: &swayward_ipc::Node) -> Option<&swayward_ipc::Node> {
                    (node.node_type == swayward_ipc::NodeType::FloatingCon && node.sticky)
                        .then_some(node)
                        .or_else(|| {
                            node.nodes
                                .iter()
                                .chain(&node.floating_nodes)
                                .find_map(sticky_root)
                        })
                }
                sticky_root(&current_tree).cloned()
            });
        if transaction.suppress_workspace_moves {
            if let Some(output_event) = events
                .iter()
                .position(|event| matches!(event, Event::OutputChanged))
            {
                let event = events.remove(output_event);
                if let Some(empty) = events
                    .iter()
                    .rposition(|event| matches!(event, Event::WorkspaceEmptied { .. }))
                {
                    events.insert(empty + 1, event);
                } else {
                    events.push(event);
                }
            }
        }

        let mut moved_workspaces = HashSet::new();
        let mut output = Vec::new();
        for mut event in events {
            match &mut event {
                Event::WorkspaceMoved { current } => {
                    if transaction.suppress_workspace_moves || !moved_workspaces.insert(current.id)
                    {
                        continue;
                    }
                    let focused = find_focused_node(current).cloned();
                    clear_workspace_focus(current, false);
                    output.push(event);
                    if let Some(container) = focused {
                        output.push(Event::SwayWindowChanged {
                            change: "focus".into(),
                            container: serde_json::to_value(container).unwrap_or_default(),
                        });
                    }
                    continue;
                }
                Event::WorkspaceInitialized { current } => {
                    // Sway emits init when it creates the empty workspace, before
                    // the command moves a container into it.
                    current.nodes.clear();
                    current.floating_nodes.clear();
                    current.focus.clear();
                    current.focused = false;
                    if let swayward_ipc::NodeProperties::Workspace(properties) =
                        &mut current.properties
                    {
                        properties.representation = None;
                    }
                }
                Event::WorkspaceFocusChanged { old, current } => {
                    if let Some(old) = old {
                        if let Some(settled) = find_workspace_by_tree_id(&current_tree, old.id) {
                            **old = settled.clone();
                            old.focused = false;
                        } else {
                            clear_workspace_focus(old, true);
                        }
                        if let Some(root) = &sticky_root {
                            old.floating_nodes = vec![root.clone()];
                            old.focus = vec![root.id];
                            for child in &mut old.floating_nodes {
                                clear_workspace_focus(child, false);
                            }
                            if !root.nodes.is_empty() {
                                if let swayward_ipc::NodeProperties::Workspace(properties) =
                                    &mut old.properties
                                {
                                    properties.representation = Some("H[]".into());
                                }
                            }
                        }
                    }
                    if let Some(settled) = find_workspace_by_tree_id(&current_tree, current.id) {
                        **current = settled.clone();
                        current.focused = true;
                    }
                    if sticky_root.is_some() {
                        current.floating_nodes.clear();
                        current.focus.clear();
                    }
                }
                Event::WorkspaceEmptied { current } => {
                    current.nodes.clear();
                    current.floating_nodes.clear();
                    current.focus.clear();
                    if sticky_root.is_none() {
                        if has_workspace_move && !transaction.suppress_workspace_moves {
                            current.layout = swayward_ipc::NodeLayout::SplitH;
                            current.orientation = "horizontal".into();
                        }
                        if let swayward_ipc::NodeProperties::Workspace(properties) =
                            &mut current.properties
                        {
                            properties.representation = None;
                        }
                    } else if let swayward_ipc::NodeProperties::Workspace(properties) =
                        &mut current.properties
                    {
                        properties.representation = Some(
                            if sticky_root
                                .as_ref()
                                .is_some_and(|root| !root.nodes.is_empty())
                            {
                                "H[]".into()
                            } else {
                                "V[]".into()
                            },
                        );
                    }
                    current.focused = false;
                }
                Event::SwayWindowChanged { change, container }
                    if change == "move"
                        && (transaction.suppress_workspace_moves
                            || container["sticky"] == true) =>
                {
                    continue;
                }
                _ => {}
            }
            output.push(event);
        }
        for event in output {
            server.send_event_now(event);
        }
    }

    pub(super) fn ipc_initialize_event_state(&mut self) {
        let previous_tree =
            self.swayward.ipc_server.as_ref().and_then(|server| {
                serde_json::from_str(&server.query_state.borrow().event_tree).ok()
            });
        self.ipc_refresh_workspaces();
        if let Some(server) = &self.swayward.ipc_server {
            let mut query_state = server.query_state.borrow_mut();
            query_state.binding_state = binding_state(&self.swayward.binding_mode);
            refresh_input_query_state(&self.swayward, &mut query_state);
            let ipc_outputs = ipc_outputs_snapshot(self);
            refresh_query_state(
                &self.swayward.layout,
                &self.swayward.global_space,
                &self.swayward.output_power,
                &ipc_outputs,
                &self.swayward.marks_by_window,
                &self.swayward.marks_by_container,
                &mut query_state,
            );
            query_state.event_tree = query_state.tree.clone();
        }
        self.ipc_refresh_windows(previous_tree.as_ref());
        self.ipc_refresh_overview();
    }

    fn ipc_refresh_workspaces(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let _span = tracy_client::span!("State::ipc_refresh_workspaces");

        let previous_tree =
            serde_json::from_str::<swayward_ipc::Node>(&server.query_state.borrow().event_tree)
                .ok();
        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.workspaces;

        let mut events = Vec::new();
        let layout = &self.swayward.layout;
        let focused_ws_id = layout.active_workspace().map(|ws| ws.id().get());

        let current_tree = crate::ipc::tree::describe_tree(
            layout,
            &self.swayward.global_space,
            &self.swayward.marks_by_window,
            &self.swayward.marks_by_container,
        );
        let old_focused = state
            .workspaces
            .values()
            .find(|workspace| workspace.is_focused)
            .cloned();
        let old_focused_node = old_focused
            .as_ref()
            .and_then(|workspace| {
                previous_tree
                    .as_ref()
                    .and_then(|tree| find_workspace_by_id(tree, workspace.id))
            })
            .cloned()
            .map(|mut workspace| {
                workspace.focused = false;
                Box::new(workspace)
            });

        // Check for workspace changes.
        let mut seen = HashSet::new();
        let mut need_workspaces_changed = false;
        for (mon, ws_idx, ws) in layout.workspaces() {
            let id = ws.id().get();
            let Some(current_node) = find_workspace_by_id(&current_tree, id) else {
                continue;
            };
            seen.insert(id);

            let Some(ipc_ws) = state.workspaces.get(&id) else {
                let mut current = current_node.clone();
                let focused = Some(id) == focused_ws_id;
                current.focused = false;
                events.push(Event::WorkspaceInitialized {
                    current: Box::new(current),
                });
                if focused {
                    let mut current = current_node.clone();
                    current.focused = true;
                    events.push(Event::WorkspaceFocusChanged {
                        old: old_focused_node.clone(),
                        current: Box::new(current),
                    });
                }
                need_workspaces_changed = true;
                continue;
            };

            let output_name = mon.map(|mon| mon.output_name());
            if ipc_ws.name != ws.sway_name() {
                if let Some(current) = find_workspace_by_id(&current_tree, id).cloned() {
                    events.push(Event::WorkspaceRenamed {
                        current: Box::new(current),
                    });
                }
                need_workspaces_changed = true;
            } else if ipc_ws.output.as_ref() != output_name {
                events.push(Event::WorkspaceMoved {
                    current: Box::new(current_node.clone()),
                });
                need_workspaces_changed = true;
            }

            let active_window_id = ws.active_window().map(|win| win.id().get());
            if ipc_ws.active_window_id != active_window_id {
                events.push(Event::WorkspaceActiveWindowChanged {
                    workspace_id: id,
                    active_window_id,
                });
            }

            // Check if this workspace urgent state changed.
            let urgent = ws.is_urgent();
            if urgent != ipc_ws.is_urgent {
                let mut current = current_node.clone();
                current.urgent = urgent;
                events.push(Event::WorkspaceUrgencyChanged {
                    id,
                    current: Box::new(current),
                });
            }

            // Check if this workspace became focused.
            let is_focused = Some(id) == focused_ws_id;
            if is_focused && !ipc_ws.is_focused {
                if let Some(mut current) = find_workspace_by_id(&current_tree, id).cloned() {
                    current.focused = true;
                    events.push(Event::WorkspaceFocusChanged {
                        old: old_focused_node.clone(),
                        current: Box::new(current),
                    });
                }
                state.apply(Event::WorkspaceActivated { id, focused: true });
                continue;
            }

            // Check if this workspace became active.
            let is_active = mon.is_some_and(|mon| mon.active_workspace_idx() == ws_idx);
            if is_active && !ipc_ws.is_active {
                events.push(Event::WorkspaceActivated { id, focused: false });
            }
        }

        if old_focused.is_some_and(|workspace| !seen.contains(&workspace.id)) {
            events.retain(|event| !matches!(event, Event::WorkspaceFocusChanged { .. }));
            if let Some(id) = focused_ws_id {
                if let Some(mut current) = find_workspace_by_id(&current_tree, id).cloned() {
                    current.focused = true;
                    events.push(Event::WorkspaceFocusChanged {
                        old: old_focused_node.clone(),
                        current: Box::new(current),
                    });
                }
            }
        }

        // Check if any workspaces were removed.
        for workspace in state
            .workspaces
            .values()
            .filter(|workspace| !seen.contains(&workspace.id))
        {
            if let Some(mut current) = previous_tree
                .as_ref()
                .and_then(|tree| find_workspace_by_id(tree, workspace.id))
                .cloned()
            {
                current.nodes.clear();
                current.floating_nodes.clear();
                current.focus.clear();
                if let swayward_ipc::NodeProperties::Workspace(properties) = &mut current.properties
                {
                    properties.representation = None;
                }
                current.focused = false;
                events.push(Event::WorkspaceEmptied {
                    current: Box::new(current),
                });
            }
            need_workspaces_changed = true;
        }

        if need_workspaces_changed {
            let sway_events = events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        Event::WorkspaceInitialized { .. }
                            | Event::WorkspaceRenamed { .. }
                            | Event::WorkspaceMoved { .. }
                            | Event::WorkspaceFocusChanged { .. }
                            | Event::WorkspaceUrgencyChanged { .. }
                            | Event::WorkspaceEmptied { .. }
                    )
                })
                .cloned()
                .collect::<Vec<_>>();
            events.clear();

            let workspaces = layout
                .workspaces()
                .filter_map(|(mon, ws_idx, ws)| {
                    let id = ws.id().get();
                    find_workspace_by_id(&current_tree, id).map(|_| Workspace {
                        id,
                        idx: u8::try_from(ws_idx + 1).unwrap_or(u8::MAX),
                        name: ws.sway_name(),
                        output: mon.map(|mon| mon.output_name().clone()),
                        is_urgent: ws.is_urgent(),
                        is_active: mon.is_some_and(|mon| mon.active_workspace_idx() == ws_idx),
                        is_focused: Some(id) == focused_ws_id,
                        active_window_id: ws.active_window().map(|win| win.id().get()),
                    })
                })
                .collect();

            state.apply(Event::WorkspacesChanged { workspaces });
            events.extend(sway_events);
        }

        for event in events {
            state.apply(event.clone());
            server.send_event(event);
        }
    }

    fn ipc_refresh_windows(&mut self, previous_tree: Option<&serde_json::Value>) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let _span = tracy_client::span!("State::ipc_refresh_windows");

        let current_tree = serde_json::to_value(crate::ipc::tree::describe_tree(
            &self.swayward.layout,
            &self.swayward.global_space,
            &self.swayward.marks_by_window,
            &self.swayward.marks_by_container,
        ))
        .unwrap_or_default();
        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.windows;

        let mut events = Vec::new();
        let mut restored_focus_events = Vec::new();
        let layout = &self.swayward.layout;
        let focused_window_closed = state.windows.values().any(|window| {
            window.is_focused
                && find_node_by_id(
                    &current_tree,
                    crate::ipc::tree::window_id_from_raw(window.id),
                )
                .is_none()
        });

        let mut batch_change_layouts: Vec<(u64, WindowLayout)> = Vec::new();

        // Check for window changes.
        let mut seen = HashSet::new();
        let mut focused_id = None;
        layout.with_windows(|mapped, _, ws_id, window_layout| {
            let id = mapped.id().get();
            seen.insert(id);

            let node_id = crate::ipc::tree::window_id(mapped.id());
            let current_node = find_node_by_id(&current_tree, node_id).cloned();
            let is_focused = mapped.is_focused();
            if is_focused {
                focused_id = Some(id);
            }

            let previous_node = previous_tree.and_then(|tree| find_node_by_id(tree, node_id));
            let Some(ipc_win) = state.windows.get(&id) else {
                if let Some(mut container) = current_node.clone() {
                    // Sway emits `new` from view_map before arranging, applying
                    // borders or setting the view title, then emits `title`
                    // when that metadata arrives (`sway/tree/view.c:902,1138`).
                    // Our diff first sees the already-settled window, so
                    // reconstruct those two map-time snapshots.
                    container["border"] = "none".into();
                    container["current_border_width"] = 0.into();
                    container["focused"] = false.into();
                    let hidden_before_focus = find_parent_of_node(&current_tree, node_id)
                        .is_some_and(|parent| {
                            parent["layout"]
                                .as_str()
                                .is_some_and(|layout| matches!(layout, "tabbed" | "stacked"))
                                && parent["nodes"]
                                    .as_array()
                                    .is_some_and(|nodes| nodes.len() > 1)
                        });
                    if hidden_before_focus {
                        container["visible"] = false.into();
                    }
                    container["name"] = serde_json::Value::Null;
                    container["percent"] = 0.0.into();
                    for rect in ["deco_rect", "rect", "window_rect"] {
                        container[rect] = serde_json::json!({
                            "x": 0,
                            "y": 0,
                            "width": 0,
                            "height": 0,
                        });
                    }
                    events.push(Event::SwayWindowChanged {
                        change: "new".into(),
                        container: container.clone(),
                    });
                    let title = with_toplevel_role(mapped.toplevel(), |role| role.title.clone());
                    if let Some(title) = title {
                        container["name"] = title.into();
                        events.push(Event::SwayWindowChanged {
                            change: "title".into(),
                            container,
                        });
                    }
                }
                let window = make_ipc_window(mapped, ws_id, window_layout);
                events.push(Event::WindowOpenedOrChanged {
                    window: window.clone(),
                });
                if window.is_focused {
                    if let Some(mut container) = current_node {
                        // Focus is delivered before sway commits the configured
                        // default border (`sway/input/seat.c:1197`, after
                        // `view_map` emitted the map-time events).
                        container["border"] = "none".into();
                        container["current_border_width"] = 0.into();
                        events.push(Event::SwayWindowChanged {
                            change: "focus".into(),
                            container,
                        });
                    }
                    events.push(Event::WindowFocusChanged { id: Some(id) });
                }
                return;
            };

            let workspace_id = ws_id.map(|id| id.get());
            let moved = ipc_win.workspace_id != workspace_id;
            let shown_from_scratchpad =
                moved && previous_node.is_some_and(|node| node["scratchpad_state"] == "fresh");
            // root_scratchpad_remove_container emits `move` (sway/tree/root.c:150-154).
            let left_scratchpad =
                previous_node
                    .zip(current_node.as_ref())
                    .is_some_and(|(old, current)| {
                        old["scratchpad_state"] == "fresh" && current["scratchpad_state"] == "none"
                    });
            let floating_changed = ipc_win.is_floating != mapped.is_floating();
            let sway_floating_changed = previous_node
                .zip(current_node.as_ref())
                .is_some_and(|(old, current)| old["type"] != current["type"]);
            let title_changed = ipc_win.title.as_deref() != Some(&mapped.formatted_title());
            let fullscreen_changed = previous_node
                .zip(current_node.as_ref())
                .is_some_and(|(old, current)| old["fullscreen_mode"] != current["fullscreen_mode"]);
            let marks_changed = previous_node
                .zip(current_node.as_ref())
                .is_some_and(|(old, current)| old["marks"] != current["marks"]);

            if let Some(container) = current_node.clone() {
                for change in [
                    (moved || left_scratchpad).then_some("move"),
                    sway_floating_changed.then_some("floating"),
                    title_changed.then_some("title"),
                    fullscreen_changed.then_some("fullscreen_mode"),
                    marks_changed.then_some("mark"),
                ]
                .into_iter()
                .flatten()
                {
                    let mut container = container.clone();
                    if change == "floating" {
                        container["floating"] = if container["type"] == "floating_con" {
                            "user_on".into()
                        } else {
                            "user_off".into()
                        };
                    }
                    events.push(Event::SwayWindowChanged {
                        change: change.into(),
                        container,
                    });
                }
            }
            if moved || floating_changed || title_changed {
                events.push(Event::WindowOpenedOrChanged {
                    window: make_ipc_window(mapped, ws_id, window_layout.clone()),
                });
                if !shown_from_scratchpad {
                    return;
                }
                if let Some(container) = current_node.clone() {
                    events.push(Event::SwayWindowChanged {
                        change: "focus".into(),
                        container,
                    });
                }
            }

            if ipc_win.layout != window_layout {
                batch_change_layouts.push((id, window_layout));
            }

            if mapped.is_focused() && !ipc_win.is_focused {
                let node_id = crate::ipc::tree::window_id(mapped.id());
                let mut container = if focused_window_closed {
                    previous_tree.and_then(|tree| find_node_by_id(tree, node_id))
                } else {
                    find_node_by_id(&current_tree, node_id)
                }
                .cloned();
                if let Some(container) = &mut container {
                    container["focused"] = true.into();
                }
                if let Some(container) = container {
                    let focus = Event::SwayWindowChanged {
                        change: "focus".into(),
                        container,
                    };
                    if focused_window_closed {
                        restored_focus_events.push(focus);
                    } else {
                        events.push(focus);
                    }
                }
                let focus = Event::WindowFocusChanged { id: Some(id) };
                if focused_window_closed {
                    restored_focus_events.push(focus);
                } else {
                    events.push(focus);
                }
            }

            let focus_timestamp = mapped.get_focus_timestamp().map(Timestamp::from);
            if focus_timestamp != ipc_win.focus_timestamp {
                events.push(Event::WindowFocusTimestampChanged {
                    id,
                    focus_timestamp,
                });
            }

            let urgent = mapped.is_urgent();
            if urgent != ipc_win.is_urgent {
                if let Some(container) =
                    find_node_by_id(&current_tree, crate::ipc::tree::window_id(mapped.id()))
                        .cloned()
                {
                    events.push(Event::SwayWindowChanged {
                        change: "urgent".into(),
                        container,
                    });
                }
                events.push(Event::WindowUrgencyChanged { id, urgent })
            }
        });

        // It might make sense to push layout changes after closed windows (since windows about to
        // be closed will occupy the same column/tile positions as the window that moved into this
        // vacated space), but also we are already pushing some layout changes in
        // WindowOpenedOrChanged above, meaning that the receiving end has to handle this case
        // anyway.
        if !batch_change_layouts.is_empty() {
            events.push(Event::WindowLayoutsChanged {
                changes: batch_change_layouts,
            });
        }

        // Check for closed windows.
        let mut ipc_focused_id = None;
        for (id, ipc_win) in &state.windows {
            let node_id = crate::ipc::tree::window_id_from_raw(*id);
            if !seen.contains(id) && find_node_by_id(&current_tree, node_id).is_none() {
                if let Some(mut container) = previous_tree
                    .and_then(|tree| find_node_by_id(tree, node_id))
                    .cloned()
                {
                    container["foreign_toplevel_identifier"] = serde_json::Value::Null;
                    events.push(Event::SwayWindowChanged {
                        change: "close".into(),
                        container,
                    });
                }
                events.push(Event::WindowClosed { id: *id });
            }

            if ipc_win.is_focused {
                ipc_focused_id = Some(id);
            }
        }

        // Sway emits close from container_begin_destroy before seat focus is
        // restored (`sway/tree/container.c:492`; `sway/input/seat.c:260-315`).
        events.append(&mut restored_focus_events);

        // Extra check for focus becoming None, since the checks above only work for focus becoming
        // a different window.
        // Session lock temporarily moves keyboard focus away from the layout,
        // but sway's container focus does not change. Keep the IPC baseline
        // intact so restoring keyboard focus on unlock does not look like a
        // new window focus transition.
        if focused_id.is_none() && ipc_focused_id.is_some() && !self.swayward.is_locked() {
            events.push(Event::WindowFocusChanged { id: None });
        }

        for event in events {
            state.apply(event.clone());
            server.send_event(event);
        }
    }

    pub fn ipc_refresh_overview(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.overview;
        let is_open = self.swayward.layout.is_overview_open();

        if state.is_open == is_open {
            return;
        }

        let event = Event::OverviewOpenedOrClosed { is_open };
        state.apply(event.clone());
        server.send_event(event);
    }

    pub fn ipc_refresh_casts(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let _span = tracy_client::span!("State::ipc_refresh_casts");

        let mut state = server.event_stream_state.borrow_mut();
        let state = &mut state.casts;

        let mut events = Vec::new();
        let mut seen = HashSet::new();

        // Check PipeWire screencasts.
        #[cfg(feature = "xdp-gnome-screencast")]
        {
            // Check pending dynamic casts.
            for pending in &self.swayward.casting.pending_dynamic_casts {
                let stream_id = pending.stream_id.get();
                seen.insert(stream_id);

                // Pending dynamic casts don't change any properties, so we only need to check if
                // it's missing from the state.
                if !state.casts.contains_key(&stream_id) {
                    let cast = swayward_ipc::Cast {
                        session_id: pending.session_id.get(),
                        stream_id,
                        kind: swayward_ipc::CastKind::PipeWire,
                        target: swayward_ipc::CastTarget::Nothing {},
                        is_dynamic_target: true,
                        is_active: false,
                        pid: None,
                        pw_node_id: None,
                    };
                    events.push(Event::CastStartedOrChanged { cast });
                }
            }

            // Check active casts.
            for cast in &self.swayward.casting.casts {
                let stream_id = cast.stream_id.get();
                seen.insert(stream_id);

                let pw_node_id = cast.node_id();
                if state.casts.get(&stream_id).is_none_or(|existing| {
                    // Only these properties can change.
                    existing.is_active != cast.is_active()
                        || !cast.target.matches(&existing.target)
                        || existing.pw_node_id != pw_node_id
                }) {
                    let cast = swayward_ipc::Cast {
                        session_id: cast.session_id.get(),
                        stream_id,
                        kind: swayward_ipc::CastKind::PipeWire,
                        target: cast.target.make_ipc(),
                        is_dynamic_target: cast.dynamic_target,
                        is_active: cast.is_active(),
                        pid: None,
                        pw_node_id,
                    };
                    events.push(Event::CastStartedOrChanged { cast });
                }
            }
        }

        // Check screencopy casts.
        //
        // First, clear expired casts. Ideally we'd have a deadline timer, but our 1 second frame
        // callback timer calls refresh regularly, so that's fine as is.
        self.swayward.screencopy_state.clear_expired_casts();

        for queue in self.swayward.screencopy_state.queues() {
            if let Some(cast_info) = queue.cast() {
                let stream_id = cast_info.stream_id.get();
                seen.insert(stream_id);

                if state.casts.get(&stream_id).is_none_or(|existing| {
                    // Only this property can change.
                    match &existing.target {
                        swayward_ipc::CastTarget::Output { name } => *name != cast_info.output_name,
                        _ => true,
                    }
                }) {
                    let cast = swayward_ipc::Cast {
                        session_id: cast_info.session_id.get(),
                        stream_id,
                        kind: swayward_ipc::CastKind::WlrScreencopy,
                        target: swayward_ipc::CastTarget::Output {
                            name: cast_info.output_name.clone(),
                        },
                        is_dynamic_target: false,
                        is_active: true,
                        pid: queue.credentials().map(|creds| creds.pid),
                        pw_node_id: None,
                    };
                    events.push(Event::CastStartedOrChanged { cast });
                }
            }
        }

        // Check for stopped casts.
        for stream_id in state.casts.keys() {
            if !seen.contains(stream_id) {
                events.push(Event::CastStopped {
                    stream_id: *stream_id,
                });
            }
        }

        for event in events {
            state.apply(event.clone());
            server.send_event(event);
        }
    }

    pub fn ipc_config_loaded(&mut self, failed: bool) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let mut state = server.event_stream_state.borrow_mut();

        let event = Event::ConfigLoaded { failed };
        state.apply(event.clone());
        server.send_event(event);
        if !failed {
            server.send_event(Event::WorkspacesChanged {
                workspaces: state.workspaces.workspaces.values().cloned().collect(),
            });
        }
    }

    pub fn ipc_screenshot_taken(&mut self, path: Option<String>) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let mut state = server.event_stream_state.borrow_mut();

        let event = Event::ScreenshotCaptured { path };
        state.apply(event.clone());
        server.send_event(event);
    }
}

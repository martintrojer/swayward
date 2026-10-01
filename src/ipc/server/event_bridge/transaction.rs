use super::*;

impl State {
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
}

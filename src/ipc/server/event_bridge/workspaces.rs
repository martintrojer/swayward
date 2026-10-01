use super::*;

impl State {
    pub(super) fn ipc_refresh_workspaces(&mut self) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        let _span = tracy_client::span!("State::ipc_refresh_workspaces");

        let previous_tree = serde_json::from_str::<swayward_ipc::Node>(
            &server.query_state.borrow().event_baseline_tree,
        )
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
}

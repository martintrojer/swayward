use super::*;

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
    pub(super) fn ipc_refresh_windows(&mut self, previous_tree: Option<&serde_json::Value>) {
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
}

//! Workspaces: naming, switching, and moving windows and workspaces between them.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::AddNamedWorkspace {
            ws_name,
            output_name,
            layout_config,
        } => {
            layout.ensure_named_workspace(&WorkspaceConfig {
                name: WorkspaceName(format!("ws{ws_name}")),
                sway_output_assignment: None,
                open_on_output: output_name.map(|name| format!("output{name}")),
                layout: layout_config.map(|x| swayward_config::WorkspaceLayoutPart(*x)),
            });
        }
        Op::UnnameWorkspace { ws_name } => {
            layout.unname_workspace(&format!("ws{ws_name}"));
        }
        Op::UpdateWorkspaceLayoutConfig {
            ws_name,
            layout_config,
        } => {
            let ws_name = format!("ws{ws_name}");
            let Some(ws) = layout
                .workspaces_mut()
                .find(|ws| ws.name() == Some(&ws_name))
            else {
                return Applied::Done;
            };

            ws.update_layout_config(layout_config.map(|x| *x));
        }
        Op::SetWorkspaceName {
            new_ws_name,
            ws_name,
        } => {
            let ws_ref = ws_name.map(|ws_name| WorkspaceReference::Name(format!("ws{ws_name}")));
            layout.set_workspace_name(format!("ws{new_ws_name}"), ws_ref);
        }
        Op::UnsetWorkspaceName { ws_name } => {
            let ws_ref = ws_name.map(|ws_name| WorkspaceReference::Name(format!("ws{ws_name}")));
            layout.unset_workspace_name(ws_ref);
        }
        Op::FocusWindowOrWorkspaceDown => layout.focus_window_or_workspace_down(),
        Op::FocusWindowOrWorkspaceUp => layout.focus_window_or_workspace_up(),
        Op::MoveWindowDownOrToWorkspaceDown => layout.move_down_or_to_workspace_down(),
        Op::MoveWindowUpOrToWorkspaceUp => layout.move_up_or_to_workspace_up(),
        Op::FocusWorkspaceDown => layout.switch_workspace_down(),
        Op::FocusWorkspaceUp => layout.switch_workspace_up(),
        Op::FocusWorkspace(idx) => layout.switch_workspace(idx),
        Op::FocusWorkspaceAutoBackAndForth(idx) => layout.switch_workspace_auto_back_and_forth(idx),
        Op::FocusWorkspacePrevious => layout.switch_workspace_previous(),
        Op::MoveWindowToWorkspaceDown(focus) => layout.move_to_workspace_down(focus),
        Op::MoveWindowToWorkspaceUp(focus) => layout.move_to_workspace_up(focus),
        Op::MoveWindowToWorkspace {
            window_id,
            workspace_idx,
        } => {
            let window_id = window_id.filter(|id| layout.has_window(id));
            layout.move_to_workspace(window_id.as_ref(), workspace_idx, ActivateWindow::Smart);
        }
        Op::MoveFocusedToWorkspaceDown(focus) => layout.move_focused_to_workspace_down(focus),
        Op::MoveFocusedToWorkspaceUp(focus) => layout.move_focused_to_workspace_up(focus),
        Op::MoveFocusedToWorkspace(idx, focus) => layout.move_focused_to_workspace(idx, focus),
        Op::MoveWindowToOutput {
            window_id,
            output_id: id,
            target_ws_idx,
        } => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };
            let mon = layout.monitor_for_output(&output).unwrap();

            let window_id = window_id.filter(|id| layout.has_window(id));
            let target_ws_idx = target_ws_idx.filter(|idx| mon.workspaces.len() > *idx);
            layout.move_to_output(
                window_id.as_ref(),
                &output,
                target_ws_idx,
                ActivateWindow::Smart,
            );
        }
        Op::MoveFocusedToOutput {
            output_id: id,
            target_ws_idx,
            activate,
        } => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.move_focused_to_output(&output, target_ws_idx, activate);
        }
        Op::MoveWorkspaceDown => layout.move_workspace_down(),
        Op::MoveWorkspaceUp => layout.move_workspace_up(),
        Op::MoveWorkspaceToIndex {
            ws_name: Some(ws_name),
            target_idx,
        } => {
            let MonitorSet::Normal { monitors, .. } = &mut layout.monitor_set else {
                return Applied::Done;
            };

            let Some((old_idx, old_output)) = monitors.iter().find_map(|monitor| {
                monitor
                    .workspaces
                    .iter()
                    .enumerate()
                    .find_map(|(i, ws)| {
                        if ws.name == Some(format!("ws{ws_name}")) {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .map(|i| (i, monitor.output.clone()))
            }) else {
                return Applied::Done;
            };

            layout.move_workspace_to_idx(Some((Some(old_output), old_idx)), target_idx)
        }
        Op::MoveWorkspaceToIndex {
            ws_name: None,
            target_idx,
        } => layout.move_workspace_to_idx(None, target_idx),
        Op::MoveWorkspaceToMonitor {
            ws_name: None,
            output_id: id,
        } => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };
            layout.move_workspace_to_output(&output);
        }
        Op::MoveWorkspaceToMonitor {
            ws_name: Some(ws_name),
            output_id: id,
        } => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };
            let MonitorSet::Normal { monitors, .. } = &mut layout.monitor_set else {
                return Applied::Done;
            };

            let Some((old_idx, old_output)) = monitors.iter().find_map(|monitor| {
                monitor
                    .workspaces
                    .iter()
                    .enumerate()
                    .find_map(|(i, ws)| {
                        if ws.name == Some(format!("ws{ws_name}")) {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .map(|i| (i, monitor.output.clone()))
            }) else {
                return Applied::Done;
            };

            let workspace_id =
                layout.monitor_for_output(&old_output).unwrap().workspaces[old_idx].id();
            layout.move_workspace_to_output_by_id(workspace_id, Some(old_output), &output);
        }
        Op::MoveFocusedContainerToNextWorkspace => {
            let target = layout.active_workspace().and_then(|workspace| {
                workspace
                    .focused_tiling_node()
                    .map(|node| (workspace.id(), node))
            });
            if let Some((workspace, node)) = target {
                let _ = layout.move_tiling_subtree_to_sway_workspace(
                    workspace,
                    node,
                    swayward_ipc::command::WorkspaceTarget::Next,
                    false,
                    false,
                );
            }
        }
        Op::MoveWorkspaceToOutput(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.move_workspace_to_output(&output);
        }
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}

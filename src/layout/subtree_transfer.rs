//! Moves and swaps of tiling subtrees within and between workspaces.

use super::*;

impl<W: LayoutElement> Layout<W> {
    pub fn detach_floating_group_child(&mut self, window: &W::Id) -> bool {
        self.workspaces_mut()
            .find(|workspace| workspace.has_window(window))
            .is_some_and(|workspace| workspace.detach_floating_group_child(window))
    }

    pub fn is_tiling_root(&self, workspace: WorkspaceId, node: tiling_tree::NodeId) -> bool {
        self.workspace(workspace)
            .is_some_and(|candidate| candidate.tiling().is_root(node))
    }

    pub fn workspace_contains_tiling_node(
        &self,
        workspace: WorkspaceId,
        node: tiling_tree::NodeId,
    ) -> bool {
        self.workspace(workspace)
            .is_some_and(|candidate| candidate.tiling().contains(node))
    }

    pub fn swap_tiling_nodes(
        &mut self,
        workspace: WorkspaceId,
        first: tiling_tree::NodeId,
        second: tiling_tree::NodeId,
    ) -> Result<(), String> {
        let workspace = self
            .workspace_mut(workspace)
            .ok_or_else(|| "No matching node.".to_owned())?;
        workspace
            .swap_tiling_nodes(first, second)
            .map_err(str::to_owned)
    }

    pub(crate) fn swap_tiling_nodes_between_workspaces(
        &mut self,
        first_workspace: WorkspaceId,
        first: tiling_tree::NodeId,
        second_workspace: WorkspaceId,
        second: tiling_tree::NodeId,
    ) -> Result<SwapRemap, String> {
        let fits = |workspace: WorkspaceId, slot: tiling_tree::NodeId, other_ws, other| {
            let height = self
                .workspace(other_ws)
                .map(|candidate| candidate.tiling().node_height(other));
            self.workspace(workspace)
                .zip(height)
                .is_some_and(|(candidate, height)| candidate.tiling().fits_at(slot, height))
        };
        if !fits(first_workspace, first, second_workspace, second)
            || !fits(second_workspace, second, first_workspace, first)
        {
            return Err(tiling_tree::TOO_DEEP.to_owned());
        }
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot swap containers without an output".into());
        };
        let (first_ws, second_ws) =
            Self::distinct_workspaces_mut(monitors, first_workspace, second_workspace)
                .ok_or_else(|| "No matching node.".to_owned())?;
        let (mut first_subtree, first_slot) = first_ws
            .detach_tiling_subtree_for_swap(first)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let (mut second_subtree, second_slot) = second_ws
            .detach_tiling_subtree_for_swap(second)
            .ok_or_else(|| "No matching node.".to_owned())?;
        first_subtree.swap_fullscreen_position(&mut second_subtree);
        let second_remapped = first_ws
            .attach_tiling_subtree_for_swap(second_subtree, first_slot)
            .1;
        let first_remapped = second_ws
            .attach_tiling_subtree_for_swap(first_subtree, second_slot)
            .1;
        first_ws.tiling_mut().finish_subtree_detach(None);
        second_ws.tiling_mut().finish_subtree_detach(None);
        Ok(SwapRemap {
            first: first_remapped,
            second: second_remapped,
        })
    }

    /// Mutable borrows of two distinct workspaces, wherever they live.
    ///
    /// Returns `None` when either is missing or both name the same workspace,
    /// so callers report "No matching node." instead of panicking on a stale
    /// id or an aliasing borrow.
    fn distinct_workspaces_mut(
        monitors: &mut [Monitor<W>],
        first: WorkspaceId,
        second: WorkspaceId,
    ) -> Option<(&mut Workspace<W>, &mut Workspace<W>)> {
        if first == second {
            return None;
        }
        let first_monitor = monitors.iter().position(|monitor| monitor.has_ws(first))?;
        let second_monitor = monitors.iter().position(|monitor| monitor.has_ws(second))?;
        if first_monitor == second_monitor {
            let monitor = &mut monitors[first_monitor];
            let first_idx = monitor.idx_of_ws(first)?;
            let second_idx = monitor.idx_of_ws(second)?;
            if first_idx < second_idx {
                let (before, after) = monitor.workspaces.split_at_mut(second_idx);
                Some((&mut before[first_idx], after.first_mut()?))
            } else {
                let (before, after) = monitor.workspaces.split_at_mut(first_idx);
                Some((after.first_mut()?, &mut before[second_idx]))
            }
        } else if first_monitor < second_monitor {
            let (before, after) = monitors.split_at_mut(second_monitor);
            let first_monitor = &mut before[first_monitor];
            let second_monitor = after.first_mut()?;
            let first_idx = first_monitor.idx_of_ws(first)?;
            let second_idx = second_monitor.idx_of_ws(second)?;
            Some((
                &mut first_monitor.workspaces[first_idx],
                &mut second_monitor.workspaces[second_idx],
            ))
        } else {
            let (before, after) = monitors.split_at_mut(first_monitor);
            let first_monitor = after.first_mut()?;
            let second_monitor = &mut before[second_monitor];
            let first_idx = first_monitor.idx_of_ws(first)?;
            let second_idx = second_monitor.idx_of_ws(second)?;
            Some((
                &mut first_monitor.workspaces[first_idx],
                &mut second_monitor.workspaces[second_idx],
            ))
        }
    }

    pub fn move_tiling_subtree_to_node(
        &mut self,
        source_workspace: WorkspaceId,
        source: tiling_tree::NodeId,
        target_workspace: WorkspaceId,
        target: tiling_tree::NodeId,
    ) -> Result<Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>, String> {
        if source_workspace == target_workspace {
            let workspace = self
                .workspace_mut(source_workspace)
                .ok_or_else(|| "No matching node.".to_owned())?;
            if !workspace.tiling().contains(source) || !workspace.tiling().contains(target) {
                return Err("No matching node.".to_owned());
            }
            workspace.tiling_mut().move_subtree_to_node(source, target);
            Ok(Vec::new())
        } else {
            let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
                return Err("cannot move a container without an output".into());
            };
            let source_monitor = monitors
                .iter()
                .position(|monitor| monitor.has_ws(source_workspace))
                .ok_or_else(|| "No matching node.".to_owned())?;
            let (source_ws, target_ws) =
                Self::distinct_workspaces_mut(monitors, source_workspace, target_workspace)
                    .ok_or_else(|| "No matching node.".to_owned())?;
            let (subtree, old_parent) = source_ws
                .detach_tiling_subtree(source)
                .ok_or_else(|| "No matching node.".to_owned())?;
            let remapped = target_ws.attach_tiling_subtree_at(subtree, Some(target)).1;
            source_ws.tiling_mut().finish_subtree_detach(old_parent);
            if monitors[source_monitor].workspace_switch.is_none() {
                monitors[source_monitor].clean_up_workspaces();
            }
            Ok(remapped)
        }
    }

    pub fn tiling_target_for_window(
        &self,
        window: &W::Id,
    ) -> Option<(WorkspaceId, tiling_tree::NodeId)> {
        self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .tiling()
                .node_for_window(window)
                .map(|node| (workspace.id(), node))
        })
    }

    pub fn swap_target_for_window(
        &self,
        window: &W::Id,
    ) -> Option<(WorkspaceId, tiling_tree::NodeId)> {
        self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .swap_node_for_window(window)
                .map(|node| (workspace.id(), node))
        })
    }

    pub fn move_tiling_subtree_to_sway_workspace(
        &mut self,
        source_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
        target: crate::command::WorkspaceTarget,
        preserve_empty_workspace: bool,
        auto_back_and_forth: bool,
    ) -> Result<(WorkspaceId, Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>), String> {
        if let Some(window) = self.floating_group_window(source_workspace, node) {
            let target_workspace =
                self.move_floating_group_to_sway_workspace(&window, target, auto_back_and_forth)?;
            return Ok((target_workspace, Vec::new()));
        }
        let (floating, empty_root) = self
            .workspace(source_workspace)
            .filter(|workspace| workspace.tiling().is_root(node))
            .map(|workspace| {
                (
                    workspace.floating_transfer_window_ids(),
                    workspace.tiling().tiles().next().is_none(),
                )
            })
            .unwrap_or_default();
        let target =
            self.resolve_move_workspace_target(source_workspace, target, auto_back_and_forth);
        let floating_target = target.clone();
        let (target_output, target_index) = self.resolve_sway_workspace_target(target)?;
        let target_workspace = match target_output.as_ref() {
            Some(output) => self
                .monitor_for_output(output)
                .and_then(|monitor| monitor.workspaces.get(target_index))
                .map(Workspace::id),
            None => self
                .workspaces()
                .nth(target_index)
                .map(|(_, _, workspace)| workspace.id()),
        }
        .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if source_workspace == target_workspace {
            // Sway wraps a focused workspace's tiling children before it
            // resolves the destination (`workspace_wrap_children` in
            // `cmd_move_container`, sway/commands/move.c:430-436), so the wrapper
            // survives even when the destination is the same workspace.
            if let Some(workspace) = self
                .workspace_mut(source_workspace)
                .filter(|workspace| workspace.tiling().is_root(node))
            {
                workspace.tiling_mut().wrap_workspace_children();
            }
            return Ok((target_workspace, Vec::new()));
        }
        if empty_root {
            for window in floating {
                self.move_window_to_sway_workspace(&window, floating_target.clone(), false)?;
            }
            return Ok((target_workspace, Vec::new()));
        }

        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot move a container without an output".into());
        };
        let source_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(source_workspace))
            .ok_or_else(|| "No matching node.".to_owned())?;
        let target_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(target_workspace))
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if source_monitor == target_monitor {
            let remapped = monitors[source_monitor]
                .move_tiling_subtree_to_workspace(
                    source_workspace,
                    node,
                    target_workspace,
                    preserve_empty_workspace || !floating.is_empty(),
                )
                .ok_or_else(|| "No matching node.".to_owned())?;
            for window in floating {
                self.move_window_to_sway_workspace(&window, floating_target.clone(), false)?;
            }
            return Ok((target_workspace, remapped));
        }

        let remapped = Self::move_tiling_subtree_between_monitors(
            monitors,
            (source_monitor, source_workspace),
            (target_monitor, target_workspace),
            node,
            preserve_empty_workspace || !floating.is_empty(),
        )?;
        for window in floating {
            self.move_window_to_sway_workspace(&window, floating_target.clone(), false)?;
        }
        Ok((target_workspace, remapped))
    }
    /// The window of `node` when it is a floating group root on
    /// `source_workspace`.
    fn floating_group_window(
        &self,
        source_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
    ) -> Option<W::Id> {
        self.workspace(source_workspace).and_then(|workspace| {
            let floating = workspace.floating();
            floating.tree(node)?;
            floating.window_in_node(node).cloned()
        })
    }

    /// Moves the floating group holding `window` and returns the workspace it
    /// landed on.
    fn move_floating_group_to_sway_workspace(
        &mut self,
        window: &W::Id,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> Result<WorkspaceId, String> {
        self.move_window_to_sway_workspace(window, target, auto_back_and_forth)?;
        self.workspaces()
            .find(|(_, _, workspace)| workspace.has_window(window))
            .map(|(_, _, workspace)| workspace.id())
            .ok_or_else(|| "No matching node.".to_owned())
    }

    /// Moves the tiling subtree at `node` between workspaces on two different
    /// monitors, cleaning up the source monitor unless `preserve_source`.
    fn move_tiling_subtree_between_monitors(
        monitors: &mut [Monitor<W>],
        (source_monitor, source_workspace): (usize, WorkspaceId),
        (target_monitor, target_workspace): (usize, WorkspaceId),
        node: tiling_tree::NodeId,
        preserve_source: bool,
    ) -> Result<Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>, String> {
        let (source, target) = if source_monitor < target_monitor {
            let (before_target, target_and_after) = monitors.split_at_mut(target_monitor);
            (&mut before_target[source_monitor], &mut target_and_after[0])
        } else {
            let (before_source, source_and_after) = monitors.split_at_mut(source_monitor);
            (&mut source_and_after[0], &mut before_source[target_monitor])
        };
        let source_idx = source
            .idx_of_ws(source_workspace)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let target_idx = target
            .idx_of_ws(target_workspace)
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        let (subtree, old_parent) = source.workspaces[source_idx]
            .detach_tiling_subtree(node)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let remapped = target.workspaces[target_idx]
            .attach_tiling_subtree(subtree)
            .1;
        source.workspaces[source_idx]
            .tiling_mut()
            .finish_subtree_detach(old_parent);
        if !preserve_source && source.workspace_switch.is_none() {
            source.clean_up_workspaces();
        }
        Ok(remapped)
    }
    /// Drops an interactively moved tile onto `target`, swapping the two.
    ///
    /// Sway swaps a centre drop with the container under the pointer
    /// (seatop_move_tiling.c:365-388). Returns the displaced tile when it must
    /// go back to `source_workspace` on another workspace.
    pub(super) fn drop_tile_swapping_with(
        mon: &mut Monitor<W>,
        ws_idx: usize,
        target: tiling_tree::NodeId,
        tile: Tile<W>,
        source_workspace: WorkspaceId,
        allow_to_activate_workspace: bool,
    ) -> Option<RemovedTile<W>> {
        let mut displaced = None;
        let ws_id = mon.workspaces[ws_idx].id();
        let target_window = mon.workspaces[ws_idx]
            .tiling()
            .window_for_node(target)
            .map(|window| window.id().clone());
        let moved_window = tile.window().id().clone();
        if source_workspace != ws_id {
            if let Some(target_window) = &target_window {
                displaced =
                    Some(mon.workspaces[ws_idx].remove_tile(target_window, Transaction::new()));
            }
        }
        mon.add_tile(
            tile,
            MonitorAddWindowTarget::Workspace {
                id: ws_id,
                column_idx: None,
            },
            ActivateWindow::Yes,
            allow_to_activate_workspace,
            false,
        );
        if source_workspace == ws_id {
            if let Some(target_window) = target_window {
                let workspace = &mut mon.workspaces[ws_idx];
                if let (Some(first), Some(second)) = (
                    workspace.tiling().node_for_window(&moved_window),
                    workspace.tiling().node_for_window(&target_window),
                ) {
                    let _ = workspace.swap_tiling_nodes(first, second);
                }
            }
        }
        displaced
    }
}

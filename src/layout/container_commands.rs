//! Sway container commands on the focused or a named tiling node: border,
//! sticky, nest, split, flatten and layout.

use super::*;

impl<W: LayoutElement> Layout<W> {
    pub fn window_border(
        &self,
        window: &W::Id,
    ) -> Option<(swayward_ipc::command::BorderStyle, u16)> {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return Some(move_.tile.sway_border());
            }
        }
        if let Some(removed) = self
            .scratchpad
            .iter()
            .find(|removed| removed.tile.window().id() == window)
        {
            return Some(removed.tile.sway_border());
        }
        self.workspaces()
            .find(|(_, _, workspace)| workspace.has_window(window))
            .and_then(|(_, _, workspace)| workspace.window_border(window))
    }

    pub fn set_window_border(
        &mut self,
        window: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> Result<(), &'static str> {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if move_.tile.window().id() == window {
                return move_
                    .tile
                    .set_sway_border(style, width, move_.is_floating)
                    .map(|_| ());
            }
        }
        if let Some(removed) = self
            .scratchpad
            .iter_mut()
            .find(|removed| removed.tile.window().id() == window)
        {
            return removed.tile.set_sway_border(style, width, true).map(|_| ());
        }
        self.workspaces_mut()
            .find(|workspace| workspace.has_window(window))
            .ok_or("Only views can have borders")?
            .set_window_border(window, style, width)
    }

    pub fn set_split_sticky(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        value: &str,
    ) -> bool {
        self.workspace_mut(workspace_id)
            .is_some_and(|workspace| workspace.set_split_sticky(node, value))
    }

    pub fn set_window_sticky(&mut self, window: &W::Id, value: &str) -> bool {
        self.set_sticky(window, None, value)
    }

    /// Applies `sticky` to a floating group root itself rather than to one of its
    /// children, as sway sets `is_sticky` on the focused container
    /// (`sway/commands/sticky.c:20-26`). Returns `None` when `node` is not a
    /// floating group root in `workspace`.
    pub fn set_floating_group_sticky(
        &mut self,
        workspace: WorkspaceId,
        node: NodeId,
        value: &str,
    ) -> Option<bool> {
        let window = self.workspace(workspace).and_then(|candidate| {
            candidate.floating().tree(node)?;
            candidate.floating().window_in_node(node).cloned()
        })?;
        Some(self.set_sticky(&window, Some(node), value))
    }

    fn set_sticky(&mut self, window: &W::Id, group: Option<NodeId>, value: &str) -> bool {
        let current = self.workspaces().any(|(_, _, workspace)| match group {
            Some(root) => workspace.floating().tree_is_sticky(root),
            None => workspace.is_window_sticky(window),
        });
        let sticky = swayward_ipc::command::parse_boolean(value, current);
        let Some(monitor) = self
            .monitors_mut()
            .find(|monitor| monitor.has_window(window))
        else {
            return false;
        };
        let Some(source) = monitor
            .workspaces
            .iter()
            .find(|workspace| workspace.has_window(window))
            .map(Workspace::id)
        else {
            return false;
        };
        let Some(source_idx) = monitor.idx_of_ws(source) else {
            return false;
        };
        let whole_tree =
            group.is_some() || monitor.workspaces[source_idx].window_is_floating_root(window);
        let changed = match group {
            Some(root) => monitor.workspaces[source_idx].set_floating_tree_sticky(root, sticky),
            None => monitor.workspaces[source_idx].set_window_sticky(window, sticky),
        };
        if !changed {
            return true;
        }
        let target = monitor.active_workspace_ref().id();
        if whole_tree && sticky && source != target {
            // The active workspace is on this monitor by construction.
            let Some(target_idx) = monitor.idx_of_ws(target) else {
                warn!("set_sticky: active workspace is not on its own monitor");
                return true;
            };
            let removed_trees = monitor.workspaces[source_idx].take_sticky_trees();
            let removed = monitor.workspaces[source_idx].take_sticky_tiles();
            for removed in removed_trees {
                monitor.workspaces[target_idx].add_floating_tree(removed, false);
            }
            for removed in removed {
                monitor.workspaces[target_idx].add_tile(
                    removed.tile,
                    WorkspaceAddWindowTarget::Auto,
                    workspace::AddTileOptions {
                        activate: ActivateWindow::Yes,
                        is_floating: true,
                    },
                );
            }
            if monitor.workspace_switch.is_none() {
                monitor.clean_up_workspaces();
            }
        }
        true
    }

    pub fn nest_focused_window(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.nest_focused_window();
    }

    pub fn unnest_focused_window(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.unnest_focused_window();
    }

    pub fn swap_window_horizontal(&mut self, right: bool) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.swap_window_horizontal(right);
    }

    pub fn toggle_focused_tabbed_display(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.toggle_focused_tabbed_display();
    }

    pub fn set_focused_layout(
        &mut self,
        layout: tiling_tree::Layout,
    ) -> Option<(WorkspaceId, Vec<(NodeId, NodeId)>)> {
        let workspace = self.active_workspace_mut()?;
        let id = workspace.id();
        Some((id, workspace.set_focused_layout(layout)))
    }

    pub fn split_focused(&mut self, layout: tiling_tree::Layout) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.split_focused(layout);
        }
    }

    pub fn flatten_focused_parent(&mut self) -> Option<(WorkspaceId, Vec<(NodeId, NodeId)>)> {
        let workspace = self.active_workspace_mut()?;
        let id = workspace.id();
        workspace
            .flatten_focused_parent()
            .map(|remapped| (id, vec![remapped]))
    }

    pub fn flatten_tiling_node_parent(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
    ) -> Option<(WorkspaceId, Vec<(NodeId, NodeId)>)> {
        let workspace = self.workspaces_mut().find(|workspace| {
            workspace.id() == workspace_id && workspace.tiling().contains(node)
        })?;
        workspace
            .tiling_mut()
            .flatten_parent(node)
            .map(|remapped| (workspace_id, vec![remapped]))
    }

    pub fn split_tiling_node(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        layout: tiling_tree::Layout,
    ) -> bool {
        let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.id() == workspace_id && workspace.tiling().contains(node))
        else {
            return false;
        };
        workspace.tiling_mut().split(node, layout);
        true
    }

    pub fn toggle_tiling_node_split(&mut self, workspace_id: WorkspaceId, node: NodeId) -> bool {
        let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.id() == workspace_id && workspace.tiling().contains(node))
        else {
            return false;
        };
        workspace.tiling_mut().toggle_split(node);
        true
    }

    pub fn set_tiling_node_layout_exact(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        layout: tiling_tree::Layout,
    ) -> bool {
        let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.id() == workspace_id && workspace.tiling().contains(node))
        else {
            return false;
        };
        workspace.tiling_mut().set_layout(node, layout);
        true
    }

    pub fn set_tiling_target_layout(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        layout: tiling_tree::Layout,
    ) -> bool {
        self.workspace_mut(workspace_id)
            .is_some_and(|workspace| workspace.tiling_mut().set_target_layout(node, layout))
    }

    pub fn toggle_tiling_target_layout(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        toggle: &swayward_ipc::command::LayoutToggle,
        container: bool,
    ) -> bool {
        self.workspace_mut(workspace_id)
            .is_some_and(|workspace| workspace.toggle_tiling_target_layout(node, toggle, container))
    }

    pub fn restore_tiling_target_layout(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        container: bool,
    ) -> bool {
        self.workspace_mut(workspace_id)
            .is_some_and(|workspace| workspace.restore_tiling_target_layout(node, container))
    }

    pub fn set_tiling_node_title_format(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        format: String,
    ) -> bool {
        self.workspace_mut(workspace_id)
            .is_some_and(|workspace| workspace.tiling_mut().set_title_format(node, format))
    }

    pub fn set_container_floating(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        floating: bool,
    ) -> Option<NodeId> {
        self.workspace_mut(workspace_id)?
            .set_container_floating(node, floating)
    }

    /// The workspace whose tiling tree or floating groups hold `node`.
    pub fn workspace_containing_node(&self, node: NodeId) -> Option<WorkspaceId> {
        self.workspaces()
            .find(|(_, _, workspace)| workspace.contains_swap_node(node))
            .map(|(_, _, workspace)| workspace.id())
    }

    pub fn window_in_node(&self, workspace_id: WorkspaceId, node: NodeId) -> Option<W::Id> {
        let workspace = self.workspace(workspace_id)?;
        workspace
            .tiling_node_windows(node)
            .and_then(|windows| windows.into_iter().next())
            .or_else(|| workspace.floating().window_in_node(node).cloned())
    }

    pub fn tiling_node_windows(
        &self,
        workspace_id: WorkspaceId,
        node: NodeId,
    ) -> Option<Vec<W::Id>> {
        self.workspace(workspace_id)?.tiling_node_windows(node)
    }

    pub fn toggle_focused_layout(
        &mut self,
        toggle: &swayward_ipc::command::LayoutToggle,
    ) -> Option<(WorkspaceId, Vec<(NodeId, NodeId)>)> {
        let workspace = self.active_workspace_mut()?;
        let id = workspace.id();
        Some((id, workspace.toggle_focused_layout(toggle)))
    }

    pub fn restore_focused_split_layout(&mut self) -> Option<(WorkspaceId, Vec<(NodeId, NodeId)>)> {
        let workspace = self.active_workspace_mut()?;
        let id = workspace.id();
        Some((id, workspace.restore_focused_split_layout()?))
    }

    pub fn toggle_focused_layout_split(&mut self) -> Option<(WorkspaceId, Vec<(NodeId, NodeId)>)> {
        let workspace = self.active_workspace_mut()?;
        let id = workspace.id();
        Some((id, workspace.toggle_focused_layout_split()))
    }

    pub fn toggle_focused_split(&mut self) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.toggle_focused_split();
        }
    }

    pub fn set_focused_display(&mut self, display: ColumnDisplay) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.set_focused_display(display);
    }
}

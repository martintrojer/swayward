//! The sway scratchpad: hiding windows and floating trees and showing them again.

use super::*;

impl<W: LayoutElement> Layout<W> {
    pub fn move_to_scratchpad(&mut self, window: Option<&W::Id>) {
        let window = window
            .cloned()
            .or_else(|| self.focus().map(|window| window.id().clone()));
        let Some(window) = window else {
            return;
        };
        self.finish_starting_interactive_move(&window);
        if self
            .scratchpad
            .iter()
            .any(|removed| removed.tile.window().id() == &window)
            || self
                .scratchpad_trees
                .iter()
                .any(|removed| removed.contains_window(&window))
        {
            return;
        }
        let floating_tree = self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .floating_tree_root_for_window(&window)
                .map(|root| (workspace.id(), root))
        });
        if let Some((source_workspace, root)) = floating_tree {
            let removed = {
                let Some(workspace) = self
                    .workspaces_mut()
                    .find(|workspace| workspace.id() == source_workspace)
                else {
                    return;
                };
                workspace.clear_floating_tree_fullscreen(root);
                let Some(removed) = workspace.remove_floating_tree(root) else {
                    warn!("move_to_scratchpad: floating tree root reported for the window is gone");
                    return;
                };
                removed
            };
            for id in removed.window_ids() {
                if !self.scratchpad_windows.contains(id) {
                    self.scratchpad_windows.push(id.clone());
                }
            }
            self.scratchpad_trees.push_back(removed);
            self.clean_up_removed_window_workspace(source_workspace);
            return;
        }
        let automatic_maximum = self.output_layout_size();
        let mut floating_working_area = None;
        if let Some(workspace) = self.workspaces_mut().find(|ws| ws.has_window(&window)) {
            workspace.prepare_tiled_window_for_scratchpad(&window, automatic_maximum);
            if workspace.fullscreen_contains_window(&window) {
                workspace.set_fullscreen(&window, false);
            }
            floating_working_area = Some(workspace.working_area());
        }
        let Some((mut removed, source_workspace)) =
            self.detach_window_inner(&window, Transaction::new(), true)
        else {
            return;
        };
        removed.floating_working_area = floating_working_area;
        if !self.scratchpad_windows.contains(&window) {
            self.scratchpad_windows.push(window);
        }
        self.scratchpad.push_back(removed);
        if let Some(source_workspace) = source_workspace {
            self.clean_up_removed_window_workspace(source_workspace);
        }
    }

    /// Moves scratchpad tree `index` onto `workspace` and returns the window
    /// it shows. The tree stays hidden if the workspace or its first window is
    /// missing, instead of being dropped.
    fn show_scratchpad_tree(&mut self, index: usize, workspace: WorkspaceId) -> Option<W::Id> {
        let shown = self
            .scratchpad_trees
            .get(index)?
            .window_ids()
            .first()?
            .clone();
        if !self
            .workspaces()
            .any(|(_, _, candidate)| candidate.id() == workspace)
        {
            return None;
        }
        let removed = self.scratchpad_trees.remove(index)?;
        self.workspaces_mut()
            .find(|candidate| candidate.id() == workspace)?
            .add_floating_tree(removed, true);
        Some(shown)
    }

    pub fn show_scratchpad(&mut self, window: Option<&W::Id>) -> Option<W::Id> {
        let focused = self.focus().map(|window| window.id().clone());
        let shown = focused
            .filter(|id| self.scratchpad_windows.contains(id) && !self.is_scratchpad_hidden(id))
            .or_else(|| {
                self.scratchpad_windows
                    .iter()
                    .find(|id| !self.is_scratchpad_hidden(id))
                    .cloned()
            });
        let target_tree = window
            .and_then(|window| {
                self.scratchpad_trees
                    .iter()
                    .position(|removed| removed.contains_window(window))
            })
            .or_else(|| {
                (window.is_none() && shown.is_none() && self.scratchpad.is_empty())
                    .then_some(0)
                    .filter(|_| !self.scratchpad_trees.is_empty())
            });
        if let Some(index) = target_tree {
            let active_workspace = self.active_workspace()?.id();
            for workspace in self.workspaces_mut() {
                let disables_fullscreen = workspace.id() == active_workspace
                    || workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global);
                if disables_fullscreen {
                    workspace.disable_fullscreen();
                }
            }
            return self.show_scratchpad_tree(index, active_workspace);
        }
        let mut target_index = window.and_then(|window| {
            self.scratchpad
                .iter()
                .position(|removed| removed.tile.window().id() == window)
        });
        if let Some(window) = window {
            if target_index.is_none() {
                let on_active_workspace = self
                    .active_workspace()
                    .is_some_and(|workspace| workspace.has_window(window));
                self.move_to_scratchpad(Some(window));
                if on_active_workspace {
                    return None;
                }
                target_index = self
                    .scratchpad
                    .iter()
                    .position(|removed| removed.tile.window().id() == window);
            }
        } else if let Some(shown) = shown {
            if self.focus().is_some_and(|focused| focused.id() == &shown) {
                self.move_to_scratchpad(Some(&shown));
                return None;
            }
            self.move_to_scratchpad(Some(&shown));
            if let Some(index) = self
                .scratchpad_trees
                .iter()
                .position(|removed| removed.contains_window(&shown))
            {
                let active_workspace = self.active_workspace()?.id();
                return self.show_scratchpad_tree(index, active_workspace);
            }
            target_index = self
                .scratchpad
                .iter()
                .position(|removed| removed.tile.window().id() == &shown);
        }

        let index = target_index.unwrap_or(0);
        self.scratchpad.get(index)?;
        let active_workspace = self.active_workspace()?.id();
        for workspace in self.workspaces_mut() {
            let disables_fullscreen = workspace.id() == active_workspace
                || workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global);
            if disables_fullscreen {
                workspace.disable_fullscreen();
            }
        }

        // Find the destination before taking the window out of the scratchpad,
        // so a missing workspace leaves it hidden rather than dropping it.
        if !self
            .workspaces()
            .any(|(_, _, workspace)| workspace.id() == active_workspace)
        {
            return None;
        }
        let mut removed = self.scratchpad.remove(index)?;
        removed.is_floating = true;
        let shown = removed.tile.window().id().clone();
        let workspace = self
            .workspaces_mut()
            .find(|workspace| workspace.id() == active_workspace)?;
        workspace.remap_floating_position(&mut removed.tile, removed.floating_working_area);
        workspace.add_tile(
            removed.tile,
            WorkspaceAddWindowTarget::Auto,
            workspace::AddTileOptions {
                activate: ActivateWindow::Yes,
                width: removed.width,
                is_full_width: removed.is_full_width,
                is_floating: true,
                anim: None,
            },
        );
        Some(shown)
    }

    pub fn scratchpad_tiles(&self) -> impl Iterator<Item = (&W, bool)> {
        self.scratchpad
            .iter()
            .map(|removed| (removed.tile.window(), removed.tile.is_sticky))
    }

    pub fn scratchpad_windows(&self) -> impl Iterator<Item = &W> {
        self.scratchpad
            .iter()
            .map(|removed| removed.tile.window())
            .chain(
                self.scratchpad_trees
                    .iter()
                    .flat_map(|removed| removed.windows()),
            )
    }

    pub fn scratchpad_trees(
        &self,
    ) -> impl Iterator<Item = (tiling_tree::IpcNode<W::Id>, bool)> + '_ {
        self.scratchpad_trees
            .iter()
            .map(|removed| (removed.ipc_tree(), removed.is_sticky()))
    }

    /// Every node of each hidden scratchpad group, with one of its windows.
    ///
    /// Sway's criteria and GET_MARKS walk hidden scratchpad containers too
    /// (`sway/sway/tree/root.c:250-257`), so a mark on a hidden group still
    /// finds it. The window is the group's representative for commands such
    /// as `scratchpad show`, which act on the whole group.
    pub fn scratchpad_tree_nodes(&self) -> impl Iterator<Item = (NodeId, &W::Id)> + '_ {
        self.scratchpad_trees.iter().flat_map(|removed| {
            let window = removed.window_ids().first();
            removed
                .ipc_tree()
                .nodes()
                .into_iter()
                .filter_map(move |(node, _)| Some((node, window?)))
        })
    }

    pub fn scratchpad_is_empty(&self) -> bool {
        self.scratchpad_windows.is_empty()
    }

    pub fn is_scratchpad_window(&self, window: &W::Id) -> bool {
        self.scratchpad_windows.contains(window)
    }

    pub fn window_is_on_visible_workspace(&self, window: &W::Id) -> bool {
        self.workspaces().any(|(monitor, index, workspace)| {
            workspace.has_window(window)
                && monitor.is_some_and(|monitor| monitor.active_workspace_idx() == index)
        })
    }

    pub fn is_scratchpad_hidden(&self, window: &W::Id) -> bool {
        self.scratchpad
            .iter()
            .any(|removed| removed.tile.window().id() == window)
            || self
                .scratchpad_trees
                .iter()
                .any(|removed| removed.contains_window(window))
    }

    /// Returning a container to tiling removes it from the scratchpad
    /// (sway/tree/container.c:990-994).
    pub(super) fn forget_scratchpad_window_if_tiled(&mut self, window: &W::Id) {
        if !self.scratchpad_windows.contains(window) {
            return;
        }
        let tiled = self
            .workspaces()
            .any(|(_, _, ws)| ws.has_window(window) && !ws.is_floating(window));
        if tiled {
            self.scratchpad_windows.retain(|id| id != window);
        }
    }
}

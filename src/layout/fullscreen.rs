//! Sway fullscreen modes on windows and tiling containers.

use super::*;

impl<W: LayoutElement> Layout<W> {
    pub fn focused_fullscreen_mode(&self) -> Option<tiling_tree::FullscreenMode> {
        self.active_workspace().and_then(Workspace::fullscreen_mode)
    }

    pub fn global_fullscreen_active(&self) -> bool {
        self.workspaces().any(|(_, _, workspace)| {
            workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global)
        })
    }

    pub fn focused_window_is_fullscreen_or_child(&self) -> bool {
        let Some(window) = self.focus() else {
            return false;
        };
        self.active_workspace()
            .is_some_and(|workspace| workspace.fullscreen_contains_window(window.id()))
    }

    pub fn disable_active_workspace_fullscreen(&mut self) {
        let window = self.active_workspace().and_then(|workspace| {
            let fullscreen = workspace.tiling().fullscreen_node()?;
            workspace.tiling().windows().find_map(|(id, window)| {
                workspace
                    .tiling()
                    .contains_node(fullscreen, id)
                    .then(|| window.id().clone())
            })
        });
        if let (Some(workspace), Some(window)) = (self.active_workspace_mut(), window) {
            workspace.set_fullscreen(&window, false);
        }
    }

    pub fn set_focused_fullscreen_mode(&mut self, mode: Option<tiling_tree::FullscreenMode>) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.disable_fullscreen();
                    break;
                }
            }
        }
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.set_focused_fullscreen(mode);
        }
    }

    pub fn tiling_node_fullscreen_mode(
        &self,
        workspace_id: WorkspaceId,
        node: NodeId,
    ) -> Option<Option<tiling_tree::FullscreenMode>> {
        let workspace = self.workspace(workspace_id)?;
        workspace
            .tiling()
            .contains(node)
            .then(|| workspace.tiling().fullscreen_mode(node))
    }

    pub fn set_tiling_node_fullscreen_mode(
        &mut self,
        workspace_id: WorkspaceId,
        node: NodeId,
        mode: Option<tiling_tree::FullscreenMode>,
    ) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.disable_fullscreen();
                    break;
                }
            }
        }
        if let Some(workspace) = self.workspace_mut(workspace_id) {
            workspace.tiling_mut().set_node_fullscreen(node, mode);
        }
        if mode == Some(tiling_tree::FullscreenMode::Global) {
            let target = self
                .workspace(workspace_id)
                .and_then(|workspace| workspace.fullscreen_window().cloned());
            if let Some(window) = target {
                self.activate_window(&window);
            }
        }
    }

    pub fn fullscreen_mode(&self, id: &W::Id) -> Option<tiling_tree::FullscreenMode> {
        self.workspaces()
            .find_map(|(_, _, workspace)| workspace.fullscreen_mode_for_window(id))
    }

    pub fn set_fullscreen_mode(&mut self, id: &W::Id, mode: Option<tiling_tree::FullscreenMode>) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.disable_fullscreen();
                    break;
                }
            }
        }
        if mode == Some(tiling_tree::FullscreenMode::Global) {
            self.activate_window(id);
        }
        if let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.has_window(id))
        {
            workspace.activate_window(id);
            if workspace.is_floating(id) {
                workspace.set_window_fullscreen(id, mode);
                workspace.activate_window(id);
                if mode == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.set_focused_fullscreen(mode);
                }
            } else {
                workspace.set_focused_fullscreen(mode);
            }
        }
    }
}

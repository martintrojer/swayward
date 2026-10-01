use super::*;

impl<W: LayoutElement> Monitor<W> {
    /// The workspace that stays behind when `moving` leaves this monitor.
    ///
    /// Reuses an unnamed empty workspace when there is one, giving it
    /// `identity`, and otherwise adds one at index 1 with `layout_config`.
    pub(in crate::layout) fn ensure_replacement_workspace(
        &mut self,
        moving: WorkspaceId,
        (name, number): (Option<String>, Option<i32>),
        layout_config: Option<LayoutPart>,
    ) -> WorkspaceId {
        if let Some(workspace) = self.workspaces.iter_mut().find(|workspace| {
            workspace.id() != moving && !workspace.has_sway_identity() && !workspace.has_windows()
        }) {
            workspace.set_sway_identity(name, number);
            workspace.id()
        } else {
            self.add_sway_workspace_at(1, name, number, layout_config)
        }
    }
}

impl<W: LayoutElement> Monitor<W> {
    /// Moves the floating tree holding `window` (or the active window) from
    /// the workspace at `source_idx` to the one at `target_idx`.
    ///
    /// Returns `false`, without moving anything, when that window is not in a
    /// floating tree. Returns `true` when it is, including when the tree
    /// vanished before it could be moved.
    pub(super) fn move_floating_tree_to_workspace(
        &mut self,
        window: Option<&W::Id>,
        source_idx: usize,
        target_idx: usize,
    ) -> bool {
        let source_id = self.workspaces[source_idx].id();
        let tree_root = window
            .and_then(|window| self.workspaces[source_idx].floating_tree_root_for_window(window))
            .or_else(|| {
                window
                    .is_none()
                    .then(|| {
                        self.workspaces[source_idx]
                            .active_window()
                            .and_then(|window| {
                                self.workspaces[source_idx]
                                    .floating_tree_root_for_window(window.id())
                            })
                    })
                    .flatten()
            });
        let Some(root) = tree_root else {
            return false;
        };
        let Some(removed) = self.workspaces[source_idx].remove_floating_tree(root) else {
            warn!("move_to_workspace: floating tree root reported for the window is gone");
            return true;
        };
        self.workspaces[target_idx].add_floating_tree(removed, false);
        if self.workspace_switch.is_none() {
            self.consider_destroy_workspace(source_id);
        }
        true
    }

    /// Animates the tile of `window`, now on the workspace at `target_idx`,
    /// from `old_render_pos` and marks it as moving between workspaces.
    pub(super) fn animate_moved_tile(
        &mut self,
        target_idx: usize,
        window: &W::Id,
        old_render_pos: Point<f64, Logical>,
        config: swayward_config::Animation,
    ) {
        if let Some((tile, new_render_pos)) = self.workspaces[target_idx]
            .tiles_with_render_positions_mut(false)
            .find(|(tile, _)| tile.window().id() == window)
        {
            tile.animate_move_from_with_config(old_render_pos - new_render_pos, config);
            tile.set_anim_y_between_workspaces();
        }
    }
}

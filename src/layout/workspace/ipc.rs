//! Workspace state as sway's IPC reports it.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn window_border(
        &self,
        window: &W::Id,
    ) -> Option<(swayward_ipc::command::BorderStyle, u16)> {
        self.tiles()
            .find(|tile| tile.window().id() == window)
            .map(|tile| tile.sway_border())
    }

    pub fn set_window_border(
        &mut self,
        window: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> Result<(), &'static str> {
        let changed = if self.floating.has_window(window) {
            self.floating.set_window_border(window, style, width)
                || self.floating.set_tree_window_border(window, style, width)
        } else {
            self.tiling.set_window_border(window, style, width)
        };
        changed
            .then_some(())
            .ok_or("This window doesn't support client side decorations")
    }

    pub fn is_floating_for_ipc(&self, id: &W::Id) -> bool {
        self.floating.has_window(id)
            || self.tiling.tiles().any(|tile| {
                tile.window().id() == id
                    && tile.restore_to_floating
                    && tile.window().pending_sizing_mode().is_fullscreen()
            })
    }

    pub fn ipc_tiling_tree(&self) -> crate::layout::tiling_tree::IpcNode<W::Id> {
        let mut tree = self.tiling.ipc_tree();
        tree.retain_leaves(&|window| !self.is_floating_for_ipc(window));
        tree
    }

    pub fn ipc_floating_trees(
        &self,
    ) -> impl Iterator<Item = (NodeId, crate::layout::tiling_tree::IpcNode<W::Id>, bool)> + '_ {
        self.floating.ipc_trees()
    }
}

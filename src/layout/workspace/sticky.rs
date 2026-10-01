//! Sticky floating windows and trees, which follow the visible workspace.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn has_non_sticky_windows(&self) -> bool {
        !self.tiling.is_empty()
            || self.floating.tiles().any(|tile| !tile.is_sticky)
            || self
                .floating
                .tree_roots()
                .any(|root| !self.floating.tree_is_sticky(root))
    }

    pub fn is_window_sticky(&self, window: &W::Id) -> bool {
        self.floating.window_is_sticky(window)
            || self
                .tiles()
                .find(|tile| tile.window().id() == window)
                .is_some_and(|tile| tile.is_sticky)
    }

    /// Sets sticky on a split container that is not a floating root; sway stores the flag on
    /// every container, and it only takes effect once the container becomes floating.
    pub fn set_split_sticky(&mut self, id: NodeId, value: &str) -> bool {
        let tree = if self.tiling.is_split(id) {
            &mut self.tiling
        } else {
            match self
                .floating
                .tree_root_for_node(id)
                .filter(|root| *root != id)
            {
                Some(root) => match self.floating.tree_mut(root) {
                    Some(tree) => tree,
                    None => return false,
                },
                None => return false,
            }
        };
        let sticky = swayward_ipc::command::parse_boolean(value, tree.is_split_sticky(id));
        tree.set_split_sticky(id, sticky)
    }

    pub fn set_window_sticky(&mut self, window: &W::Id, sticky: bool) -> bool {
        if self.floating.tree_root_for_window(window).is_some() {
            return self.floating.set_window_sticky(window, sticky);
        }
        let Some(tile) = self.tiles_mut().find(|tile| tile.window().id() == window) else {
            return false;
        };
        tile.is_sticky = sticky;
        true
    }

    pub fn set_floating_tree_sticky(&mut self, root: NodeId, sticky: bool) -> bool {
        self.floating.set_tree_sticky(root, sticky)
    }

    pub fn take_sticky_trees(&mut self) -> Vec<RemovedFloatingTree<W>> {
        let removed = self.floating.take_sticky_trees();
        if self.floating.is_empty() {
            self.floating_is_active = FloatingActive::No;
        }
        removed
    }

    pub fn take_sticky_tiles(&mut self) -> Vec<RemovedTile<W>> {
        let ids = self
            .floating
            .tiles()
            .filter(|tile| {
                tile.is_sticky && self.floating.window_is_floating_root(tile.window().id())
            })
            .map(|tile| tile.window().id().clone())
            .collect::<Vec<_>>();
        ids.iter()
            .map(|id| self.remove_tile(id, Transaction::new()))
            .collect()
    }
}

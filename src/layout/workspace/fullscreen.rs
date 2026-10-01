//! Workspace and global fullscreen across the tiling tree and floating trees.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn fullscreen_mode(&self) -> Option<crate::layout::tiling_tree::FullscreenMode> {
        self.tiling
            .fullscreen_node()
            .and_then(|id| self.tiling.fullscreen_mode(id))
            .or_else(|| self.floating.fullscreen_mode())
    }

    pub fn fullscreen_mode_for_window(
        &self,
        window: &W::Id,
    ) -> Option<crate::layout::tiling_tree::FullscreenMode> {
        self.tiling
            .node_for_window(window)
            .and_then(|node| {
                let fullscreen = self.tiling.fullscreen_node()?;
                self.tiling
                    .contains_node(fullscreen, node)
                    .then(|| self.tiling.fullscreen_mode(fullscreen))
                    .flatten()
            })
            .or_else(|| self.floating.fullscreen_mode_for_window(window))
    }

    pub fn fullscreen_contains_window(&self, window: &W::Id) -> bool {
        self.tiling.fullscreen_contains_window(window)
            || self.floating.fullscreen_contains_window(window)
    }

    pub fn fullscreen_window(&self) -> Option<&W::Id> {
        self.tiling
            .fullscreen_window()
            .or_else(|| self.floating.fullscreen_window())
    }

    pub fn set_window_fullscreen(
        &mut self,
        window: &W::Id,
        mode: Option<crate::layout::tiling_tree::FullscreenMode>,
    ) -> bool {
        if mode.is_some() {
            self.disable_fullscreen();
        }
        if self.floating.tree_root_for_window(window).is_some() {
            return self.floating.set_window_fullscreen(window, mode);
        }
        self.set_fullscreen(window, mode.is_some());
        let Some(id) = self.tiling.node_for_window(window) else {
            return false;
        };
        self.tiling.set_node_fullscreen(id, mode)
    }

    pub fn disable_fullscreen(&mut self) {
        if let Some(fullscreen) = self.tiling.fullscreen_node() {
            self.tiling.set_node_fullscreen(fullscreen, None);
        } else {
            self.floating.disable_fullscreen();
        }
    }

    pub fn set_focused_fullscreen(
        &mut self,
        mode: Option<crate::layout::tiling_tree::FullscreenMode>,
    ) -> bool {
        if self.floating_is_active.get() {
            let Some(window) = self.active_window().map(LayoutElement::id).cloned() else {
                return false;
            };
            if self.floating.tree_root_for_window(&window).is_some() {
                return self.floating.set_focused_fullscreen(mode);
            }
            self.set_fullscreen(&window, mode.is_some());
            return true;
        }
        let Some(id) = self.tiling.focus() else {
            return false;
        };
        self.tiling.set_node_fullscreen(id, mode)
    }

    pub fn active_floating_is_fullscreen(&self) -> bool {
        self.tiling.active_tile().is_some_and(|tile| {
            tile.restore_to_floating && tile.window().pending_sizing_mode().is_fullscreen()
        })
    }
}

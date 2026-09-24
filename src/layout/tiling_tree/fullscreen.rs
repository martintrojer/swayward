use super::*;

impl<W: LayoutElement> TilingTree<W> {
    pub fn set_fullscreen(&mut self, window: &W::Id, fullscreen: bool) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        self.set_node_fullscreen(id, fullscreen.then_some(FullscreenMode::Workspace))
    }

    pub fn set_node_fullscreen(&mut self, id: NodeId, fullscreen: Option<FullscreenMode>) -> bool {
        if !self.replace_fullscreen_state(id, fullscreen) {
            return false;
        }
        self.request_window_sizes_with(Some(Transaction::new()), true);
        true
    }

    pub(super) fn replace_fullscreen_state(
        &mut self,
        id: NodeId,
        fullscreen: Option<FullscreenMode>,
    ) -> bool {
        if !self.nodes.contains_key(&id) {
            return false;
        }
        let current = self.fullscreen_node();
        if fullscreen.is_none() && current != Some(id)
            || current == Some(id)
                && self.pending_modes.get(&id).and_then(|mode| mode.fullscreen) == fullscreen
        {
            return false;
        }
        if let Some(current) = current {
            if let Some(mode) = self.pending_modes.get_mut(&current) {
                mode.fullscreen = None;
            }
        }
        if let Some(fullscreen) = fullscreen {
            self.pending_modes
                .entry(id)
                .or_insert(PendingMode {
                    fullscreen: None,
                    maximized: false,
                })
                .fullscreen = Some(fullscreen);
            self.set_focus_id(self.focused_leaf_in(id));
        }
        self.cancel_resize_for(id);
        true
    }

    pub fn fullscreen_node(&self) -> Option<NodeId> {
        self.pending_modes
            .iter()
            .find_map(|(id, mode)| mode.fullscreen.map(|_| *id))
    }

    pub fn fullscreen_mode(&self, id: NodeId) -> Option<FullscreenMode> {
        self.pending_modes.get(&id).and_then(|mode| mode.fullscreen)
    }

    pub fn fullscreen_contains_window(&self, window: &W::Id) -> bool {
        self.fullscreen_node()
            .zip(self.node_for_window(window))
            .is_some_and(|(fullscreen, node)| self.contains_node(fullscreen, node))
    }

    pub fn fullscreen_window(&self) -> Option<&W::Id> {
        let fullscreen = self.fullscreen_node()?;
        let leaf = self.focused_leaf_in(fullscreen)?;
        self.tile(leaf).map(|tile| tile.window().id())
    }

    pub fn set_maximized(&mut self, window: &W::Id, maximized: bool) -> bool {
        let Some(id) = self.node_for_window(window) else {
            return false;
        };
        let mode = self.pending_modes.entry(id).or_insert(PendingMode {
            fullscreen: None,
            maximized: false,
        });
        if mode.maximized == maximized {
            return false;
        }
        mode.maximized = maximized;
        self.cancel_resize_for(id);
        self.request_window_sizes_with(Some(Transaction::new()), true);
        true
    }

    pub fn is_active_pending_fullscreen(&self) -> bool {
        self.focus
            .and_then(|focus| self.fullscreen_node().map(|fullscreen| (focus, fullscreen)))
            .is_some_and(|(focus, fullscreen)| self.contains_node(fullscreen, focus))
    }

    pub fn is_pending_fullscreen(&self, window: &W::Id) -> bool {
        self.node_for_window(window)
            .and_then(|id| self.pending_modes.get(&id))
            .is_some_and(|mode| mode.fullscreen.is_some())
    }

    pub fn is_pending_maximized(&self, window: &W::Id) -> bool {
        self.node_for_window(window)
            .and_then(|id| self.pending_modes.get(&id))
            .is_some_and(|mode| mode.maximized)
    }
}

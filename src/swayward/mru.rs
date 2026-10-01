use super::*;

impl Swayward {
    pub fn close_mru(&mut self, close_request: MruCloseRequest) -> Option<Window> {
        if !self.window_mru_ui.is_open() {
            return None;
        }
        self.queue_redraw_all();

        let id = self.window_mru_ui.close(close_request)?;
        self.find_window_by_id(id)
    }

    pub fn cancel_mru(&mut self) {
        self.close_mru(MruCloseRequest::Cancel);
    }

    pub fn mru_apply_keyboard_commit(&mut self) {
        let Some(pending) = self.pending_mru_commit.take() else {
            return;
        };
        self.event_loop.remove(pending.token);

        if let Some(window) = self
            .layout
            .workspaces_mut()
            .flat_map(|ws| ws.windows_mut())
            .find(|w| w.id() == pending.id)
        {
            window.set_focus_timestamp(pending.stamp);
        }
    }

    pub fn queue_redraw_mru_output(&mut self) {
        if let Some(output) = self.window_mru_ui.output().cloned() {
            self.queue_redraw(&output);
        }
    }
}

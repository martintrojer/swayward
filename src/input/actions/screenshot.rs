use super::*;

impl State {
    pub(super) fn do_screenshot_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::ScreenshotScreen(write_to_disk, show_pointer, path) => {
                let active = self.swayward.layout.active_output().cloned();
                if let Some(active) = active {
                    self.backend.with_primary_renderer(|renderer| {
                        if let Err(err) = self.swayward.screenshot(
                            renderer,
                            &active,
                            write_to_disk,
                            show_pointer,
                            path,
                        ) {
                            warn!("error taking screenshot: {err:?}");
                        }
                    });
                }
            }
            Action::ConfirmScreenshot { write_to_disk } => {
                self.confirm_screenshot(write_to_disk);
            }
            Action::CancelScreenshot => {
                if !self.swayward.screenshot_ui.is_open() {
                    return None;
                }

                self.swayward.screenshot_ui.close();
                self.swayward
                    .cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
                self.swayward.queue_redraw_all();
            }
            Action::ScreenshotTogglePointer => {
                self.swayward.screenshot_ui.toggle_pointer();
                self.swayward.queue_redraw_all();
            }
            Action::Screenshot(show_cursor, path) => {
                self.open_screenshot_ui(show_cursor, path);
                self.swayward.cancel_mru();
            }
            Action::ScreenshotWindow(write_to_disk, show_pointer, path) => {
                let focus = self.swayward.layout.focus_with_output();
                if let Some((mapped, output)) = focus {
                    self.backend.with_primary_renderer(|renderer| {
                        if let Err(err) = self.swayward.screenshot_window(
                            renderer,
                            output,
                            mapped,
                            write_to_disk,
                            show_pointer,
                            path,
                        ) {
                            warn!("error taking screenshot: {err:?}");
                        }
                    });
                }
            }
            action => return Some(action),
        }
        None
    }
}

use super::*;

impl Swayward {
    pub fn handle_focus_follows_mouse(&mut self, new_focus: &PointContents) {
        let Some(ffm) = self.config.borrow().input.focus_follows_mouse else {
            return;
        };

        let pointer = &self.seat.get_pointer().unwrap();
        if pointer.is_grabbed() {
            return;
        }

        if self.window_mru_ui.is_open() {
            return;
        }

        // Recompute the current pointer focus because we don't update it during animations.
        let current_focus = self.contents_under(pointer.current_location());

        if let Some(output) = &new_focus.output {
            if current_focus.output.as_ref() != Some(output) {
                self.layout.focus_output(output);
            }
        }

        if let Some((window, hit)) = &new_focus.window {
            let tab_target = matches!(
                hit,
                HitType::Activate {
                    is_tab_indicator: true
                }
            )
            .then(|| self.layout.tab_indicator_focus_target(window))
            .flatten()
            .map(|mapped| mapped.window.clone());
            let window = tab_target.as_ref().unwrap_or(window);
            let current_window = current_focus.window.as_ref().map(|(window, _)| window);

            // Sway's `yes` focuses only when the hovered window changed, so
            // that a workspace switch sliding a window under a still pointer
            // does not steal focus. `always` drops that guard and re-focuses
            // the hovered window even then
            // (`sway/sway/input/seatop_default.c:590-598`).
            let hover_changed = current_window != Some(window)
                || ffm.mode == swayward_config::input::FocusFollowsMouseMode::Always;

            if !self.layout.is_overview_open() && hover_changed {
                if !self.layout.should_trigger_focus_follows_mouse_on(window) {
                    return;
                }

                if let Some(threshold) = ffm.max_scroll_amount {
                    if self.layout.scroll_amount_to_activate(window) > threshold.0 {
                        return;
                    }
                }

                self.layout.activate_window_without_raising(window);
                self.layer_shell_on_demand_focus = None;
            }
        }

        if let Some(layer) = &new_focus.layer {
            if current_focus.layer.as_ref() != Some(layer) {
                self.layer_shell_on_demand_focus = Some(layer.clone());
            }
        }
    }
}

use super::*;

impl State {
    pub(super) fn do_overview_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::ToggleOverview => {
                // A layer surface holding on-demand focus outranks the
                // overview (Swayward::compute_focus checks Layer::Top first),
                // so clicking a bar and then opening the overview left every
                // key going to the bar and none of the overview binds firing.
                self.swayward.layer_shell_on_demand_focus = None;
                self.swayward.layout.toggle_overview();
                self.swayward.queue_redraw_all();
            }
            Action::OpenOverview => {
                if self.swayward.layout.open_overview() {
                    self.swayward.layer_shell_on_demand_focus = None;
                    self.swayward.queue_redraw_all();
                }
            }
            Action::CloseOverview => {
                if self.swayward.layout.close_overview() {
                    self.swayward.queue_redraw_all();
                }
            }
            action => return Some(action),
        }
        None
    }
}

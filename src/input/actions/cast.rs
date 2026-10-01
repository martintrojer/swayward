use super::*;

impl State {
    pub(super) fn do_cast_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::SetDynamicCastWindow => {
                let id = self
                    .swayward
                    .layout
                    .active_workspace()
                    .and_then(|ws| ws.active_window())
                    .map(|mapped| mapped.id().get());
                if let Some(id) = id {
                    self.set_dynamic_cast_target(CastTarget::Window { id });
                }
            }
            Action::SetDynamicCastMonitor(output) => {
                let output = match output {
                    None => self.swayward.layout.active_output(),
                    Some(name) => self.swayward.output_by_name_match(&name),
                };
                if let Some(output) = output {
                    self.set_dynamic_cast_target(CastTarget::output(output));
                }
            }
            Action::ClearDynamicCastTarget => {
                self.set_dynamic_cast_target(CastTarget::Nothing);
            }
            action => return Some(action),
        }
        None
    }
}

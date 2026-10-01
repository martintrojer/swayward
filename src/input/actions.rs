#[path = "actions/cast.rs"]
mod cast;
#[path = "actions/monitor.rs"]
mod monitor;
#[path = "actions/mru.rs"]
mod mru;
#[path = "actions/overview.rs"]
mod overview;
#[path = "actions/screenshot.rs"]
mod screenshot;
#[path = "actions/session.rs"]
mod session;
#[path = "actions/tree.rs"]
mod tree;
#[path = "actions/window.rs"]
mod window;
#[path = "actions/workspace.rs"]
mod workspace;

use super::*;

impl State {
    pub fn handle_bind(&mut self, bind: Bind) {
        if self.swayward.is_locked()
            && !(bind.allow_when_locked || allowed_when_locked(&bind.action))
        {
            return;
        }

        if let Some(cooldown) = bind.cooldown {
            let cooldown_key = bind.cooldown_identity();
            match self
                .swayward
                .bind_cooldown_timers
                .entry(cooldown_key.clone())
            {
                Entry::Occupied(_) => return,
                Entry::Vacant(entry) => {
                    let timer = Timer::from_duration(cooldown);
                    let token = self
                        .swayward
                        .event_loop
                        .insert_source(timer, move |_, _, state| {
                            if state
                                .swayward
                                .bind_cooldown_timers
                                .remove(&cooldown_key)
                                .is_none()
                            {
                                error!("bind cooldown timer entry disappeared");
                            }
                            TimeoutAction::Drop
                        })
                        .unwrap();
                    entry.insert(token);
                }
            }
        }

        let event = sway_binding_event(&bind, self.backend.mod_key(&self.swayward.config.borrow()));
        let succeeded = match bind.action {
            Action::SwayCommand(command) => crate::command::execute(self, &command)
                .into_iter()
                .all(|outcome| outcome.success),
            action => {
                self.do_action(action, bind.allow_when_locked);
                false
            }
        };
        if succeeded {
            if let (Some(server), Some(event)) = (&self.swayward.ipc_server, event) {
                server.send_event(event);
            }
        }
    }

    pub(super) fn focused_view_id(&self) -> Option<i64> {
        let workspace = self.swayward.layout.active_workspace()?;
        if workspace
            .focused_container_node()
            .is_some_and(|node| workspace.is_tiling_split(node))
        {
            return None;
        }
        self.swayward
            .layout
            .focus()
            .map(|window| crate::ipc::tree::window_id(window.id()))
    }

    pub(super) fn emit_window_move(&mut self, moved: bool, id: Option<i64>) {
        if !moved {
            return;
        }
        self.ipc_refresh_layout();
        if let (Some(server), Some(id)) = (&self.swayward.ipc_server, id) {
            server.send_event(swayward_ipc::legacy::Event::WindowMoved { id });
        }
    }

    pub fn do_action(&mut self, action: Action, allow_when_locked: bool) {
        if self.swayward.is_locked() && !(allow_when_locked || allowed_when_locked(&action)) {
            return;
        }

        if let Some(touch) = self.swayward.seat.get_touch() {
            touch.cancel(self);
        }

        let action = self.do_session_action(action);
        let action = action.and_then(|action| self.do_screenshot_action(action));
        let action = action.and_then(|action| self.do_window_action(action));
        let action = action.and_then(|action| self.do_tree_action(action));
        let action = action.and_then(|action| self.do_workspace_action(action));
        let action = action.and_then(|action| self.do_monitor_action(action));
        let action = action.and_then(|action| self.do_cast_action(action));
        let action = action.and_then(|action| self.do_overview_action(action));
        let action = action.and_then(|action| self.do_mru_action(action));
        if let Some(action) = action {
            error!(?action, "unhandled action");
        }
    }
}

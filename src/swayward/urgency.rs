use super::*;

impl Swayward {
    pub(super) fn focus_clears_urgency(&mut self, surface: &WlSurface, changed_workspace: bool) {
        let Some((mapped, _)) = self.layout.find_window_and_output_mut(surface) else {
            return;
        };
        if !mapped.is_urgent() || self.urgency_timers.contains_key(&mapped.id()) {
            return;
        }

        let id = mapped.id();
        let timeout_ms = self.config.borrow().urgent_timeout_ms.0;
        if !changed_workspace || timeout_ms == 0 {
            mapped.set_urgent(false);
            return;
        }

        let token = self
            .event_loop
            .insert_source(
                Timer::from_duration(Duration::from_millis(u64::from(timeout_ms))),
                move |_, _, state| {
                    state.swayward.urgency_timers.remove(&id);
                    state.swayward.clear_window_urgency(id);
                    TimeoutAction::Drop
                },
            )
            .unwrap();
        self.urgency_timers.insert(id, token);
    }

    fn clear_window_urgency(&mut self, id: MappedId) {
        self.layout.with_windows_mut(|mapped, _| {
            if mapped.id() == id {
                mapped.set_urgent(false);
            }
        });
        self.queue_redraw_all();
    }

    #[cfg(test)]
    pub fn fire_urgency_timer_for_test(&mut self, id: MappedId) -> bool {
        let Some(token) = self.urgency_timers.remove(&id) else {
            return false;
        };
        self.event_loop.remove(token);
        self.clear_window_urgency(id);
        true
    }

    pub fn cancel_urgency_timer(&mut self, id: MappedId) {
        if let Some(token) = self.urgency_timers.remove(&id) {
            self.event_loop.remove(token);
        }
    }

    pub fn set_window_urgent(&mut self, id: MappedId, urgent: bool) {
        if !urgent {
            self.cancel_urgency_timer(id);
        }
        self.layout.with_windows_mut(|mapped, _| {
            if mapped.id() == id {
                mapped.set_urgent(urgent);
            }
        });
    }
}

use super::*;

pub(super) fn synthetic_bind(trigger: Trigger, action: Action, cooldown: Option<Duration>) -> Bind {
    Bind {
        key: Key {
            trigger,
            modifiers: Modifiers::empty(),
        },
        action,
        mouse_regions: MouseRegions::empty(),
        input_device: "*".into(),
        group: None,
        release: false,
        repeat: true,
        cooldown,
        allow_when_locked: false,
        allow_inhibiting: false,
        hotkey_overlay_title: None,
    }
}

impl State {
    pub(super) fn resolve_axis_binds(
        &mut self,
        triggers: (Trigger, Trigger),
        mods: ModifiersState,
        modifiers: Modifiers,
        mod_key: ModKey,
        input_device: &str,
        check_region: bool,
    ) -> (Option<Bind>, Option<Bind>) {
        let (negative, positive) = triggers;
        let config = self.swayward.config.borrow();
        let bindings = make_binds_iter(
            &config,
            &self.swayward.binding_mode,
            &mut self.swayward.window_mru_ui,
            modifiers,
        );
        let negative = find_configured_bind_for_device(
            bindings.clone(),
            mod_key,
            negative,
            mods,
            input_device,
        );
        let positive =
            find_configured_bind_for_device(bindings, mod_key, positive, mods, input_device);
        drop(config);
        let filter = |bind: &Bind| {
            (!check_region || self.mouse_bind_matches_region(bind))
                && (!self.swayward.screenshot_ui.is_open()
                    || allowed_during_screenshot(&bind.action))
        };
        (negative.filter(&filter), positive.filter(filter))
    }

    pub(super) fn fire_axis_ticks(
        &mut self,
        ticks: i8,
        negative: Option<Bind>,
        positive: Option<Bind>,
    ) -> bool {
        let mut handled = false;
        if let Some(positive) = positive {
            for _ in 0..ticks {
                self.handle_bind(positive.clone());
                handled = true;
            }
        }
        if let Some(negative) = negative {
            for _ in ticks..0 {
                self.handle_bind(negative.clone());
                handled = true;
            }
        }
        handled
    }
}

use swayward_config::Action;

use crate::swayward::State;

pub(super) fn mutate_switch_binding(
    state: &mut State,
    mode: &str,
    combo: &str,
    command: Option<String>,
    locked: bool,
) -> Result<(), String> {
    let Some((switch, trigger)) = combo.split_once(':') else {
        return Err(
            "Invalid unbindswitch command (expected binding with the form <switch>:<state>)".into(),
        );
    };
    let switch = match switch {
        "lid" => smithay::backend::input::Switch::Lid,
        "tablet" => smithay::backend::input::Switch::TabletMode,
        _ => unreachable!("parser validated switch"),
    };
    let trigger = match trigger {
        "on" => Some(smithay::backend::input::SwitchState::On),
        "off" => Some(smithay::backend::input::SwitchState::Off),
        "toggle" => None,
        _ => unreachable!("parser validated switch state"),
    };
    let mode = mode.to_owned();
    if mode == "default" {
        let config = state.swayward.config.borrow();
        let file_binding_exists = match (switch, trigger) {
            (
                smithay::backend::input::Switch::Lid,
                Some(smithay::backend::input::SwitchState::On),
            ) => config.switch_events.lid_close.is_some(),
            (
                smithay::backend::input::Switch::Lid,
                Some(smithay::backend::input::SwitchState::Off),
            ) => config.switch_events.lid_open.is_some(),
            (
                smithay::backend::input::Switch::TabletMode,
                Some(smithay::backend::input::SwitchState::On),
            ) => config.switch_events.tablet_mode_on.is_some(),
            (
                smithay::backend::input::Switch::TabletMode,
                Some(smithay::backend::input::SwitchState::Off),
            ) => config.switch_events.tablet_mode_off.is_some(),
            _ => false,
        };
        if file_binding_exists {
            return Err(
                "runtime switch binding conflicts with a narrower KDL switch-event binding".into(),
            );
        }
    }
    let bindings = &mut state.swayward.runtime_switch_bindings;
    let existing = bindings.iter().position(|binding| {
        binding.mode == mode
            && binding.switch == switch
            && binding.state == trigger
            && binding.locked == locked
    });
    if let Some(command) = command {
        let binding = crate::swayward::RuntimeSwitchBinding {
            mode,
            switch,
            state: trigger,
            locked,
            command,
        };
        if let Some(index) = existing {
            bindings[index] = binding;
        } else {
            bindings.push(binding);
        }
    } else if let Some(index) = existing {
        bindings.remove(index);
    } else {
        return Err(format!("Could not find switch binding `{combo}`"));
    }
    Ok(())
}

pub(super) struct BindingMutation<'a> {
    pub(super) mode: &'a str,
    pub(super) key: &'a str,
    pub(super) command: Option<String>,
    pub(super) keycode: bool,
    pub(super) release: bool,
    pub(super) locked: bool,
    pub(super) inhibited: bool,
    pub(super) no_repeat: bool,
    pub(super) input_device: String,
}

pub(super) enum BindingMutationError {
    Parse(String),
    Command(String),
}

pub(super) fn mutate_key_binding(
    state: &mut State,
    mutation: BindingMutation<'_>,
) -> Result<(), BindingMutationError> {
    let BindingMutation {
        mode,
        key,
        command,
        keycode,
        release,
        locked,
        inhibited,
        no_repeat,
        input_device,
    } = mutation;
    let keycombo = key.to_owned();
    let key = if keycode {
        let (modifiers, code) = key
            .rsplit_once('+')
            .map_or(("", key), |(mods, code)| (mods, code));
        let code: u32 = match code.parse() {
            Ok(code) if (8..=255).contains(&code) => code,
            _ if command.is_none() => {
                return Err(BindingMutationError::Command(format!(
                    "Could not find binding `{keycombo}` for the given flags"
                )));
            }
            _ => {
                return Err(BindingMutationError::Command(format!(
                    "Invalid keycode '{code}'"
                )))
            }
        };
        if modifiers.is_empty() {
            format!("code:{code}")
        } else {
            format!("{modifiers}+code:{code}")
        }
    } else {
        key.to_owned()
    };
    let key = key
        .parse::<swayward_config::Key>()
        .map_err(|_| BindingMutationError::Parse(format!("Unknown key or button '{keycombo}'")))?;
    if !matches!(
        key.trigger,
        swayward_config::Trigger::Keysym(_) | swayward_config::Trigger::Keycode(_)
    ) {
        return Err(BindingMutationError::Command(
            "runtime mouse bindings require exact pointer-region semantics".into(),
        ));
    }
    let identity_matches = |bind: &swayward_config::Bind| {
        bind.key == key
            && bind.input_device == input_device
            && bind.release == release
            && bind.allow_when_locked == locked
            && bind.allow_inhibiting != inhibited
            && bind.group.is_none()
            && bind.mouse_regions.is_empty()
    };

    let binding_mode = mode;
    let mut config = state.swayward.config.borrow_mut();
    let binds = if binding_mode == "default" {
        &mut config.binds.0
    } else {
        &mut config
            .binding_modes
            .iter_mut()
            .find(|mode| mode.name == binding_mode)
            .ok_or_else(|| {
                BindingMutationError::Command(format!("Unknown binding mode '{binding_mode}'"))
            })?
            .binds
            .0
    };
    let existing = binds.iter().position(identity_matches);
    if let Some(command) = command {
        let bind = swayward_config::Bind {
            key,
            action: Action::SwayCommand(command),
            mouse_regions: swayward_config::MouseRegions::empty(),
            input_device,
            group: None,
            release,
            repeat: !release && !no_repeat,
            cooldown: None,
            allow_when_locked: locked,
            allow_inhibiting: !inhibited,
            hotkey_overlay_title: None,
        };
        if let Some(index) = existing {
            // Sway overwrites an equal binding in place (`binding_upsert`,
            // sway/sway/commands/bind.c:260-278).
            binds[index] = bind;
        } else {
            binds.push(bind);
        }
    } else if let Some(index) = existing {
        binds.remove(index);
    } else {
        return Err(BindingMutationError::Command(format!(
            "Could not find binding `{keycombo}` for the given flags"
        )));
    }
    drop(config);
    refresh_binding_caches(state);
    Ok(())
}

fn refresh_binding_caches(state: &mut State) {
    let config = state.swayward.config.borrow();
    let mod_key = state.backend.mod_key(&config);
    state
        .swayward
        .hotkey_overlay
        .on_hotkey_config_updated(mod_key);
    state.swayward.mods_with_mouse_binds =
        crate::input::mods_with_mouse_binds(mod_key, &config.binds);
    state.swayward.mods_with_wheel_binds =
        crate::input::mods_with_wheel_binds(mod_key, &config.binds);
    state.swayward.mods_with_tablet_stylus_binds =
        crate::input::mods_with_tablet_stylus_binds(mod_key, &config.binds);
    state.swayward.mods_with_finger_scroll_binds =
        crate::input::mods_with_finger_scroll_binds(mod_key, &config.binds);
}

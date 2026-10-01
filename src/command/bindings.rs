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
    // The parser already rejected any other switch or state.
    let switch = match switch {
        "lid" => smithay::backend::input::Switch::Lid,
        "tablet" => smithay::backend::input::Switch::TabletMode,
        _ => return Err(format!("unknown switch {switch}")),
    };
    let trigger = match trigger {
        "on" => Some(smithay::backend::input::SwitchState::On),
        "off" => Some(smithay::backend::input::SwitchState::Off),
        "toggle" => None,
        _ => return Err(format!("unknown state {trigger}")),
    };
    let mode = mode.to_owned();
    if mode == "default"
        && kdl_switch_binding_exists(&state.swayward.config.borrow(), switch, trigger)
    {
        return Err(
            "runtime switch binding conflicts with a narrower KDL switch-event binding".into(),
        );
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

/// Whether the KDL config binds this exact switch transition. A `toggle`
/// binding has no KDL counterpart.
fn kdl_switch_binding_exists(
    config: &swayward_config::Config,
    switch: smithay::backend::input::Switch,
    trigger: Option<smithay::backend::input::SwitchState>,
) -> bool {
    use smithay::backend::input::{Switch, SwitchState};
    let events = &config.switch_events;
    match (switch, trigger) {
        (Switch::Lid, Some(SwitchState::On)) => events.lid_close.is_some(),
        (Switch::Lid, Some(SwitchState::Off)) => events.lid_open.is_some(),
        (Switch::TabletMode, Some(SwitchState::On)) => events.tablet_mode_on.is_some(),
        (Switch::TabletMode, Some(SwitchState::Off)) => events.tablet_mode_off.is_some(),
        _ => false,
    }
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
    let key = normalise_key(key, keycode, command.is_some())?;
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

    let mut config = state.swayward.config.borrow_mut();
    let binds = binds_for_mode(&mut config, mode)?;
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

/// Parse a bindsym or bindcode combo into a config key. A bindcode combo's
/// last component becomes `code:<n>`; sway accepts codes 8 to 255.
fn normalise_key(
    key: &str,
    keycode: bool,
    has_command: bool,
) -> Result<swayward_config::Key, BindingMutationError> {
    let keycombo = key;
    let key = if keycode {
        let (modifiers, code) = key
            .rsplit_once('+')
            .map_or(("", key), |(mods, code)| (mods, code));
        let code: u32 = match code.parse() {
            Ok(code) if (8..=255).contains(&code) => code,
            _ if !has_command => {
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
    key.parse::<swayward_config::Key>()
        .map_err(|_| BindingMutationError::Parse(format!("Unknown key or button '{keycombo}'")))
}

/// The binding list of `mode`, where "default" is the top-level list.
fn binds_for_mode<'a>(
    config: &'a mut swayward_config::Config,
    mode: &str,
) -> Result<&'a mut Vec<swayward_config::Bind>, BindingMutationError> {
    if mode == "default" {
        return Ok(&mut config.binds.0);
    }
    config
        .binding_modes
        .iter_mut()
        .find(|binding_mode| binding_mode.name == mode)
        .map(|binding_mode| &mut binding_mode.binds.0)
        .ok_or_else(|| BindingMutationError::Command(format!("Unknown binding mode '{mode}'")))
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

impl BindingMutationError {
    fn into_outcome(self) -> swayward_ipc::CommandOutcome {
        match self {
            Self::Parse(error) => swayward_ipc::command::parse_error(error),
            Self::Command(error) => super::failure(error),
        }
    }
}

pub(super) struct BindingCommand {
    pub(super) key: String,
    pub(super) command: Option<String>,
    pub(super) keycode: bool,
    pub(super) release: bool,
    pub(super) locked: bool,
    pub(super) inhibited: bool,
    pub(super) no_repeat: bool,
    pub(super) input_device: String,
}

fn key_binding(state: &mut State, mode: &str, binding: BindingCommand) -> super::HandlerResult {
    let BindingCommand {
        key,
        command,
        keycode,
        release,
        locked,
        inhibited,
        no_repeat,
        input_device,
    } = binding;
    mutate_key_binding(
        state,
        BindingMutation {
            mode,
            key: &key,
            command,
            keycode,
            release,
            locked,
            inhibited,
            no_repeat,
            input_device,
        },
    )
    .map_err(BindingMutationError::into_outcome)?;
    Ok(None)
}

pub(super) fn mode(
    state: &mut State,
    name: String,
    pango_markup: bool,
    subcommand: Option<Box<super::Command>>,
) -> super::HandlerResult {
    if let Some(subcommand) = subcommand {
        let mut config = state.swayward.config.borrow_mut();
        if name != "default" && !config.binding_modes.iter().any(|mode| mode.name == name) {
            config.binding_modes.push(swayward_config::BindingMode {
                name: name.clone(),
                pango_markup,
                binds: Default::default(),
            });
        }
        drop(config);
        // Sway points `config->current_mode` at the named mode, runs the same
        // handler the top level would, then restores the previous mode
        // (`sway/sway/commands/mode.c:69-84`), so a nested bind never switches
        // modes.
        match *subcommand {
            super::Command::Set { name, value } => set_variable(state, name, value),
            super::Command::Bind {
                key,
                command,
                keycode,
                release,
                locked,
                inhibited,
                no_repeat,
                input_device,
            } => {
                key_binding(
                    state,
                    &name,
                    BindingCommand {
                        key,
                        command,
                        keycode,
                        release,
                        locked,
                        inhibited,
                        no_repeat,
                        input_device,
                    },
                )?;
            }
            super::Command::SwitchBind {
                switch,
                command,
                locked,
            } => {
                mutate_switch_binding(state, &name, &switch, command, locked)
                    .map_err(super::failure)?;
            }
            // The mode parser admits only the subcommands above.
            _ => return Err(super::failure("Unknown/invalid mode subcommand")),
        }
        Ok(None)
    } else {
        let pango_markup = if name == "default" {
            false
        } else {
            let pango_markup = state
                .swayward
                .config
                .borrow()
                .binding_modes
                .iter()
                .find(|mode| mode.name == name)
                .map(|mode| mode.pango_markup);
            let Some(pango_markup) = pango_markup else {
                return Err(super::failure(format!("Unknown mode `{name}'")));
            };
            pango_markup
        };
        state.swayward.binding_mode = name.clone();
        if let Some(server) = &state.swayward.ipc_server {
            server.send_event(swayward_ipc::legacy::Event::BindingModeChanged {
                mode: name,
                pango_markup,
            });
        }
        Ok(None)
    }
}

pub(super) fn set_variable(state: &mut State, name: String, value: String) {
    // Sway replaces an existing value in place and keeps the list sorted
    // longest name first, so a longer name is never shadowed by a shorter
    // prefix of itself (`sway/sway/commands/set.c:36-55`).
    swayward_ipc::command::set_variable(&mut state.swayward.sway_variables, name, value);
}

pub(super) fn bind(state: &mut State, binding: BindingCommand) -> super::HandlerResult {
    let mode = state.swayward.binding_mode.clone();
    key_binding(state, &mode, binding)
}

pub(super) fn switch_bind(
    state: &mut State,
    switch: String,
    command: Option<String>,
    locked: bool,
) -> super::HandlerResult {
    let mode = state.swayward.binding_mode.clone();
    mutate_switch_binding(state, &mode, &switch, command, locked).map_err(super::failure)?;
    Ok(None)
}

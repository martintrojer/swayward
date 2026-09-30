use super::*;

/// One keyboard event as seen through each xkb view that bindings can match against.
#[derive(Clone, Copy)]
pub(super) struct KeyEventContext<'a> {
    pub input_device: &'a str,
    pub key_code: Keycode,
    pub modified: Keysym,
    pub raw: Option<Keysym>,
    pub group: u32,
    pub code_modifiers: ModifiersState,
    pub raw_modifiers: ModifiersState,
    pub translated_modifiers: ModifiersState,
}

/// Session state that decides which bindings may fire.
#[derive(Clone, Copy)]
pub(super) struct BindingPolicy {
    pub mod_key: ModKey,
    pub locked: bool,
    pub inhibited: bool,
    pub disable_power_key_handling: bool,
}

/// The device, layout group and session state one configured-binding lookup matches.
#[derive(Clone, Copy)]
pub(super) struct BindingContext<'a> {
    pub input_device: &'a str,
    pub group: u32,
    pub locked: bool,
    pub inhibited: bool,
}

/// Check whether the key should be intercepted and mark intercepted
/// pressed keys as `suppressed`, thus preventing `releases` corresponding
/// to them from being delivered.
pub(super) fn should_intercept_key<'a>(
    suppressed_keys: &mut HashSet<Keycode>,
    held_release_bind: &mut Option<Bind>,
    bindings: impl IntoIterator<Item = &'a Bind> + Clone,
    event: KeyEventContext<'_>,
    pressed: bool,
    screenshot_ui: &ScreenshotUi,
    policy: BindingPolicy,
) -> FilterResult<Option<Bind>> {
    let KeyEventContext {
        key_code,
        modified,
        raw,
        raw_modifiers,
        ..
    } = event;
    let bindings = bindings.into_iter().collect::<Vec<_>>();
    let release_bind = pressed
        .then(|| {
            find_bind(
                bindings.iter().copied().filter(|bind| bind.release),
                event,
                policy,
            )
        })
        .flatten();
    if held_release_bind.as_ref() != release_bind.as_ref()
        || (!pressed
            && held_release_bind.as_ref().is_some_and(|bind| {
                bind.key.trigger == Trigger::Keycode(key_code.raw())
                    || bind.key.trigger == Trigger::Keysym(modified)
                    || raw.is_some_and(|raw| bind.key.trigger == Trigger::Keysym(raw))
            }))
    {
        if !pressed {
            suppressed_keys.remove(&key_code);
            return FilterResult::Intercept(held_release_bind.take());
        }
        *held_release_bind = None;
    }
    if pressed && release_bind.is_some() {
        *held_release_bind = release_bind;
    }

    let mut final_bind = find_bind(
        bindings.iter().copied().filter(|bind| !bind.release),
        event,
        policy,
    );

    // Allow only a subset of compositor actions while the screenshot UI is open, since the user
    // cannot see the screen.
    if screenshot_ui.is_open() {
        let mut use_screenshot_ui_action = true;

        if let Some(bind) = &final_bind {
            if allowed_during_screenshot(&bind.action) {
                use_screenshot_ui_action = false;
            }
        }

        if use_screenshot_ui_action {
            if let Some(raw) = raw {
                final_bind = screenshot_ui.action(raw, raw_modifiers).map(|action| Bind {
                    key: Key {
                        trigger: Trigger::Keysym(raw),
                        // Not entirely correct but it doesn't matter in how we currently use
                        // it.
                        modifiers: Modifiers::empty(),
                    },
                    action,
                    mouse_regions: MouseRegions::empty(),
                    input_device: "*".into(),
                    group: None,
                    release: false,
                    repeat: true,
                    cooldown: None,
                    allow_when_locked: false,
                    // The screenshot UI owns the focus anyway, so this doesn't really matter.
                    // But logically, nothing can inhibit its actions. Only opening it can be
                    // inhibited.
                    allow_inhibiting: false,
                    hotkey_overlay_title: None,
                });
            }
        }
    }

    match (final_bind, pressed) {
        (Some(bind), true) => {
            if policy.inhibited && bind.allow_inhibiting {
                FilterResult::Forward
            } else {
                suppressed_keys.insert(key_code);
                FilterResult::Intercept(Some(bind))
            }
        }
        (_, false) if suppressed_keys.remove(&key_code) => FilterResult::Intercept(None),
        (_, false) => FilterResult::Forward,
        (None, true) if held_release_bind.is_some() => FilterResult::Intercept(None),
        (None, true) => FilterResult::Forward,
    }
}

pub(super) fn find_bind<'a>(
    bindings: impl IntoIterator<Item = &'a Bind> + Clone,
    event: KeyEventContext<'_>,
    policy: BindingPolicy,
) -> Option<Bind> {
    let KeyEventContext {
        input_device,
        key_code,
        modified,
        raw,
        group,
        code_modifiers,
        raw_modifiers,
        translated_modifiers,
    } = event;
    let BindingPolicy {
        mod_key,
        locked,
        inhibited,
        disable_power_key_handling,
    } = policy;
    let context = BindingContext {
        input_device,
        group,
        locked,
        inhibited,
    };
    use keysyms::*;

    // Handle hardcoded binds.
    let hardcoded_action = match modified.raw() {
        modified if (KEY_XF86Switch_VT_1..=KEY_XF86Switch_VT_12).contains(&modified) => {
            let vt = (modified - KEY_XF86Switch_VT_1 + 1) as i32;
            Some(Action::ChangeVt(vt))
        }
        key if key == KEY_XF86PowerOff && !disable_power_key_handling => Some(Action::Suspend),
        _ => None,
    };

    if let Some(action) = hardcoded_action {
        return Some(Bind {
            key: Key {
                // Not entirely correct but it doesn't matter in how we currently use it.
                trigger: Trigger::Keysym(modified),
                modifiers: Modifiers::empty(),
            },
            action,
            mouse_regions: MouseRegions::empty(),
            input_device: "*".into(),
            group: None,
            release: false,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            // In a worst-case scenario, the user has no way to unlock the compositor and a
            // misbehaving client has a keyboard shortcuts inhibitor, "jailing" the user.
            // The user must always be able to change VTs to recover from such a situation.
            // It also makes no sense to inhibit the default power key handling.
            // Hardcoded binds must never be inhibited.
            allow_inhibiting: false,
            hotkey_overlay_title: None,
        });
    }

    let modified_bind = find_configured_bind_with_context(
        bindings.clone(),
        mod_key,
        &[Trigger::Keysym(modified)],
        translated_modifiers,
        context,
    );
    let numlock_changed_keypad_symbol = translated_modifiers.num_lock
        && raw != Some(modified)
        && (keysyms::KEY_KP_Space..=keysyms::KEY_KP_Equal).contains(&modified.raw());
    if modified_bind.is_some() || numlock_changed_keypad_symbol {
        return modified_bind;
    }

    raw.and_then(|raw| {
        find_configured_bind_with_context(
            bindings.clone(),
            mod_key,
            &[Trigger::Keysym(raw)],
            raw_modifiers,
            context,
        )
    })
    .or_else(|| {
        find_configured_bind_with_context(
            bindings,
            mod_key,
            &[Trigger::Keycode(key_code.raw())],
            code_modifiers,
            context,
        )
    })
}

pub(super) fn mouse_regions_match(
    configured: MouseRegions,
    click_region: MouseRegions,
    on_workspace: bool,
) -> bool {
    click_region.intersects(configured) && (!on_workspace || configured.contains(click_region))
}

#[cfg(test)]
pub(super) fn find_configured_bind<'a>(
    bindings: impl IntoIterator<Item = &'a Bind> + Clone,
    mod_key: ModKey,
    trigger: Trigger,
    mods: ModifiersState,
) -> Option<Bind> {
    find_configured_bind_for_device(bindings, mod_key, trigger, mods, "*")
}

pub(super) fn find_configured_bind_for_device<'a>(
    bindings: impl IntoIterator<Item = &'a Bind> + Clone,
    mod_key: ModKey,
    trigger: Trigger,
    mods: ModifiersState,
    input_device: &str,
) -> Option<Bind> {
    find_configured_bind_with_context(
        bindings,
        mod_key,
        &[trigger],
        mods,
        BindingContext {
            input_device,
            group: 0,
            locked: false,
            inhibited: false,
        },
    )
}

pub(super) fn find_configured_bind_with_context<'a>(
    bindings: impl IntoIterator<Item = &'a Bind> + Clone,
    mod_key: ModKey,
    triggers: &[Trigger],
    mods: ModifiersState,
    context: BindingContext<'_>,
) -> Option<Bind> {
    let BindingContext {
        input_device,
        group,
        locked,
        inhibited,
    } = context;
    let mut modifiers = modifiers_from_state(mods);
    let mod_down = mod_key.is_pressed(modifiers);
    if mod_down {
        modifiers |= Modifiers::COMPOSITOR;
    }

    let mut best = None;
    let mut best_rank = None;
    let mut lock_fallback = None;
    let mut lock_fallback_rank = None;
    for trigger in triggers {
        for bind in bindings.clone() {
            let bind_locked = bind.allow_when_locked || allowed_when_locked(&bind.action);
            if bind.key.trigger != *trigger
                || bind
                    .group
                    .is_some_and(|bind_group| u32::from(bind_group) != group)
                || (locked && !bind_locked)
                || (inhibited && bind.allow_inhibiting)
            {
                continue;
            }

            let mut bind_modifiers = bind.key.modifiers;
            if bind_modifiers.contains(Modifiers::COMPOSITOR) {
                bind_modifiers |= mod_key.to_modifiers();
            } else if mod_key != ModKey::None && bind_modifiers.contains(mod_key.to_modifiers()) {
                bind_modifiers |= Modifiers::COMPOSITOR;
            }

            let exact_input = bind.input_device == input_device;
            if !exact_input && bind.input_device != "*" {
                continue;
            }
            let rank = (
                exact_input,
                bind.group.is_some(),
                bind_locked == locked,
                (!bind.allow_inhibiting) == inhibited,
            );
            if bind_modifiers == modifiers {
                if best_rank.is_none_or(|current| rank > current) {
                    best = Some(bind.clone());
                    best_rank = Some(rank);
                } else if best_rank == Some(rank) && best.as_ref() != Some(bind) {
                    debug!("encountered conflicting bindings");
                }
                continue;
            }

            // i3 adds Caps Lock and Num Lock variants for bindings that do not name those
            // modifiers, while keeping an explicit lock-qualified binding as the exact match.
            let locks = Modifiers::CAPS | Modifiers::NUM;
            if !bind_modifiers.intersects(locks)
                && bind_modifiers == modifiers.difference(locks)
                && lock_fallback_rank.is_none_or(|current| rank > current)
            {
                lock_fallback = Some(bind.clone());
                lock_fallback_rank = Some(rank);
            }
        }
    }
    best.or(lock_fallback)
}

pub(super) fn find_configured_switch_action(
    bindings: &SwitchBinds,
    switch: Switch,
    state: SwitchState,
) -> Option<Action> {
    let switch_action = match (switch, state) {
        (Switch::Lid, SwitchState::Off) => &bindings.lid_open,
        (Switch::Lid, SwitchState::On) => &bindings.lid_close,
        (Switch::TabletMode, SwitchState::Off) => &bindings.tablet_mode_off,
        (Switch::TabletMode, SwitchState::On) => &bindings.tablet_mode_on,
        _ => unreachable!(),
    };
    switch_action
        .as_ref()
        .map(|switch_action| Action::Spawn(switch_action.spawn.clone()))
}

pub(super) fn sway_binding_event(
    bind: &Bind,
    mod_key: ModKey,
) -> Option<swayward_ipc::legacy::Event> {
    let Action::SwayCommand(command) = &bind.action else {
        return None;
    };
    let mut modifiers = bind.key.modifiers;
    if modifiers.contains(Modifiers::COMPOSITOR) {
        modifiers.remove(Modifiers::COMPOSITOR);
        modifiers.insert(mod_key.to_modifiers());
    }
    let event_state_mask = [
        (Modifiers::SHIFT, "Shift"),
        (Modifiers::CTRL, "Control"),
        (Modifiers::ALT, "Mod1"),
        (Modifiers::ISO_LEVEL5_SHIFT, "Mod3"),
        (Modifiers::SUPER, "Mod4"),
        (Modifiers::ISO_LEVEL3_SHIFT, "Mod5"),
    ]
    .into_iter()
    .filter(|(modifier, _)| modifiers.contains(*modifier))
    .map(|(_, name)| name.into())
    .collect();
    let (input_codes, input_code, symbols, symbol, input_type) = match bind.key.trigger {
        Trigger::Keycode(code) => (vec![code], code, vec![], None, "keyboard"),
        Trigger::Keysym(keysym) => {
            let symbol = keysym_get_name(keysym);
            (vec![], 0, vec![symbol.clone()], Some(symbol), "keyboard")
        }
        Trigger::MouseLeft
        | Trigger::MouseRight
        | Trigger::MouseMiddle
        | Trigger::MouseBack
        | Trigger::MouseForward
        | Trigger::WheelScrollDown
        | Trigger::WheelScrollUp
        | Trigger::WheelScrollLeft
        | Trigger::WheelScrollRight => {
            let symbol: String = match bind.key.trigger {
                Trigger::MouseLeft => "button1",
                Trigger::MouseRight => "button2",
                Trigger::MouseMiddle => "button3",
                Trigger::MouseBack => "button4",
                Trigger::MouseForward => "button5",
                Trigger::WheelScrollUp => "button4",
                Trigger::WheelScrollDown => "button5",
                Trigger::WheelScrollLeft => "button6",
                Trigger::WheelScrollRight => "button7",
                _ => unreachable!(),
            }
            .into();
            (vec![], 0, vec![symbol.clone()], Some(symbol), "mouse")
        }
        _ => return None,
    };
    Some(swayward_ipc::legacy::Event::SwayBinding {
        command: command.clone(),
        event_state_mask,
        input_codes,
        input_code,
        symbols,
        symbol,
        input_type: input_type.into(),
    })
}

pub(super) fn translated_modifiers(
    keysym: &smithay::input::keyboard::KeysymHandle<'_>,
    keycode: Keycode,
    mut mods: ModifiersState,
) -> ModifiersState {
    let xkb = keysym.xkb().lock().unwrap();
    // SAFETY: neither reference outlives the locked Xkb value.
    let state = unsafe { xkb.state() };
    let consumed = state.key_get_consumed_mods(keycode)
        & state.serialize_mods(smithay::input::keyboard::xkb::STATE_MODS_EFFECTIVE);
    // SAFETY: the keymap reference does not outlive the locked Xkb value.
    let keymap = unsafe { xkb.keymap() };
    let consumed_named = |name| {
        let index = keymap.mod_get_index(name);
        index != smithay::input::keyboard::xkb::MOD_INVALID && consumed & (1 << index) != 0
    };
    mods.ctrl &= !consumed_named(smithay::input::keyboard::xkb::MOD_NAME_CTRL);
    mods.alt &= !consumed_named(smithay::input::keyboard::xkb::MOD_NAME_ALT);
    mods.shift &= !consumed_named(smithay::input::keyboard::xkb::MOD_NAME_SHIFT);
    mods.logo &= !consumed_named(smithay::input::keyboard::xkb::MOD_NAME_LOGO);
    mods.iso_level3_shift &=
        !consumed_named(smithay::input::keyboard::xkb::MOD_NAME_ISO_LEVEL3_SHIFT);
    mods.iso_level5_shift &= !consumed_named(smithay::input::keyboard::xkb::MOD_NAME_MOD3);
    mods
}

pub(super) fn modifiers_from_state(mods: ModifiersState) -> Modifiers {
    let mut modifiers = Modifiers::empty();
    if mods.ctrl {
        modifiers |= Modifiers::CTRL;
    }
    if mods.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if mods.caps_lock {
        modifiers |= Modifiers::CAPS;
    }
    if mods.alt {
        modifiers |= Modifiers::ALT;
    }
    if mods.logo {
        modifiers |= Modifiers::SUPER;
    }
    if mods.num_lock {
        modifiers |= Modifiers::NUM;
    }
    if mods.iso_level3_shift {
        modifiers |= Modifiers::ISO_LEVEL3_SHIFT;
    }
    if mods.iso_level5_shift {
        modifiers |= Modifiers::ISO_LEVEL5_SHIFT;
    }
    modifiers
}

pub(super) fn should_activate_monitors<I: InputBackend>(event: &InputEvent<I>) -> bool {
    match event {
        InputEvent::Keyboard { event } if event.state() == KeyState::Pressed => true,
        InputEvent::PointerButton { event } if event.state() == ButtonState::Pressed => true,
        InputEvent::PointerMotion { .. }
        | InputEvent::PointerMotionAbsolute { .. }
        | InputEvent::PointerAxis { .. }
        | InputEvent::GestureSwipeBegin { .. }
        | InputEvent::GesturePinchBegin { .. }
        | InputEvent::GestureHoldBegin { .. }
        | InputEvent::TouchDown { .. }
        | InputEvent::TouchMotion { .. }
        | InputEvent::TabletToolAxis { .. }
        | InputEvent::TabletToolProximity { .. }
        | InputEvent::TabletToolTip { .. }
        | InputEvent::TabletToolButton { .. } => true,
        // Ignore events like device additions and removals, key releases, gesture ends.
        _ => false,
    }
}

pub(super) fn should_hide_hotkey_overlay<I: InputBackend>(event: &InputEvent<I>) -> bool {
    match event {
        InputEvent::Keyboard { event } if event.state() == KeyState::Pressed => true,
        InputEvent::PointerButton { event } if event.state() == ButtonState::Pressed => true,
        InputEvent::PointerAxis { .. }
        | InputEvent::GestureSwipeBegin { .. }
        | InputEvent::GesturePinchBegin { .. }
        | InputEvent::TouchDown { .. }
        | InputEvent::TouchMotion { .. }
        | InputEvent::TabletToolTip { .. }
        | InputEvent::TabletToolButton { .. } => true,
        _ => false,
    }
}

pub(super) fn should_hide_exit_confirm_dialog<I: InputBackend>(event: &InputEvent<I>) -> bool {
    match event {
        InputEvent::Keyboard { event } if event.state() == KeyState::Pressed => true,
        InputEvent::PointerButton { event } if event.state() == ButtonState::Pressed => true,
        InputEvent::PointerAxis { .. }
        | InputEvent::GestureSwipeBegin { .. }
        | InputEvent::GesturePinchBegin { .. }
        | InputEvent::TouchDown { .. }
        | InputEvent::TouchMotion { .. }
        | InputEvent::TabletToolTip { .. }
        | InputEvent::TabletToolButton { .. } => true,
        _ => false,
    }
}

pub(super) fn should_notify_activity<I: InputBackend>(event: &InputEvent<I>) -> bool {
    !matches!(
        event,
        InputEvent::DeviceAdded { .. } | InputEvent::DeviceRemoved { .. }
    )
}

pub(super) fn should_reset_pointer_inactivity_timer<I: InputBackend>(
    event: &InputEvent<I>,
) -> bool {
    matches!(
        event,
        InputEvent::PointerAxis { .. }
            | InputEvent::PointerButton { .. }
            | InputEvent::PointerMotion { .. }
            | InputEvent::PointerMotionAbsolute { .. }
            | InputEvent::TabletToolAxis { .. }
            | InputEvent::TabletToolButton { .. }
            | InputEvent::TabletToolProximity { .. }
            | InputEvent::TabletToolTip { .. }
    )
}

pub(super) fn allowed_when_locked(action: &Action) -> bool {
    matches!(
        action,
        Action::Quit(_)
            | Action::ChangeVt(_)
            | Action::Suspend
            | Action::PowerOffMonitors
            | Action::PowerOnMonitors
            | Action::SwitchLayout(_)
            | Action::ToggleKeyboardShortcutsInhibit
    )
}

pub(super) fn allowed_during_screenshot(action: &Action) -> bool {
    matches!(
        action,
        Action::Quit(_)
            | Action::ChangeVt(_)
            | Action::Suspend
            | Action::PowerOffMonitors
            | Action::PowerOnMonitors
            // Intended for binds such as volume up/down, lock the screen, etc.
            | Action::Spawn(_)
            | Action::SpawnSh(_)
            // The screenshot UI can handle these.
            | Action::MoveColumnLeft
            | Action::MoveColumnLeftOrToMonitorLeft
            | Action::MoveColumnRight
            | Action::MoveColumnRightOrToMonitorRight
            | Action::MoveWindowUp
            | Action::MoveWindowUpOrToWorkspaceUp
            | Action::MoveWindowDown
            | Action::MoveWindowDownOrToWorkspaceDown
            | Action::MoveColumnToMonitorLeft
            | Action::MoveColumnToMonitorRight
            | Action::MoveColumnToMonitorUp
            | Action::MoveColumnToMonitorDown
            | Action::MoveColumnToMonitorPrevious
            | Action::MoveColumnToMonitorNext
            | Action::MoveColumnToMonitor(_)
            | Action::MoveWindowToMonitorLeft
            | Action::MoveWindowToMonitorRight
            | Action::MoveWindowToMonitorUp
            | Action::MoveWindowToMonitorDown
            | Action::MoveWindowToMonitorPrevious
            | Action::MoveWindowToMonitorNext
            | Action::MoveWindowToMonitor(_)
            | Action::SetWindowWidth(_)
            | Action::SetWindowHeight(_)
            | Action::SetColumnWidth(_)
    )
}

pub(crate) fn hardcoded_overview_bind(raw: Keysym, mods: ModifiersState) -> Option<Bind> {
    // Caps Lock and Num Lock are states, not chords. find_bind already ignores
    // them when matching configured binds; requiring them clear here meant a
    // keyboard with Num Lock on -- which `input { keyboard { numlock } }` makes
    // the default -- rejected every overview key while the mouse still worked.
    let locks = Modifiers::CAPS | Modifiers::NUM;
    let mods = modifiers_from_state(mods).difference(locks);
    if !mods.is_empty() {
        return None;
    }

    let mut repeat = true;
    let action = match raw {
        Keysym::Escape | Keysym::Return => {
            repeat = false;
            Action::ToggleOverview
        }
        // Unlike niri's scrolling layout, the tree can consume directional focus inside the
        // current workspace. In the overview, arrows always select another workspace.
        Keysym::Left | Keysym::Up => Action::FocusWorkspaceUp,
        Keysym::Right | Keysym::Down => Action::FocusWorkspaceDown,
        _ => {
            return None;
        }
    };

    Some(Bind {
        key: Key {
            trigger: Trigger::Keysym(raw),
            modifiers: Modifiers::empty(),
        },
        action,
        mouse_regions: MouseRegions::empty(),
        input_device: "*".into(),
        group: None,
        release: false,
        repeat,
        cooldown: None,
        allow_when_locked: false,
        allow_inhibiting: false,
        hotkey_overlay_title: None,
    })
}

pub(super) fn grab_allows_hot_corner(grab: &(dyn PointerGrab<State> + 'static)) -> bool {
    let grab = grab.as_any();

    // We lean on the blocklist approach here since it's not a terribly big deal if hot corner
    // works where it shouldn't, but it could prevent some workflows if the hot corner doesn't work
    // when it should.
    //
    // Some notable grabs not mentioned here:
    // - DnDGrab allows hot corner to DnD across workspaces.
    // - ClickGrab keeps pointer focus on the window, so the hot corner doesn't trigger.
    // - Touch grabs: touch doesn't trigger the hot corner.
    if grab.is::<ResizeGrab>() || grab.is::<SpatialMovementGrab>() {
        return false;
    }

    if let Some(grab) = grab.downcast_ref::<MoveGrab>() {
        // Window move allows hot corner to DnD across workspaces.
        if !grab.is_move() {
            return false;
        }
    }

    true
}

/// Returns an iterator over bindings.
///
/// Includes dynamically populated bindings like the MRU UI.
pub(super) fn make_binds_iter<'a>(
    config: &'a Config,
    binding_mode: &str,
    mru: &'a mut WindowMruUi,
    mods: Modifiers,
) -> impl Iterator<Item = &'a Bind> + Clone {
    // Figure out the binds to use depending on the active mode and whether the MRU is open.
    let mode_binds = config
        .binding_modes
        .iter()
        .find(|mode| mode.name == binding_mode)
        .into_iter()
        .flat_map(|mode| mode.binds.0.iter());
    let general_binds =
        (!mru.is_open() && binding_mode == "default").then_some(config.binds.0.iter());
    let general_binds = general_binds.into_iter().flatten();

    let mru_binds =
        (config.recent_windows.on || mru.is_open()).then_some(config.recent_windows.binds.iter());
    let mru_binds = mru_binds.into_iter().flatten();

    let mru_open_binds = mru.is_open().then(|| mru.opened_bindings(mods));
    let mru_open_binds = mru_open_binds.into_iter().flatten();

    // General binds take precedence over the MRU binds.
    mode_binds
        .chain(general_binds)
        .chain(mru_binds)
        .chain(mru_open_binds)
}

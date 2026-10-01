use std::cell::{Cell, RefCell};

use super::libinput::{apply_pointer_settings, PointerSetting};
use super::*;
use crate::animation::Clock;

#[test]
fn libinput_pointer_settings_report_failures_and_continue() {
    let settings = [
        PointerSetting::NaturalScroll(true),
        PointerSetting::AccelSpeed(0.5),
        PointerSetting::LeftHanded(true),
    ];
    let mut applied = Vec::new();

    let failures = apply_pointer_settings(settings, |setting| {
        applied.push(setting.name());
        if matches!(setting, PointerSetting::AccelSpeed(_)) {
            Err(input::DeviceConfigError::Unsupported)
        } else {
            Ok(())
        }
    });

    assert_eq!(applied, ["natural-scroll", "accel-speed", "left-handed"]);
    assert_eq!(
        failures,
        [("accel-speed", input::DeviceConfigError::Unsupported)]
    );
}

#[test]
fn mouse_region_matching_uses_intersection_except_for_workspace_background() {
    let whole = MouseRegions::all();
    assert!(mouse_regions_match(whole, MouseRegions::CONTENTS, false));
    assert!(mouse_regions_match(whole, MouseRegions::all(), true));
    assert!(!mouse_regions_match(
        MouseRegions::BORDER,
        MouseRegions::all(),
        true
    ));
    assert!(!mouse_regions_match(
        MouseRegions::TITLEBAR,
        MouseRegions::CONTENTS,
        false
    ));
}

fn binding(command: &str, group: Option<u8>) -> Bind {
    Bind {
        key: Key {
            trigger: Trigger::Keysym(Keysym::q),
            modifiers: Modifiers::empty(),
        },
        action: Action::SwayCommand(command.into()),
        mouse_regions: MouseRegions::empty(),
        input_device: "*".into(),
        group,
        release: false,
        repeat: true,
        cooldown: None,
        allow_when_locked: false,
        allow_inhibiting: true,
        hotkey_overlay_title: None,
    }
}

#[test]
fn exact_xkb_group_beats_the_wildcard_and_wrong_groups_do_not_match() {
    let wildcard = binding("nop wildcard", None);
    let group_2 = binding("nop group-2", Some(1));
    let bindings = [&wildcard, &group_2];
    assert_eq!(
        find_configured_bind_with_context(
            bindings,
            ModKey::Super,
            &[Trigger::Keysym(Keysym::q)],
            ModifiersState::default(),
            BindingContext {
                input_device: "*",
                group: 1,
                locked: false,
                inhibited: false,
            },
        )
        .as_ref(),
        Some(&group_2)
    );
    assert_eq!(
        find_configured_bind_with_context(
            [&group_2],
            ModKey::Super,
            &[Trigger::Keysym(Keysym::q)],
            ModifiersState::default(),
            BindingContext {
                input_device: "*",
                group: 0,
                locked: false,
                inhibited: false,
            },
        ),
        None
    );
}

#[test]
fn exact_input_beats_group_lock_and_inhibition_matches() {
    let wildcard = binding("nop wildcard", Some(1));
    let mut exact = binding("nop exact", None);
    exact.input_device = "0:0:keyboard".into();
    let bindings = [&wildcard, &exact];

    assert_eq!(
        find_configured_bind_with_context(
            bindings,
            ModKey::Super,
            &[Trigger::Keysym(Keysym::q)],
            ModifiersState::default(),
            BindingContext {
                input_device: "0:0:keyboard",
                group: 1,
                locked: false,
                inhibited: false,
            },
        )
        .as_ref(),
        Some(&exact)
    );
}

#[test]
fn group_agnostic_binding_matches_every_xkb_group() {
    let wildcard = binding("nop wildcard", None);
    for group in [0, 1, 3] {
        assert_eq!(
            find_configured_bind_with_context(
                [&wildcard],
                ModKey::Super,
                &[Trigger::Keysym(Keysym::q)],
                ModifiersState::default(),
                BindingContext {
                    input_device: "*",
                    group,
                    locked: false,
                    inhibited: false,
                },
            )
            .as_ref(),
            Some(&wildcard),
        );
    }
}

#[test]
fn release_bindings_fire_only_when_the_chord_is_released() {
    let keysym = Keysym::x;
    let key_code = Keycode::from(keysym.raw() + 8);
    let bindings = Binds(vec![Bind {
        key: Key {
            trigger: Trigger::Keysym(keysym),
            modifiers: Modifiers::SHIFT,
        },
        action: Action::SwayCommand("nop release".into()),
        mouse_regions: MouseRegions::empty(),
        input_device: "*".into(),
        group: None,
        release: true,
        repeat: false,
        cooldown: None,
        allow_when_locked: false,
        allow_inhibiting: true,
        hotkey_overlay_title: None,
    }]);
    let screenshot_ui = ScreenshotUi::new(Clock::default(), Default::default());
    let mods = ModifiersState {
        shift: true,
        ..Default::default()
    };
    let mut suppressed_keys = HashSet::new();
    let mut held_release_bind = None;

    let press = should_intercept_key(
        &mut suppressed_keys,
        &mut held_release_bind,
        &bindings.0,
        KeyEventContext {
            input_device: "*",
            key_code,
            modified: keysym,
            raw: Some(keysym),
            group: 0,
            code_modifiers: mods,
            raw_modifiers: mods,
            translated_modifiers: mods,
        },
        true,
        &screenshot_ui,
        BindingPolicy {
            mod_key: ModKey::Super,
            locked: false,
            inhibited: false,
            disable_power_key_handling: false,
        },
    );
    assert!(matches!(press, FilterResult::Intercept(None)));
    assert!(held_release_bind.is_some());

    let release = should_intercept_key(
        &mut suppressed_keys,
        &mut held_release_bind,
        &bindings.0,
        KeyEventContext {
            input_device: "*",
            key_code,
            modified: keysym,
            raw: Some(keysym),
            group: 0,
            code_modifiers: mods,
            raw_modifiers: mods,
            translated_modifiers: mods,
        },
        false,
        &screenshot_ui,
        BindingPolicy {
            mod_key: ModKey::Super,
            locked: false,
            inhibited: false,
            disable_power_key_handling: false,
        },
    );
    assert!(matches!(
        release,
        FilterResult::Intercept(Some(Bind { release: true, .. }))
    ));
    assert!(held_release_bind.is_none());
}

#[test]
fn numlock_is_a_distinct_locked_modifier_for_binding_match() {
    let bind = Bind {
        key: Key {
            trigger: Trigger::Keysym(Keysym::a),
            modifiers: Modifiers::NUM,
        },
        action: Action::CloseWindow,
        mouse_regions: MouseRegions::empty(),
        input_device: "*".into(),
        group: None,
        release: false,
        repeat: true,
        cooldown: None,
        allow_when_locked: false,
        allow_inhibiting: true,
        hotkey_overlay_title: None,
    };
    assert!(find_configured_bind(
        [&bind],
        ModKey::Super,
        Trigger::Keysym(Keysym::a),
        ModifiersState::default(),
    )
    .is_none());
    assert!(find_configured_bind(
        [&bind],
        ModKey::Super,
        Trigger::Keysym(Keysym::a),
        ModifiersState {
            num_lock: true,
            ..Default::default()
        },
    )
    .is_some());
}

#[test]
fn bindings_suppress_keys() {
    let close_keysym = Keysym::q;
    let bindings = Binds(vec![Bind {
        key: Key {
            trigger: Trigger::Keysym(close_keysym),
            modifiers: Modifiers::COMPOSITOR | Modifiers::CTRL,
        },
        action: Action::CloseWindow,
        mouse_regions: MouseRegions::empty(),
        input_device: "*".into(),
        group: None,
        release: false,
        repeat: true,
        cooldown: None,
        allow_when_locked: false,
        allow_inhibiting: true,
        hotkey_overlay_title: None,
    }]);

    let comp_mod = ModKey::Super;
    let mut suppressed_keys = HashSet::new();
    let held_release_bind = RefCell::new(None);

    let screenshot_ui = ScreenshotUi::new(Clock::default(), Default::default());
    let disable_power_key_handling = false;
    let is_inhibiting_shortcuts = Cell::new(false);

    // The key_code we pick is arbitrary, the only thing
    // that matters is that they are different between cases.

    let close_key_code = Keycode::from(close_keysym.raw() + 8u32);
    let close_key_event = |suppr: &mut HashSet<Keycode>, mods: ModifiersState, pressed| {
        should_intercept_key(
            suppr,
            &mut held_release_bind.borrow_mut(),
            &bindings.0,
            KeyEventContext {
                input_device: "*",
                key_code: close_key_code,
                modified: close_keysym,
                raw: Some(close_keysym),
                group: 0,
                code_modifiers: mods,
                raw_modifiers: mods,
                translated_modifiers: mods,
            },
            pressed,
            &screenshot_ui,
            BindingPolicy {
                mod_key: comp_mod,
                locked: false,
                inhibited: is_inhibiting_shortcuts.get(),
                disable_power_key_handling,
            },
        )
    };

    // Key event with the code which can't trigger any action.
    let none_key_event = |suppr: &mut HashSet<Keycode>, mods: ModifiersState, pressed| {
        should_intercept_key(
            suppr,
            &mut held_release_bind.borrow_mut(),
            &bindings.0,
            KeyEventContext {
                input_device: "*",
                key_code: Keycode::from(Keysym::l.raw() + 8),
                modified: Keysym::l,
                raw: Some(Keysym::l),
                group: 0,
                code_modifiers: mods,
                raw_modifiers: mods,
                translated_modifiers: mods,
            },
            pressed,
            &screenshot_ui,
            BindingPolicy {
                mod_key: comp_mod,
                locked: false,
                inhibited: is_inhibiting_shortcuts.get(),
                disable_power_key_handling,
            },
        )
    };

    let mut mods = ModifiersState {
        logo: true,
        ctrl: true,
        ..Default::default()
    };

    // Action press/release.

    let filter = close_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(
        filter,
        FilterResult::Intercept(Some(Bind {
            action: Action::CloseWindow,
            ..
        }))
    ));
    assert!(suppressed_keys.contains(&close_key_code));

    let filter = close_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Intercept(None)));
    assert!(suppressed_keys.is_empty());

    // Remove mod to make it for a binding.

    mods.shift = true;
    let filter = close_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(filter, FilterResult::Forward));

    mods.shift = false;
    let filter = close_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Forward));

    // Just none press/release.

    let filter = none_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(filter, FilterResult::Forward));

    let filter = none_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Forward));

    // Press action, press arbitrary, release action, release arbitrary.

    let filter = close_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(
        filter,
        FilterResult::Intercept(Some(Bind {
            action: Action::CloseWindow,
            ..
        }))
    ));

    let filter = none_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(filter, FilterResult::Forward));

    let filter = close_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Intercept(None)));

    let filter = none_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Forward));

    // Trigger and remove all mods.

    let filter = close_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(
        filter,
        FilterResult::Intercept(Some(Bind {
            action: Action::CloseWindow,
            ..
        }))
    ));

    mods = Default::default();
    let filter = close_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Intercept(None)));

    // Ensure that no keys are being suppressed.
    assert!(suppressed_keys.is_empty());

    // Now test shortcut inhibiting.

    // With inhibited shortcuts, we don't intercept our shortcut.
    is_inhibiting_shortcuts.set(true);

    mods = ModifiersState {
        logo: true,
        ctrl: true,
        ..Default::default()
    };

    let filter = close_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(filter, FilterResult::Forward));
    assert!(suppressed_keys.is_empty());

    let filter = close_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Forward));
    assert!(suppressed_keys.is_empty());

    // Toggle it off after pressing the shortcut.
    let filter = close_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(filter, FilterResult::Forward));
    assert!(suppressed_keys.is_empty());

    is_inhibiting_shortcuts.set(false);

    let filter = close_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Forward));
    assert!(suppressed_keys.is_empty());

    // Toggle it on after pressing the shortcut.
    let filter = close_key_event(&mut suppressed_keys, mods, true);
    assert!(matches!(
        filter,
        FilterResult::Intercept(Some(Bind {
            action: Action::CloseWindow,
            ..
        }))
    ));
    assert!(suppressed_keys.contains(&close_key_code));

    is_inhibiting_shortcuts.set(true);

    let filter = close_key_event(&mut suppressed_keys, mods, false);
    assert!(matches!(filter, FilterResult::Intercept(None)));
    assert!(suppressed_keys.is_empty());
}

#[test]
fn comp_mod_handling() {
    let bindings = Binds(vec![
        Bind {
            key: Key {
                trigger: Trigger::Keysym(Keysym::q),
                modifiers: Modifiers::COMPOSITOR,
            },
            action: Action::CloseWindow,
            mouse_regions: MouseRegions::empty(),
            input_device: "*".into(),
            group: None,
            release: false,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        },
        Bind {
            key: Key {
                trigger: Trigger::Keysym(Keysym::h),
                modifiers: Modifiers::SUPER,
            },
            action: Action::FocusColumnLeft,
            mouse_regions: MouseRegions::empty(),
            input_device: "*".into(),
            group: None,
            release: false,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        },
        Bind {
            key: Key {
                trigger: Trigger::Keysym(Keysym::j),
                modifiers: Modifiers::empty(),
            },
            action: Action::FocusWindowDown,
            mouse_regions: MouseRegions::empty(),
            input_device: "*".into(),
            group: None,
            release: false,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        },
        Bind {
            key: Key {
                trigger: Trigger::Keysym(Keysym::k),
                modifiers: Modifiers::COMPOSITOR | Modifiers::SUPER,
            },
            action: Action::FocusWindowUp,
            mouse_regions: MouseRegions::empty(),
            input_device: "*".into(),
            group: None,
            release: false,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        },
        Bind {
            key: Key {
                trigger: Trigger::Keysym(Keysym::l),
                modifiers: Modifiers::SUPER | Modifiers::ALT,
            },
            action: Action::FocusColumnRight,
            mouse_regions: MouseRegions::empty(),
            input_device: "*".into(),
            group: None,
            release: false,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        },
    ]);

    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::q),
            ModifiersState {
                logo: true,
                ..Default::default()
            }
        )
        .as_ref(),
        Some(&bindings.0[0])
    );
    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::q),
            ModifiersState::default(),
        ),
        None,
    );

    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::h),
            ModifiersState {
                logo: true,
                ..Default::default()
            }
        )
        .as_ref(),
        Some(&bindings.0[1])
    );
    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::h),
            ModifiersState::default(),
        ),
        None,
    );

    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::j),
            ModifiersState {
                logo: true,
                ..Default::default()
            }
        ),
        None,
    );
    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::j),
            ModifiersState::default(),
        )
        .as_ref(),
        Some(&bindings.0[2])
    );

    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::k),
            ModifiersState {
                logo: true,
                ..Default::default()
            }
        )
        .as_ref(),
        Some(&bindings.0[3])
    );
    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::k),
            ModifiersState::default(),
        ),
        None,
    );

    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::l),
            ModifiersState {
                logo: true,
                alt: true,
                ..Default::default()
            }
        )
        .as_ref(),
        Some(&bindings.0[4])
    );
    assert_eq!(
        find_configured_bind(
            &bindings.0,
            ModKey::Super,
            Trigger::Keysym(Keysym::l),
            ModifiersState {
                logo: true,
                ..Default::default()
            },
        ),
        None,
    );
}

/// Sway lists binding modifiers in get_modifier_names order: Shift, Lock,
/// Control, Mod1, Mod2, Mod3, Mod4, Mod5 (sway/sway/input/keyboard.c:26-38).
#[test]
fn binding_event_state_mask_uses_sways_modifier_names_and_order() {
    let mut bind = binding("nop all", None);
    bind.key.modifiers = Modifiers::all() - Modifiers::COMPOSITOR;
    let Some(swayward_ipc::legacy::Event::SwayBinding {
        event_state_mask, ..
    }) = sway_binding_event(&bind, ModKey::Super)
    else {
        panic!("expected a binding event");
    };
    assert_eq!(
        event_state_mask,
        ["Shift", "Lock", "Control", "Mod1", "Mod2", "Mod3", "Mod4", "Mod5"]
    );
}

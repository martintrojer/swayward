/// Runtime key binding commands must mutate the exact table the keyboard path
/// reads: add must fire, unbind must stop firing, and reload must restore the
/// file-backed table rather than retaining runtime mutations.
#[test]
fn runtime_bindsym_fires_unbinds_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let added = crate::command::execute(
        fixture.niri_state(),
        "bindsym Mod4+a workspace runtime-bound",
    );
    assert!(added[0].success, "{added:?}");
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("runtime-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace unbound-check")[0].success);
    let removed = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+a");
    assert!(removed[0].success, "{removed:?}");
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("unbound-check")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace should-not-survive",
        )[0]
        .success
    );
    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-check")[0].success);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("reload-check")
    );
}

#[test]
fn runtime_bindcode_fires_and_unbindcode_stops_it() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(fixture.niri_state(), "bindcode 39 workspace code-bound",)[0]
            .success
    );
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("code-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "unbindcode 39")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace code-unbound")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("code-unbound")
    );
}

#[test]
fn runtime_bindsym_release_fires_only_on_key_release() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym --release a workspace released",
        )[0]
        .success
    );

    key_event(&mut fixture, 38, true);
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("released")
    );
    key_event(&mut fixture, 38, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("released")
    );
}

/// Sway replaces an equal binding rather than appending a competing one
/// (`binding_upsert`, sway/sway/commands/bind.c:266-278).
#[test]
fn runtime_bindsym_duplicate_overwrites_the_old_command() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace first-command",
        )[0]
        .success
    );
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace replacement-command",
        )[0]
        .success
    );
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);

    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("replacement-command")
    );
    assert_eq!(fixture.swayward().config.borrow().binds.0.len(), 1);
}

/// `unbind*` reports failure when no binding has the same key and flags
/// (`sway/sway/commands/bind.c:304-321`) and leaves the table alone.
#[test]
fn runtime_unbindsym_missing_binding_fails_without_mutation() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let before = fixture.swayward().config.borrow().binds.0.clone();

    let outcome = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+a");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Could not find binding `Mod4+a` for the given flags")
    );
    assert_eq!(fixture.swayward().config.borrow().binds.0, before);
}

/// Sway rejects an unknown keysym before looking for an existing binding
/// (`sway/sway/commands/bind.c:194-202`).
#[test]
fn runtime_unbindsym_unknown_keysym_is_a_parse_error() {
    let mut fixture = Fixture::new();
    let outcome = crate::command::execute(fixture.niri_state(), "unbindsym λ-日本語-🙂");

    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Unknown key or button 'λ-日本語-🙂'")
    );
    assert_eq!(outcome[0].parse_error, Some(true));
}

/// Top-level runtime binds target sway's current mode, not always the default
/// (`sway/sway/commands/bind.c:476-482`).
#[test]
fn runtime_bindsym_mutates_the_active_binding_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds { Mod4+a { command "workspace default-mode"; }; }
mode "resize" {
    Mod4+b { command "nop"; };
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+a workspace resize-mode",)[0]
            .success
    );

    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("resize-mode")
    );

    // The default table was not overwritten.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-mode")
    );
}

/// Runtime variable substitution happens before cmd_bindsym stores its command,
/// so redefining the variable later does not rewrite the captured binding.
#[test]
fn runtime_bindsym_captures_the_current_variable_value() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "set $dest captured")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+a workspace $dest",)[0].success
    );
    assert!(crate::command::execute(fixture.niri_state(), "set $dest later")[0].success);

    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("captured")
    );
}

/// A binding may mutate the binding table while it is itself being dispatched.
/// Sway does this safely because the table is a plain list; swayward holds it
/// behind a `RefCell`, so a live borrow across dispatch would panic rather than
/// misbehave. Drive the reentrant case through real key input to prove the
/// borrow is released before the command runs.
#[test]
fn a_binding_may_rebind_and_unbind_itself_while_dispatching() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Mod4+a { command "bindsym Mod4+b workspace chained"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    // Mod4+a adds Mod4+b from inside its own dispatch.
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 56, true);
    key_event(&mut fixture, 56, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("chained")
    );

    // A binding that removes itself takes effect from the next press onwards.
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+c unbindsym Mod4+c",)[0]
            .success
    );
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 54, true);
    key_event(&mut fixture, 54, false);
    key_event(&mut fixture, 133, false);
    let outcome = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+c");
    assert!(
        !outcome[0].success,
        "the self-unbinding bind should already be gone: {outcome:?}"
    );
}

#[test]
fn runtime_bindswitch_fires_unbinds_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch lid:on workspace switch-bound",
        )[0]
        .success
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "unbindswitch lid:on")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace switch-unbound")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-unbound")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch lid:on workspace should-not-survive",
        )[0]
        .success
    );
    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(crate::command::execute(fixture.niri_state(), "workspace switch-reload")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-reload")
    );
}

#[test]
fn runtime_bindswitch_respects_mode_and_toggle_trigger() {
    let config =
        swayward_config::Config::parse_mem(r#"mode "switch-mode" { x { command "nop"; }; }"#)
            .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode switch-mode")[0].success);
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch tablet:toggle workspace toggled",
        )[0]
        .success
    );

    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("toggled")
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace before-off")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::Off,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("toggled")
    );

    // The mode-local binding is inactive in default mode.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "workspace default-switch-mode")[0].success
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-switch-mode")
    );
}

#[test]
fn runtime_bindswitch_refuses_to_shadow_a_narrower_kdl_switch_event() {
    let config = swayward_config::Config::parse_mem(
        r#"
switch-events {
    lid-close { spawn "true"; }
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "bindswitch lid:on workspace would-shadow",
    );
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("runtime switch binding conflicts with a narrower KDL switch-event binding")
    );
    assert!(fixture.swayward().runtime_switch_bindings.is_empty());
}

/// Gesture binds remain honest failures: gesture events have no sway command
/// binding table or matching path.
#[test]
fn unsupported_runtime_gesture_binds_do_not_mutate_key_table() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let before = fixture.swayward().config.borrow().binds.0.clone();

    for command in ["bindgesture swipe:3:left nop", "unbindgesture swipe:3:left"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(!outcome[0].success, "{command}: {outcome:?}");
    }
    assert_eq!(fixture.swayward().config.borrow().binds.0, before);
}

#[test]
fn numlock_qualified_binding_dispatches_only_while_numlock_is_active() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Num+a { command "rename workspace to numlocked"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_none());

    key_event(&mut fixture, 77, true);
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .num_lock
    );
    key_event(&mut fixture, 77, false);
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .num_lock
    );
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_some());
}

#[test]
fn unqualified_binding_dispatches_while_numlock_is_active() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Mod4+a { command "rename workspace to numlocked"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 77, true);
    key_event(&mut fixture, 77, false);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_some());
}

#[test]
fn modifier_bindcode_matches_without_its_own_modifier() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { "code:133" release=true { command "rename workspace to super-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 133, true);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("super-release")
        .is_none());
    key_event(&mut fixture, 133, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("super-release")
        .is_some());
}


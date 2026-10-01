#[test]
fn input_policy_settings_apply_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let layout = |f: &mut Fixture| f.swayward().config.borrow().layout.clone();

    // Values sway rejects must fail here too, with sway's message.
    let outcome = crate::command::execute(f.niri_state(), "focus_follows_mouse maybe");
    assert!(
        !outcome[0].success,
        "invalid focus_follows_mouse should have failed"
    );
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'focus_follows_mouse no|yes|always'")
    );

    assert!(crate::command::execute(f.niri_state(), "show_marks no")[0].success);
    assert!(!layout(&mut f).titlebar.show_marks);
    assert!(crate::command::execute(f.niri_state(), "title_align right")[0].success);
    assert_eq!(
        layout(&mut f).titlebar.alignment,
        swayward_config::TitleAlignment::Right
    );
    assert!(crate::command::execute(f.niri_state(), "smart_gaps inverse_outer")[0].success);
    assert_eq!(
        layout(&mut f).smart_gaps,
        swayward_config::SmartGaps::InverseOuter
    );
    assert!(crate::command::execute(f.niri_state(), "smart_gaps toggle")[0].success);
    assert_eq!(layout(&mut f).smart_gaps, swayward_config::SmartGaps::Off);

    assert!(crate::command::execute(f.niri_state(), "tiling_drag no")[0].success);
    assert!(!f.swayward().config.borrow().input.tiling_drag);
    assert!(crate::command::execute(f.niri_state(), "tiling_drag toggle")[0].success);
    assert!(f.swayward().config.borrow().input.tiling_drag);
    assert!(crate::command::execute(f.niri_state(), "tiling_drag_threshold 17")[0].success);
    assert_eq!(f.swayward().config.borrow().input.tiling_drag_threshold, 17);
    assert!(crate::command::execute(f.niri_state(), "force_display_urgency_hint 700ms")[0].success);
    assert_eq!(f.swayward().config.borrow().urgent_timeout_ms.0, 700);
    // Oracle: state/settings_urgency_hint_parse
    // (`sway/sway/commands/force_display_urgency_hint.c:12-23`).
    for (command, error) in [
        (
            "force_display_urgency_hint 5msms",
            "timeout integer invalid",
        ),
        (
            "force_display_urgency_hint 500 extra",
            "Expected 'force_display_urgency_hint <timeout> [ms]'",
        ),
    ] {
        let outcome = &crate::command::execute(f.niri_state(), command)[0];
        assert_eq!(outcome.error.as_deref(), Some(error), "{command}");
        assert_eq!(outcome.parse_error, Some(true), "{command}");
    }
    for command in [
        "force_display_urgency_hint 500 ms",
        "force_display_urgency_hint 500 ms extra",
    ] {
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command}"
        );
    }
    assert_eq!(f.swayward().config.borrow().urgent_timeout_ms.0, 500);

    assert!(crate::command::execute(f.niri_state(), "focus_on_window_activation none")[0].success);
    assert_eq!(
        f.swayward().config.borrow().focus_on_window_activation,
        swayward_config::FocusOnWindowActivation::None
    );

    // focus_follows_mouse and workspace_auto_back_and_forth live under input.
    // Sway keeps three distinct states, and `always` is not `yes`
    // (`sway/include/sway/config.h:458-462`), so assert the stored mode and
    // not merely that the setting is enabled.
    use swayward_config::input::FocusFollowsMouseMode;
    let ffm = |f: &mut Fixture| f.swayward().config.borrow().input.focus_follows_mouse;
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse yes")[0].success);
    assert_eq!(
        ffm(&mut f).map(|v| v.mode),
        Some(FocusFollowsMouseMode::Yes)
    );
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse always")[0].success);
    assert_eq!(
        ffm(&mut f).map(|v| v.mode),
        Some(FocusFollowsMouseMode::Always),
        "`always` must be stored distinctly from `yes`"
    );
    assert!(crate::command::execute(f.niri_state(), "focus_follows_mouse no")[0].success);
    assert_eq!(
        ffm(&mut f),
        None,
        "sway's FOLLOWS_NO is the field's absence"
    );
    // Sway compares these three with strcmp, so they are case-sensitive
    // (`sway/sway/commands/focus_follows_mouse.c:9-18`).
    let outcome = crate::command::execute(f.niri_state(), "focus_follows_mouse ALWAYS");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'focus_follows_mouse no|yes|always'")
    );

    assert!(
        crate::command::execute(f.niri_state(), "workspace_auto_back_and_forth yes")[0].success
    );
    assert!(
        f.swayward()
            .config
            .borrow()
            .input
            .workspace_auto_back_and_forth
    );
}

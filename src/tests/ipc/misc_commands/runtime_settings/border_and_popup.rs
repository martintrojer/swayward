#[test]
fn border_and_popup_settings_apply_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let layout = |f: &mut Fixture| f.swayward().config.borrow().layout.clone();

    // Xwayland's mode is fixed at launch. Sway accepts the command and
    // refuses only a change (`sway/sway/commands/xwayland.c:24-28`), so
    // asking for the value already in effect succeeds and flipping it fails
    // with sway's message. The default config enables it.
    // default_border governs the border a new tiled window gets; sway keeps
    // the previous width when the command omits one
    // (`sway/sway/commands/default_border.c:22-24`). new_window and new_float
    // are the older i3 spellings of the same two settings.
    use swayward_config::layout::SwayBorderStyle;
    assert!(crate::command::execute(f.niri_state(), "default_border pixel 3")[0].success);
    assert_eq!(layout(&mut f).default_border.style, SwayBorderStyle::Pixel);
    assert_eq!(layout(&mut f).default_border.width, Some(3));
    assert!(crate::command::execute(f.niri_state(), "default_border normal")[0].success);
    assert_eq!(layout(&mut f).default_border.style, SwayBorderStyle::Normal);
    assert_eq!(
        layout(&mut f).default_border.width,
        Some(3),
        "omitting the width must keep the previous one"
    );
    assert!(crate::command::execute(f.niri_state(), "new_float none")[0].success);
    assert_eq!(
        layout(&mut f).default_floating_border.style,
        SwayBorderStyle::None,
        "new_float is the deprecated spelling of default_floating_border"
    );
    // Oracle: state/settings_default_border_parse. Styles are case-sensitive
    // and sway reads widths with atoi (`sway/sway/commands/default_border.c:12-24`).
    for bad in ["default_border csd", "default_border PIXEL"] {
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(outcome[0].parse_error, Some(true), "{bad}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("Expected 'default_border <none|normal|pixel>' or 'default_border <normal|pixel> <px>'")
        );
    }
    let outcome = crate::command::execute(f.niri_state(), "default_floating_border Normal");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'default_floating_border <none|normal|pixel>' or 'default_floating_border <normal|pixel> <px>'")
    );
    for (command, width) in [("default_border pixel wide", 0), ("default_border pixel 7px", 7)] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success, "{command}");
        assert_eq!(layout(&mut f).default_border.width, Some(width), "{command}");
    }

    // popup_during_fullscreen shares its accepted values and error string
    // with the KDL node, so IPC and the config file cannot drift.
    assert!(crate::command::execute(f.niri_state(), "popup_during_fullscreen ignore")[0].success);
    assert_eq!(
        f.swayward().config.borrow().popup_during_fullscreen,
        swayward_config::misc::PopupDuringFullscreen::Ignore
    );
    let outcome = crate::command::execute(f.niri_state(), "popup_during_fullscreen sometimes");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'popup_during_fullscreen smart|ignore|leave_fullscreen'")
    );


}

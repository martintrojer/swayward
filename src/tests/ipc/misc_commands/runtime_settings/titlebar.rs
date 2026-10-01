#[test]
fn titlebar_settings_apply_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let layout = |f: &mut Fixture| f.swayward().config.borrow().layout.clone();

    // `sway/sway/commands/font.c:13-22` strips a leading pango: prefix and joins the
    // remaining words, so the family and size survive as one string.
    assert!(crate::command::execute(f.niri_state(), "font pango:monospace 11")[0].success);
    assert_eq!(layout(&mut f).titlebar.font, "monospace 11");
    assert!(layout(&mut f).titlebar.pango_markup);
    assert!(f.swayward().layout.options().layout.titlebar.pango_markup);
    assert!(crate::command::execute(f.niri_state(), "font Sans Bold 9")[0].success);
    assert_eq!(layout(&mut f).titlebar.font, "Sans Bold 9");
    assert!(!layout(&mut f).titlebar.pango_markup);
    assert!(!f.swayward().layout.options().layout.titlebar.pango_markup);
    for (command, expected) in [
        ("font 11", "Invalid font family."),
        ("font monospace", "Invalid font size."),
    ] {
        let before = layout(&mut f).titlebar.clone();
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
        assert_eq!(layout(&mut f).titlebar, before);
    }

    // One value sets both axes; two set horizontal then vertical. Sway requires
    // both padding axes to be at least the current border thickness, and a new
    // thickness may not exceed the current vertical padding.
    assert!(crate::command::execute(f.niri_state(), "titlebar_padding 7")[0].success);
    assert_eq!(layout(&mut f).titlebar.horizontal_padding, 7.);
    assert_eq!(layout(&mut f).titlebar.vertical_padding, 7.);
    assert!(crate::command::execute(f.niri_state(), "titlebar_border_thickness 6")[0].success);
    assert_eq!(layout(&mut f).titlebar.border_thickness, 6);
    for bad in [
        "titlebar_border_thickness 8",
        "titlebar_border_thickness -1",
        "titlebar_border_thickness wide",
        "titlebar_padding 5 7",
        "titlebar_padding 7 5",
        "titlebar_padding -1",
        "titlebar_padding wide",
    ] {
        let before = layout(&mut f).titlebar.clone();
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some("Invalid size specified"));
        assert_eq!(layout(&mut f).titlebar, before);
    }
    assert!(crate::command::execute(f.niri_state(), "titlebar_padding 9 6")[0].success);
    assert_eq!(layout(&mut f).titlebar.horizontal_padding, 9.);
    assert_eq!(layout(&mut f).titlebar.vertical_padding, 6.);

    // focused_tab_title has no effective indicator or child-border colours in
    // sway, so the titlebar ring makes that class complete. The other classes
    // still use those colours on window borders and remain fail-loud.
    assert!(
        crate::command::execute(
            f.niri_state(),
            "client.focused_tab_title #123456 #abcdef #fedcba #010203 #040506"
        )[0]
        .success
    );
    let focused_tab = layout(&mut f).titlebar.focused_tab_title;
    assert_eq!(
        focused_tab.border_color.to_array_unpremul(),
        [
            0x12 as f32 / 255.,
            0x34 as f32 / 255.,
            0x56 as f32 / 255.,
            1.
        ]
    );
    assert_eq!(
        focused_tab.background_color.to_array_unpremul(),
        [
            0xab as f32 / 255.,
            0xcd as f32 / 255.,
            0xef as f32 / 255.,
            1.
        ]
    );
    assert_eq!(
        focused_tab.text_color.to_array_unpremul(),
        [
            0xfe as f32 / 255.,
            0xdc as f32 / 255.,
            0xba as f32 / 255.,
            1.
        ]
    );

    let before = layout(&mut f).titlebar;
    for command in [
        "client.focused #102030 #405060 #708090",
        "client.focused_inactive #112233 #445566 #778899",
        "client.unfocused #203040 #506070 #8090a0",
        "client.urgent #304050 #607080 #90a0b0",
        "client.focused #010203 #11223380 #44556640 #778899 #aabbcc",
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].parse_error, Some(true));
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("client colour commands are unsupported because sway window-border colours are not fully rendered")
        );
        assert_eq!(layout(&mut f).titlebar, before);
        assert_eq!(f.swayward().layout.options().layout.titlebar, before);
    }

    let before = layout(&mut f).titlebar.focused;
    for (command, expected) in [
        (
            "client.focused #000000 #111111",
            "Invalid client.focused command (expected at least 3 arguments, got 2)",
        ),
        (
            "client.focused #000000 #111111 #222222 #333333 #444444 #555555",
            "Invalid client.focused command (expected at most 5 arguments, got 6)",
        ),
        (
            "client.focused #000000 #111111 #222222 nope",
            "Invalid indicator color nope",
        ),
        (
            "client.focused #000000 #111111 #222222 #333333 nope",
            "Invalid child_border color nope",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
        assert_eq!(layout(&mut f).titlebar.focused, before);
    }


}

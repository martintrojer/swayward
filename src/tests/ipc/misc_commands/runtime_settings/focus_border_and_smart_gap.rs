#[test]
fn focus_border_and_smart_gap_settings_apply_at_runtime_like_sway() {
    use swayward_config::layout::{FocusWrapping, HideEdgeBorders, SmartBorders};

    // These directives are in sway's shared `handlers` table, which serves both
    // the config file and IPC (`sway/sway/commands.c:43-100,160-173`), so they
    // are live commands there. This test deliberately covers parsing and the
    // stored mode distinctions that are not observable until later input or
    // mapping. Consumer behavior is covered by geometry, real-input, border
    // and rendering tests; these field checks do not prove the consumers work.
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let layout = |f: &mut Fixture| f.swayward().config.borrow().layout.clone();

    for (command, check) in [
        (
            "focus_wrapping no",
            &(|l: &swayward_config::Layout| l.focus_wrapping == FocusWrapping::No)
                as &dyn Fn(&swayward_config::Layout) -> bool,
        ),
        ("focus_wrapping force", &|l| {
            l.focus_wrapping == FocusWrapping::Force
        }),
        // Deprecated in sway, kept as a boolean alias selecting between
        // force and yes (`sway/sway/commands/force_focus_wrapping.c:20-24`).
        ("force_focus_wrapping no", &|l| {
            l.focus_wrapping == FocusWrapping::Yes
        }),
        ("force_focus_wrapping yes", &|l| {
            l.focus_wrapping == FocusWrapping::Force
        }),
        ("hide_edge_borders both", &|l| {
            l.hide_edge_borders == HideEdgeBorders::Both
        }),
        // sway folds smart and smart_no_gaps into the smart-border toggle,
        // and smart resets the edge mode to none
        // (`sway/sway/commands/hide_edge_borders.c:34-39`).
        ("hide_edge_borders smart", &|l| {
            l.smart_borders == SmartBorders::On
                && l.hide_edge_borders == HideEdgeBorders::None
        }),
        ("smart_borders no_gaps", &|l| {
            l.smart_borders == SmartBorders::NoGaps
        }),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(outcome[0].success, "{command} failed: {outcome:?}");
        assert!(
            check(&layout(&mut f)),
            "{command} did not change the config"
        );
    }

    // Oracle: state/settings_hide_edge_borders_parse. --i3 counts only as
    // the first argument, values are case-sensitive, and later arguments are
    // ignored (`sway/sway/commands/hide_edge_borders.c:16-42`).
    let usage = "Expected 'hide_edge_borders [--i3] none|vertical|horizontal|both|smart|smart_no_gaps";
    for (command, error) in [
        ("hide_edge_borders NONE", usage),
        ("hide_edge_borders --i3", usage),
        ("hide_edge_borders --i3 --i3 none", usage),
        ("hide_edge_borders --i3 none", "hide_edge_borders --i3 is unsupported because swayward cannot hide a lone tab's title bar"),
    ] {
        let outcome = &crate::command::execute(f.niri_state(), command)[0];
        assert_eq!(outcome.error.as_deref(), Some(error), "{command}");
    }
    assert!(crate::command::execute(f.niri_state(), "hide_edge_borders vertical --i3")[0].success);
    assert_eq!(layout(&mut f).hide_edge_borders, HideEdgeBorders::Vertical);
}

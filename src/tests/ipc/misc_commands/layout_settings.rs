#[test]
fn layout_settings_apply_at_runtime_like_sway() {
    use swayward_config::layout::{FocusWrapping, HideEdgeBorders, SmartBorders};

    // These directives are in sway's shared `handlers` table, which serves both
    // the config file and IPC (`sway/sway/commands.c:43-100,160-173`), so they
    // are live commands there. This test deliberately covers parsing and the
    // stored mode distinctions that are not observable until later input or
    // mapping. Consumer behavior is covered by the geometry, real-input,
    // borders and rendering tests; do not treat these field checks as evidence
    // that a setting's consumer works.
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
        // force and yes (`sway/sway/commands/force_focus_wrapping.c`).
        ("force_focus_wrapping no", &|l| {
            l.focus_wrapping == FocusWrapping::Yes
        }),
        ("force_focus_wrapping yes", &|l| {
            l.focus_wrapping == FocusWrapping::Force
        }),
        ("hide_edge_borders both", &|l| {
            l.hide_edge_borders == HideEdgeBorders::Both
        }),
        // sway folds smart and smart_no_gaps into the smart-border toggle
        // rather than treating them as edge-border values.
        ("hide_edge_borders smart", &|l| {
            l.smart_borders == SmartBorders::On
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

    // Values sway rejects must fail here too, with sway's message.
    let outcome = crate::command::execute(f.niri_state(), "focus_follows_mouse maybe");
    assert!(!outcome[0].success, "invalid focus_follows_mouse should have failed");
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
    assert_eq!(f.swayward().config.borrow().urgent_timeout_ms, 700);
    // Oracle: state/settings_urgency_hint_parse. Only one "ms" suffix, and
    // a second argument must be "ms"; later ones are ignored
    // (`sway/sway/commands/force_display_urgency_hint.c:12-23`).
    for (command, error) in [
        ("force_display_urgency_hint 5msms", "timeout integer invalid"),
        (
            "force_display_urgency_hint 500 extra",
            "Expected 'force_display_urgency_hint <timeout> [ms]'",
        ),
    ] {
        let outcome = &crate::command::execute(f.niri_state(), command)[0];
        assert_eq!(outcome.error.as_deref(), Some(error), "{command}");
        assert_eq!(outcome.parse_error, Some(true), "{command}");
    }
    assert_eq!(f.swayward().config.borrow().urgent_timeout_ms, 700);
    for command in [
        "force_display_urgency_hint 500 ms",
        "force_display_urgency_hint 500 ms extra",
    ] {
        assert!(crate::command::execute(f.niri_state(), command)[0].success, "{command}");
    }
    assert_eq!(f.swayward().config.borrow().urgent_timeout_ms, 500);

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

    // sway parses these with strtol and requires a literal x between two
    // integers, rejecting a trailing suffix because it checks the remainder
    // (`sway/sway/commands/floating_minmax_size.c`).
    assert!(crate::command::execute(f.niri_state(), "floating_minimum_size 100 x 50")[0].success);
    assert_eq!(layout(&mut f).floating_minimum_size.width, 100);
    assert_eq!(layout(&mut f).floating_minimum_size.height, 50);
    assert!(crate::command::execute(f.niri_state(), "floating_maximum_size 800 x 600")[0].success);
    assert_eq!(layout(&mut f).floating_maximum_size.width, 800);
    assert_eq!(layout(&mut f).floating_maximum_size.height, 600);
    for (bad, expected) in [
        (
            "floating_minimum_size 100 50",
            "Invalid floating_minimum_size command (expected 3 arguments, got 2)",
        ),
        (
            "floating_minimum_size 100 x 50px",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
        (
            "floating_minimum_size 100 by 50",
            "Expected 'floating_minimum_size <width> x <height>'",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), bad);
        assert!(!outcome[0].success, "{bad} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected));
    }

    // `sway/sway/commands/font.c` strips a leading pango: prefix and joins the
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
    // Oracle: state/settings_default_border_parse. Sway matches the style
    // with strcmp and reads the width with atoi
    // (`sway/sway/commands/default_border.c:12-24`), so a capitalised style
    // fails and a width without digits is 0.
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

    // Sway spells the modifier Mod1..Mod5; swayward names them. The modifier
    // and the inverse bit are independent fields, and this is its own setting
    // rather than the compositor `mod_key`, which must not move
    // (`sway/include/sway/config.h:509-510`).
    use swayward_config::input::{FloatingModifier, ModKey};
    let floating = |f: &mut Fixture| f.swayward().config.borrow().input.floating_modifier;
    let mod_key_before = f.swayward().config.borrow().input.mod_key;
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: false
        })
    );
    assert_eq!(
        f.swayward().config.borrow().input.mod_key,
        mod_key_before,
        "floating_modifier must not move the compositor mod key"
    );
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Alt normal")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Alt,
            inverse: false
        })
    );
    // inverse is stored, not refused: it swaps the move and resize buttons.
    assert!(crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success);
    assert_eq!(
        floating(&mut f),
        Some(FloatingModifier {
            modifier: ModKey::Super,
            inverse: true
        })
    );
    // `none` is a value, not a key name, and sway returns before reading the
    // second argument (`sway/sway/commands/floating_modifier.c:11-14`), so a
    // trailing word is ignored and the inverse bit resets.
    for command in ["floating_modifier none", "floating_modifier NONE inverse"] {
        assert!(
            crate::command::execute(f.niri_state(), "floating_modifier Mod4 inverse")[0].success
        );
        assert!(
            crate::command::execute(f.niri_state(), command)[0].success,
            "{command} failed"
        );
        assert_eq!(
            floating(&mut f),
            Some(FloatingModifier {
                modifier: ModKey::None,
                inverse: false
            }),
            "{command} must disable the drag rather than name a key"
        );
    }
    // Sway validates the modifier before the mode, so an invalid modifier
    // wins over an invalid trailing word.
    for (command, expected) in [
        ("floating_modifier Mod9", "Invalid modifier"),
        ("floating_modifier Mod9 sideways", "Invalid modifier"),
        (
            "floating_modifier Mod4 sideways",
            "Usage: floating_modifier <mod> [inverse|normal]",
        ),
        (
            "floating_modifier",
            "Invalid floating_modifier command (expected at least 1 argument, got 0)",
        ),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert!(!outcome[0].success, "{command} should have failed");
        assert_eq!(outcome[0].error.as_deref(), Some(expected), "{command}");
    }

    // Sway keeps the three warping modes apart: `output` warps only across
    // outputs, `container` warps on every qualifying focus change
    // (`sway/sway/input/seat.c:1526-1547`). This is its own policy and must
    // not be folded into the inherited `warp-mouse-to-focus` centering mode.
    use swayward_config::input::MouseWarping;
    let warping = |f: &mut Fixture| f.swayward().config.borrow().input.mouse_warping;
    let warp_to_focus_before = f.swayward().config.borrow().input.warp_mouse_to_focus;
    assert!(crate::command::execute(f.niri_state(), "mouse_warping output")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Output);
    assert!(crate::command::execute(f.niri_state(), "mouse_warping container")[0].success);
    assert_eq!(
        warping(&mut f),
        MouseWarping::Container,
        "`container` must be stored distinctly from `output`"
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::No);
    assert_eq!(
        f.swayward().config.borrow().input.warp_mouse_to_focus,
        warp_to_focus_before,
        "mouse_warping must not overwrite the inherited centering option"
    );
    // strcasecmp, unlike focus_follows_mouse
    // (`sway/sway/commands/mouse_warping.c:9-16`).
    assert!(crate::command::execute(f.niri_state(), "mouse_warping CONTAINER")[0].success);
    assert_eq!(warping(&mut f), MouseWarping::Container);
    let outcome = crate::command::execute(f.niri_state(), "mouse_warping sideways");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Expected 'mouse_warping output|container|none'")
    );
    assert!(crate::command::execute(f.niri_state(), "mouse_warping none")[0].success);
}

/// Oracle: command-fuzz config-only-workspace-layout,
/// config-only-default-orientation, config-only-orientation,
/// config-only-primary-selection and config-only-xwayland. At run time sway
/// searches `command_handlers` and the shared `handlers`, never
/// `config_handlers` (`sway/sway/commands.c:102-110,156-173`), and
/// `orientation` is in no table. All five are unknown over IPC and change
/// nothing.
#[test]
fn config_only_directives_are_unknown_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let snapshot = |f: &mut Fixture| {
        let config = f.swayward().config.borrow();
        (
            config.layout.workspace_layout,
            config.layout.default_orientation,
            config.clipboard.disable_primary,
            config.xwayland_satellite.off,
        )
    };
    let before = snapshot(&mut f);

    for (command, name) in [
        ("workspace_layout tabbed", "workspace_layout"),
        ("default_orientation vertical", "default_orientation"),
        ("orientation vertical", "orientation"),
        ("primary_selection disabled", "primary_selection"),
        ("xwayland disable", "xwayland"),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            outcome,
            vec![swayward_ipc::command::parse_error(format!(
                "Unknown/invalid command '{name}'"
            ))],
            "{command}"
        );
    }
    assert_eq!(snapshot(&mut f), before);
}

/// A KDL `workspace "name" { layout { gaps N } }` must reach the workspace
/// however it comes to exist. Sway reads the workspace config inside
/// `workspace_create` (`sway/sway/tree/workspace.c:224-243`), so this holds for
/// a workspace created on demand, not only one created eagerly at startup.
///
/// Regression test: workspaces carrying an output assignment skip eager
/// creation (`src/swayward.rs:1549-1552`), and the lazy creation path used to
/// drop the per-name layout entirely.
#[test]
fn configured_workspace_layout_applies_however_the_workspace_is_created() {
    for assignment in ["", "sway-output-assignment \"fake-1\""] {
        let config = swayward_config::Config::parse_mem(&format!(
            r#"
layout {{
    gaps 10
    border {{ off; }}
}}
workspace "roomy" {{
    {assignment}
    layout {{ gaps 45; }}
}}
"#
        ))
        .unwrap();
        let mut fixture = Fixture::with_config(config);
        fixture.add_output(1, (1280, 800));
        assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
        add_two_tiled_windows(&mut fixture);
        assert_eq!(
            tiled_window_rects_on(&mut fixture, "roomy")[0]["x"],
            45,
            "assignment: {assignment:?}"
        );
    }
}

/// Workspace names in a stable order, for rename assertions.
fn workspace_names(fixture: &mut Fixture) -> Vec<String> {
    let swayward = fixture.swayward();
    let mut names = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .map(|workspace| workspace.name)
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Put one window with `app_id` on each named workspace, leaving the last
/// created workspace focused.
fn windows_on_workspaces(fixture: &mut Fixture, plan: &[(&str, &str)]) {
    let client = fixture.add_client();
    for (workspace, app_id) in plan {
        assert!(
            crate::command::execute(fixture.niri_state(), &format!("workspace {workspace}"))[0]
                .success
        );
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id((*app_id).into());
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}


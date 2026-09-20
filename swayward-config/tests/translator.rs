use std::path::PathBuf;
use std::process::Command;

use swayward_config::Config;

#[test]
fn sway_exec_translates_to_loadable_startup_commands_without_weakening_binds() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-exec-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(
        &fixture,
        "exec --no-startup-id sh -c 'printf one,two; printf three'\n\
         exec_always session-start\n\
         bindsym Mod4+x exec fuzzel --show drun\n",
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(translated.contains("spawn-sh-at-startup \"sh -c 'printf one,two; printf three'\""));
    assert!(translated.contains("spawn-sh-at-startup \"session-start\""));
    assert!(translated.contains("command \"exec fuzzel --show drun\""));
    let summary = String::from_utf8(output.stderr).unwrap();
    assert!(summary.starts_with("manual attention: 1 directive(s)"));
    assert!(summary.contains("exec_always reload behavior is not preserved"));
    Config::parse_mem(&translated).unwrap();
}

#[test]
fn sway_inner_gap_units_translate_to_loadable_typed_geometry() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-outer-gaps-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(&fixture, "gaps inner 10px\n").unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(translated.contains("    gaps 10"), "{translated}");
    assert!(!translated.contains("struts"), "{translated}");
    assert!(!translated.contains("px"), "{translated}");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "manual attention: none\n"
    );
    Config::parse_mem(&translated).unwrap();
}

#[test]
fn sway_xkb_numlock_translates_to_loadable_keyboard_state() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (value, enabled) in [
        ("enabled", true),
        ("disabled", false),
        ("yes", true),
        ("no", false),
        ("toggle", true),
        ("garbage", false),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-xkb-numlock-{}-{value}.conf",
            std::process::id()
        ));
        std::fs::write(
            &fixture,
            format!("input type:keyboard {{\n    xkb_numlock {value}\n}}\n"),
        )
        .unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        let translated = String::from_utf8(output.stdout).unwrap();
        assert!(
            translated.contains(&format!("// sway-to-kdl: sway xkb_numlock {value}")),
            "{translated}"
        );
        assert!(
            !translated.contains("xkb {\n            numlock"),
            "{translated}"
        );
        assert_eq!(
            Config::parse_mem(&translated)
                .unwrap()
                .input
                .keyboard
                .numlock,
            enabled,
            "{value}: {translated}"
        );
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "manual attention: none\n"
        );
    }
}

#[test]
fn sway_titlebar_settings_translate_without_silently_dropping_colors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-titlebar-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(
        &fixture,
        "font pango:Fira Sans 11\nclient.focused #111111 #223344 #ffffff #555555 #666666\nclient.focused_inactive #111111 #334455 #eeeeee\nclient.focused_tab_title #111111 #445566 #dddddd\nclient.unfocused #111111 #556677 #cccccc\nclient.urgent #111111 #667788 #bbbbbb\n",
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(translated.contains("font \"Fira Sans 11\""), "{translated}");
    for (state, background, text) in [
        ("focused", "#223344", "#ffffff"),
        ("focused-inactive", "#334455", "#eeeeee"),
        ("focused-tab-title", "#445566", "#dddddd"),
        ("unfocused", "#556677", "#cccccc"),
        ("urgent", "#667788", "#bbbbbb"),
    ] {
        assert!(
            translated.contains(&format!("        {state} {{")),
            "{translated}"
        );
        assert!(
            translated.contains(&format!("background-color \"{background}\"")),
            "{translated}"
        );
        assert!(
            translated.contains(&format!("text-color \"{text}\"")),
            "{translated}"
        );
    }
    let summary = String::from_utf8(output.stderr).unwrap();
    assert_eq!(summary.lines().skip(1).count(), 5, "{summary}");
    assert!(translated.contains("border/indicator/child_border colors have no titlebar equivalent"));
    assert!(
        translated.contains("client.focused-tab-title border colors have no titlebar equivalent")
    );
    Config::parse_mem(&translated).unwrap();
}

#[test]
fn sway_ignored_client_directives_remain_visible_and_loadable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-client-noops-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(
        &fixture,
        "client.background #111111 #222222 #333333 #444444 #555555\nclient.placeholder ignored legacy values\n",
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    for directive in ["client.background", "client.placeholder"] {
        assert!(
            translated.contains(&format!(
                "{directive} is accepted but ignored by sway itself "
            )),
            "{translated}"
        );
        assert!(!translated.contains(&format!("unhandled: {directive}")));
    }
    let summary = String::from_utf8(output.stderr).unwrap();
    assert!(
        summary.starts_with("manual attention: 2 directive(s)"),
        "{summary}"
    );
    Config::parse_mem(&translated).unwrap();
}

#[test]
fn force_focus_wrapping_maps_to_swaywards_focus_wrapping_mode() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (source, expected) in [
        ("force_focus_wrapping true\n", "focus-wrapping \"force\""),
        ("force_focus_wrapping false\n", "focus-wrapping \"yes\""),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-focus-wrapping-{}-{}.conf",
            std::process::id(),
            expected
        ));
        std::fs::write(&fixture, source).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        let translated = String::from_utf8(output.stdout).unwrap();
        assert!(translated.contains(expected), "{translated}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "manual attention: none\n"
        );
        Config::parse_mem(&translated).unwrap();
    }
}

#[test]
fn focus_wrapping_maps_exact_modes_and_refuses_unknown_modes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (directive, value, expected) in [
        ("focus_wrapping", "1", Some("yes")),
        ("FoCuS_WrApPiNg", "YeS", Some("yes")),
        ("focus_wrapping", "ON", Some("yes")),
        ("focus_wrapping", "true", Some("yes")),
        ("focus_wrapping", "Enable", Some("yes")),
        ("focus_wrapping", "ENABLED", Some("yes")),
        ("focus_wrapping", "active", Some("yes")),
        ("focus_wrapping", "FoRcE", Some("force")),
        ("focus_wrapping", "no", Some("no")),
        ("focus_wrapping", "WoRkSpAcE", Some("workspace")),
        ("focus_wrapping", "toggle", None),
        ("focus_wrapping", "false", None),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-modern-focus-wrapping-{}-{value}.conf",
            std::process::id()
        ));
        std::fs::write(&fixture, format!("{directive} {value}\n")).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        let translated = String::from_utf8(output.stdout).unwrap();
        let summary = String::from_utf8(output.stderr).unwrap();
        if let Some(expected) = expected {
            assert!(
                translated.contains(&format!("focus-wrapping \"{expected}\"")),
                "{translated}"
            );
            assert_eq!(summary, "manual attention: none\n");
            Config::parse_mem(&translated).unwrap();
        } else {
            assert!(!translated.contains("    focus-wrapping"), "{translated}");
            assert!(
                translated
                    .contains("unknown focus wrapping mode is refused instead of treated as no"),
                "{translated}"
            );
            assert!(summary.starts_with("manual attention: 1 directive(s)"));
        }
    }
}

#[test]
fn floating_constraints_preserve_values_and_refuse_invalid_forms() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (source, minimum, maximum) in [
        (
            "floating_minimum_size 60 x 40\nfloating_maximum_size 100 x 90\n",
            Some((60, 40)),
            Some((100, 90)),
        ),
        (
            "floating_minimum_size -1 x -1\nfloating_maximum_size 0 x 0\n",
            Some((-1, -1)),
            Some((0, 0)),
        ),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-floating-constraints-{}-{}.conf",
            std::process::id(),
            minimum.unwrap().0
        ));
        std::fs::write(&fixture, source).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "manual attention: none\n"
        );
        let config = Config::parse_mem(&String::from_utf8(output.stdout).unwrap()).unwrap();
        let minimum = minimum.unwrap();
        let maximum = maximum.unwrap();
        assert_eq!(
            (
                config.layout.floating_minimum_size.width,
                config.layout.floating_minimum_size.height
            ),
            minimum
        );
        assert_eq!(
            (
                config.layout.floating_maximum_size.width,
                config.layout.floating_maximum_size.height
            ),
            maximum
        );
    }

    for source in [
        "floating_minimum_size 60 X 40\n",
        "floating_maximum_size -2 x 100\n",
        "floating_minimum_size 60x40\n",
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-invalid-floating-constraints-{}-{}.conf",
            std::process::id(),
            source.len()
        ));
        std::fs::write(&fixture, source).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .starts_with("manual attention: 1 directive(s)"));
    }
}

#[test]
fn default_orientation_maps_all_values_to_loadable_layout() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (value, expected) in [
        (
            "horizontal",
            swayward_config::DefaultOrientation::Horizontal,
        ),
        ("vertical", swayward_config::DefaultOrientation::Vertical),
        ("auto", swayward_config::DefaultOrientation::Auto),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-default-orientation-{}-{value}.conf",
            std::process::id()
        ));
        std::fs::write(&fixture, format!("default_orientation {value}\n")).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        let translated = String::from_utf8(output.stdout).unwrap();
        let config = Config::parse_mem(&translated).unwrap_or_else(|error| {
            panic!("{value} generated invalid KDL: {error:?}\n{translated}")
        });
        assert_eq!(config.layout.default_orientation, expected);
        assert!(translated.contains(&format!("// sway-to-kdl: sway default_orientation {value}")));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "manual attention: none\n"
        );
    }
}

#[test]
fn workspace_layout_maps_sway_values_and_refuses_i3_stacked_spelling() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (value, expected) in [
        ("default", Some("default")),
        ("DeFaUlT", Some("default")),
        ("stacking", Some("stacking")),
        ("StAcKiNg", Some("stacking")),
        ("tabbed", Some("tabbed")),
        ("TaBbEd", Some("tabbed")),
        ("stacked", None),
        ("splitv", None),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-workspace-layout-{}-{value}.conf",
            std::process::id()
        ));
        std::fs::write(&fixture, format!("workspace_layout {value}\n")).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        let translated = String::from_utf8(output.stdout).unwrap();
        let summary = String::from_utf8(output.stderr).unwrap();
        if let Some(expected) = expected {
            assert!(
                translated.contains(&format!("workspace-layout \"{expected}\"")),
                "{translated}"
            );
            assert_eq!(summary, "manual attention: none\n");
            Config::parse_mem(&translated).unwrap();
        } else {
            assert!(!translated.contains("    workspace-layout"), "{translated}");
            assert!(summary.starts_with("manual attention: 1 directive(s)"));
        }
    }
}

#[test]
fn title_criteria_preserve_regex_escapes_and_translate_window_actions() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-title-rules-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(
        &fixture,
        r#"assign [title="^test\w+$"] targetws
for_window [title="^test\w+$"] layout tabbed, focus, move workspace moved
"#,
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(
        translated.contains(r#"match title="^test\\w+$""#),
        "{translated}"
    );
    assert!(
        translated.contains("open-on-workspace \"targetws\""),
        "{translated}"
    );
    assert!(
        translated.contains("default-column-display \"tabbed\""),
        "{translated}"
    );
    assert!(translated.contains("open-focused true"), "{translated}");
    assert!(
        translated.contains("open-on-workspace \"moved\""),
        "{translated}"
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "manual attention: none\n"
    );
    let config = Config::parse_mem(&translated).unwrap();
    assert_eq!(
        config.window_rules[0].matches[0]
            .title
            .as_ref()
            .unwrap()
            .0
            .as_str(),
        r"^test\w+$"
    );
}

#[test]
fn no_focus_maps_portable_criteria_and_refuses_x11_only_criteria() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (criterion, accepted) in [("class", false), ("title", true), ("instance", false)] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-no-focus-{}-{criterion}.conf",
            std::process::id()
        ));
        std::fs::write(&fixture, format!("no_focus [{criterion}=\"value\"]\n")).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        let translated = String::from_utf8(output.stdout).unwrap();
        let summary = String::from_utf8(output.stderr).unwrap();
        if accepted {
            assert!(translated.contains("open-focused false"), "{translated}");
            assert_eq!(summary, "manual attention: none\n");
            Config::parse_mem(&translated).unwrap();
        } else {
            assert!(!translated.contains("window-rule {"), "{translated}");
            assert!(translated.contains("X11-only criterion"), "{translated}");
            assert!(summary.starts_with("manual attention: 1 directive(s)"));
        }
    }
}

#[test]
fn sway_default_border_aliases_translate_to_initial_window_rules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-default-borders-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(&fixture, "new_window 1pixel\nnew_float normal 3\n").unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "manual attention: none\n"
    );
    let config = Config::parse_mem(&String::from_utf8(output.stdout).unwrap()).unwrap();
    let rule = &config.window_rules[0];
    assert_eq!(
        rule.sway_border,
        Some(swayward_ipc::command::BorderStyle::Pixel)
    );
    assert_eq!(rule.sway_border_width, Some(1));
    let rule = &config.window_rules[1];
    assert_eq!(
        rule.sway_floating_border,
        Some(swayward_ipc::command::BorderStyle::Normal)
    );
    assert_eq!(rule.sway_floating_border_width, Some(3));
}

#[test]
fn mouse_warping_maps_exact_modes_and_refuses_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (value, expected) in [
        ("none", None),
        ("NoNe", None),
        (
            "container",
            Some(swayward_config::WarpMouseToFocusMode::CenterXy),
        ),
        (
            "CoNtAiNeR",
            Some(swayward_config::WarpMouseToFocusMode::CenterXy),
        ),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-mouse-warping-{}-{value}.conf",
            std::process::id()
        ));
        std::fs::write(&fixture, format!("mouse_warping {value}\n")).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "manual attention: none\n"
        );
        let config = Config::parse_mem(&String::from_utf8(output.stdout).unwrap()).unwrap();
        assert_eq!(
            config
                .input
                .warp_mouse_to_focus
                .and_then(|warping| warping.mode),
            expected
        );
    }

    let fixture = std::env::temp_dir().join(format!(
        "swayward-mouse-warping-{}-output.conf",
        std::process::id()
    ));
    std::fs::write(&fixture, "mouse_warping OuTpUt\n").unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .starts_with("manual attention: 1 directive(s)"));
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("mouse_warping output has no exact swayward equivalent"));
}

#[test]
fn sway_workspace_output_preserves_ordered_fallback_lists() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-workspace-output-{}-{}.conf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(
        &fixture,
        "workspace 7:web output missing HDMI-A-1\nworkspace chat room output DP-2\n",
    )
    .unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(
        translated.contains(
            "workspace \"7:web\" {\n    sway-output-assignment \"missing\" \"HDMI-A-1\"\n}"
        ),
        "{translated}"
    );
    assert!(
        // A single output now also emits sway-output-assignment. A sway
        // `workspace <name> output <output>` is an ASSIGNMENT consulted by
        // workspace_next_name, whereas open-on-output is niri's directive for a
        // PRE-CREATED workspace; collapsing to the latter put every assigned
        // workspace on the first output at startup.
        translated.contains("workspace \"chat room\" {\n    sway-output-assignment \"DP-2\"\n}"),
        "{translated}"
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "manual attention: none\n"
    );
    let config = Config::parse_mem(&translated).unwrap();
    assert_eq!(
        config.workspaces[0].sway_output_assignment.as_deref(),
        Some(["missing".to_owned(), "HDMI-A-1".to_owned()].as_slice())
    );
}

#[test]
fn swayfx_titlebar_padding_translates_to_loadable_layout() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for (directive, horizontal, vertical) in [
        ("titlebar_padding 6", 6., 6.),
        ("titlebar_padding 6 3", 6., 3.),
    ] {
        let fixture = std::env::temp_dir().join(format!(
            "swayward-titlebar-padding-{}-{}",
            std::process::id(),
            horizontal
        ));
        std::fs::write(&fixture, directive).unwrap();
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(&fixture)
            .output()
            .unwrap();
        std::fs::remove_file(fixture).unwrap();

        assert!(output.status.success());
        let translated = String::from_utf8(output.stdout).unwrap();
        let config = Config::parse_mem(&translated).unwrap_or_else(|error| {
            panic!("{directive} generated invalid KDL: {error:?}\n{translated}")
        });
        assert_eq!(config.layout.titlebar.horizontal_padding, horizontal);
        assert_eq!(config.layout.titlebar.vertical_padding, vertical);
        assert!(translated.contains(&format!("// sway-to-kdl: SwayFX {directive}")));
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            "manual attention: none\n"
        );
    }
}

#[test]
fn upstream_sway_and_swayfx_defaults_translate_to_valid_config() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    for fixture in ["sway-1.11-default.conf", "swayfx-default.conf"] {
        let output = Command::new("python3")
            .arg(root.join("contrib/sway-to-kdl"))
            .arg(root.join("tests/fixtures/config").join(fixture))
            .output()
            .unwrap();
        assert!(output.status.success(), "{fixture}: {output:?}");

        let translated = String::from_utf8(output.stdout).unwrap();
        Config::parse_mem(&translated).unwrap_or_else(|error| {
            panic!("{fixture} generated invalid KDL: {error:?}\n{translated}")
        });

        let summary = String::from_utf8(output.stderr).unwrap();
        assert!(
            summary.starts_with("manual attention:"),
            "{fixture}: {summary}"
        );
        let expected = match fixture {
            "sway-1.11-default.conf" => 3,
            "swayfx-default.conf" => 7,
            _ => unreachable!(),
        };
        assert!(
            summary.starts_with(&format!("manual attention: {expected} directive(s)")),
            "{fixture}: {summary}"
        );
        for item in summary.lines().skip(1) {
            assert!(
                translated.contains(item.trim()),
                "{fixture} silently dropped: {item}"
            );
        }
    }
}

#[test]
fn default_valued_unsupported_directives_are_satisfied_and_loadable() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned();
    let fixture = std::env::temp_dir().join(format!(
        "swayward-default-valued-{}.conf",
        std::process::id()
    ));
    std::fs::write(&fixture, "swaybg_command -\nsmart_gaps off\n").unwrap();
    let output = Command::new("python3")
        .arg(root.join("contrib/sway-to-kdl"))
        .arg(&fixture)
        .output()
        .unwrap();
    std::fs::remove_file(fixture).unwrap();

    assert!(output.status.success());
    let translated = String::from_utf8(output.stdout).unwrap();
    assert!(translated.contains("satisfied by swayward defaults: swaybg_command -"));
    assert!(translated.contains("satisfied by swayward defaults: smart_gaps off"));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "manual attention: none\n"
    );
    Config::parse_mem(&translated).unwrap();
}

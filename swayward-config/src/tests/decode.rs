use std::path::Path;

use pretty_assertions::assert_eq;

use crate::*;

#[test]
fn sway_nodes_decode_non_default_values() {
    let config = Config::parse_mem(
        r#"
            layout {
                default-orientation "vertical"
                workspace-layout "tabbed"
                hide-edge-borders "both"
                smart-borders "no-gaps"
                default-border "pixel" width=3
                default-floating-border "none"
                floating-minimum-size 100 80
                floating-maximum-size 1200 900
            }
            binds { "code:24" { command "nop keycode"; }; }
            window-rule {
                open-on-workspace-number "7"
                sway-border "pixel"
                sway-border-width 4
                sway-for-window-command "nop rule"
            }
            "#,
    )
    .unwrap();

    assert_eq!(
        config.layout.default_orientation,
        DefaultOrientation::Vertical
    );
    assert_eq!(config.layout.workspace_layout, WorkspaceLayout::Tabbed);
    assert_eq!(config.layout.hide_edge_borders, HideEdgeBorders::Both);
    assert_eq!(config.layout.smart_borders, SmartBorders::NoGaps);
    assert_eq!(
        config.layout.default_border,
        SwayBorderDefault {
            style: SwayBorderStyle::Pixel,
            width: Some(3),
        }
    );
    assert_eq!(config.layout.floating_minimum_size.width, 100);
    assert_eq!(config.layout.floating_maximum_size.height, 900);
    assert!(matches!(
        config.binds.0[0].key.trigger,
        Trigger::Keycode(24)
    ));
    let rule = &config.window_rules[0];
    assert_eq!(rule.open_on_workspace_number.as_deref(), Some("7"));
    assert_eq!(rule.sway_border, Some(SwayWindowBorderStyle::Pixel));
    assert_eq!(rule.sway_border_width, Some(4));
    assert_eq!(rule.sway_for_window_commands, ["nop rule"]);
}

#[test]
fn output_scale_rejects_values_outside_the_supported_range() {
    for scale in ["0", "-1", "10.1"] {
        let source = format!("output \"DP-1\" {{ scale {scale}; }}");
        assert!(
            Config::parse_mem(&source).is_err(),
            "accepted scale {scale}"
        );
    }
}

#[test]
fn workspace_parses_ordered_output_fallbacks() {
    let config =
        Config::parse_mem("workspace \"web\" { sway-output-assignment \"missing\" \"HDMI-A-1\"; }")
            .unwrap();

    assert_eq!(
        config.workspaces[0].sway_output_assignment,
        Some(vec!["missing".to_owned(), "HDMI-A-1".to_owned()])
    );
}

#[test]
fn workspace_rejects_ambiguous_or_empty_output_assignments() {
    for source in [
        "workspace \"web\" { sway-output-assignment; }",
        "workspace \"web\" { sway-output-assignment \"DP-1\"; open-on-output \"DP-2\"; }",
    ] {
        assert!(Config::parse_mem(source).is_err(), "accepted {source}");
    }
}

#[test]
fn urgent_timeout_defaults_to_sway_value() {
    assert_eq!(Config::parse_mem("").unwrap().urgent_timeout_ms, 500);
}

#[test]
fn urgent_timeout_parses_milliseconds() {
    let config = Config::parse(Path::new("test.kdl"), "urgent-timeout-ms 500")
        .config
        .unwrap();
    assert_eq!(config.urgent_timeout_ms, 500);
}

#[test]
fn sway_runtime_settings_decode_from_kdl() {
    let config = Config::parse_mem(
        r#"
            layout {
                smart-gaps "inverse-outer"
                titlebar {
                    show-marks false
                    alignment "right"
                }
            }
            input {
                tiling-drag false
                tiling-drag-threshold 17
                border-resize false
                gap-resize
            }
            "#,
    )
    .unwrap();

    assert_eq!(config.layout.smart_gaps, SmartGaps::InverseOuter);
    assert!(!config.layout.titlebar.show_marks);
    assert_eq!(config.layout.titlebar.alignment, TitleAlignment::Right);
    assert!(!config.input.tiling_drag);
    assert_eq!(config.input.tiling_drag_threshold, 17);
    assert!(!config.input.border_resize);
    assert!(config.input.gap_resize);
}

#[test]
fn retired_scrolling_layout_settings_are_rejected() {
    for source in [
        r#"layout { center-focused-column "always"; }"#,
        "layout { always-center-single-column; }",
        r#"layout { default-column-display "tabbed"; }"#,
    ] {
        let error = Config::parse_mem(source).unwrap_err();
        assert!(
            format!("{error:?}").contains("retired with the scrolling layout engine"),
            "wrong error for {source}: {error:?}"
        );
    }
}

#[test]
fn retired_horizontal_view_movement_animation_is_rejected() {
    let error = Config::parse_mem("animations { horizontal-view-movement { duration-ms 100; }; }")
        .unwrap_err();
    assert!(
        format!("{error:?}")
            .contains("horizontal-view-movement was retired with the scrolling layout engine"),
        "{error:?}"
    );
}

#[test]
fn popup_during_fullscreen_parses_all_modes_and_rejects_invalid_input() {
    for (value, expected) in [
        ("smart", PopupDuringFullscreen::Smart),
        ("IGNORE", PopupDuringFullscreen::Ignore),
        ("Leave_Fullscreen", PopupDuringFullscreen::LeaveFullscreen),
    ] {
        let config = Config::parse_mem(&format!("popup-during-fullscreen \"{value}\"")).unwrap();
        assert_eq!(config.popup_during_fullscreen, expected);
    }

    for source in [
        "popup-during-fullscreen",
        "popup-during-fullscreen \"smart\" \"ignore\"",
        "popup-during-fullscreen \"all\"",
    ] {
        let error = Config::parse_mem(source).unwrap_err();
        assert!(format!("{error:?}")
            .contains("Expected 'popup_during_fullscreen smart|ignore|leave_fullscreen'"));
    }
}

#[test]
fn can_create_default_config() {
    let _ = Config::load_default();
}

#[test]
fn default_config_enables_the_quiet_effect_set() {
    let config = Config::load_default();
    assert!(config.layout.focus_ring.off);
    assert!(!config.layout.border.off);
    assert!(config.layout.shadow.on);
    assert!(config.layout.draw_uncovered_top_border);
    assert!(config.debug.deactivate_unfocused_windows);
    assert!(config
        .window_rules
        .iter()
        .all(|rule| rule.background_effect.blur != Some(true)));

    let default_rule = config
        .window_rules
        .iter()
        .find(|rule| rule.matches.is_empty() && rule.excludes.is_empty())
        .unwrap();
    assert_eq!(default_rule.geometry_corner_radius, Some(12_f32.into()));
    assert_eq!(default_rule.clip_to_geometry, Some(true));
}

#[test]
fn default_config_disables_hot_corners_and_recent_windows() {
    let config = Config::load_default();
    assert!(config.gestures.hot_corners.off);
    assert!(!config.recent_windows.on);
}

#[test]
fn uncovered_top_border_can_be_disabled() {
    let config = Config::parse_mem("layout { draw-uncovered-top-border false; }").unwrap();
    assert!(!config.layout.draw_uncovered_top_border);
}

#[test]
fn default_config_exposes_core_tree_commands() {
    let config = Config::load_default();
    let command_for = |key: &str| {
        let key = key.parse::<Key>().unwrap();
        config.binds.0.iter().find_map(|bind| {
            (bind.key == key).then_some(match &bind.action {
                Action::SwayCommand(command) => command.as_str(),
                _ => "<typed action>",
            })
        })
    };

    for (key, command) in [
        ("Mod+H", "focus left"),
        ("Mod+Left", "focus left"),
        ("Mod+Shift+H", "move left"),
        ("Mod+Shift+Left", "move left"),
        ("Mod+B", "split h"),
        ("Mod+V", "split v"),
        ("Mod+W", "layout tabbed"),
        ("Mod+S", "layout stacking"),
        ("Mod+E", "layout toggle split"),
        ("Mod+A", "focus parent"),
        ("Mod+Ctrl+A", "focus child"),
        ("Mod+F", "fullscreen"),
        ("Mod+Shift+Space", "floating toggle"),
        ("Mod+Shift+Minus", "move scratchpad"),
        ("Mod+Minus", "scratchpad show"),
        ("Mod+R", "mode resize"),
    ] {
        assert_eq!(command_for(key), Some(command), "default bind {key}");
    }
    assert!(config
        .binding_modes
        .iter()
        .any(|mode| mode.name == "resize"));
}

#[test]
fn empty_mode_name_is_rejected() {
    let error = Config::parse_mem(r#"mode "" { x { command "nop"; }; }"#).unwrap_err();
    assert!(format!("{error:?}").contains("mode name must not be empty"));
}

#[test]
fn repeated_mode_blocks_merge_and_default_extends_top_level_binds() {
    let config = Config::parse_mem(
        r#"binds { x { command "nop top"; }; }
            mode "resize" { Left { command "nop left"; }; }
            mode "resize" { Right { command "nop right"; }; }
            mode "default" { y { command "nop default"; }; }"#,
    )
    .unwrap();

    assert_eq!(config.binding_modes.len(), 1);
    assert_eq!(config.binding_modes[0].name, "resize");
    assert_eq!(config.binding_modes[0].binds.0.len(), 2);
    assert_eq!(config.binds.0.len(), 2);
}

#[test]
fn binding_mode_parses_command_binds() {
    let config = Config::parse_mem(
        r#"mode "resize" pango-markup=true {
                Left { command "resize shrink width 10 px"; }
                Escape { command "mode default"; }
            }"#,
    )
    .unwrap();

    assert_eq!(config.binding_modes.len(), 1);
    assert_eq!(config.binding_modes[0].name, "resize");
    assert!(config.binding_modes[0].pango_markup);
    assert_eq!(config.binding_modes[0].binds.0.len(), 2);
    assert_eq!(
        config.binding_modes[0].binds.0[1].action,
        Action::SwayCommand("mode default".into())
    );
}

#[test]
fn device_specific_and_wildcard_binds_can_share_a_trigger() {
    let config = Config::parse_mem(
        r#"binds {
                x { command "nop wildcard"; }
                x input-device="123:456:keyboard with spaces" { command "nop exact"; }
            }"#,
    )
    .unwrap();

    assert_eq!(config.binds.0.len(), 2);
    assert_eq!(config.binds.0[0].input_device, "*");
    assert_eq!(
        config.binds.0[1].input_device,
        "123:456:keyboard with spaces"
    );
}

#[test]
fn grouped_and_group_agnostic_binds_can_share_a_trigger() {
    let config = Config::parse_mem(
        r#"binds {
                Q { command "nop wildcard"; }
                Group2+Q { command "nop group-2"; }
                Mode_switch+W { command "nop alias"; }
            }"#,
    )
    .unwrap();

    assert_eq!(config.binds.0.len(), 3);
    assert_eq!(config.binds.0[0].group, None);
    assert_eq!(config.binds.0[1].group, Some(1));
    assert_eq!(config.binds.0[2].group, Some(1));
}

#[test]
fn group_parser_rejects_invalid_and_duplicate_groups() {
    for key in ["Group0+Q", "Group5+Q", "Group2+Group3+Q"] {
        let error = Config::parse_mem(&format!(
            "binds {{ {key} {{ command \"nop invalid\"; }}; }}"
        ))
        .unwrap_err();
        assert!(format!("{error:?}").contains("Group1 to Group4"), "{key}");
    }
}

#[test]
fn press_and_release_binds_can_share_a_trigger() {
    let config = Config::parse_mem(
        r#"binds {
                Print { command "nop press"; }
                Print release=true repeat=false { command "nop release"; }
            }"#,
    )
    .unwrap();

    assert_eq!(config.binds.0.len(), 2);
    assert!(!config.binds.0[0].release);
    assert!(config.binds.0[1].release);
    assert!(!config.binds.0[1].repeat);
}

#[test]
fn mouse_region_binds_can_share_a_trigger() {
    let config = Config::parse_mem(
        r#"binds {
                MouseLeft mouse-regions="titlebar" { command "nop titlebar"; }
                MouseLeft mouse-regions="border" { command "nop border"; }
            }"#,
    )
    .unwrap();
    assert_eq!(config.binds.0.len(), 2);
}

#[test]
fn bind_properties_must_match_the_trigger_kind() {
    for config in [
        r#"binds { Mod+Q mouse-regions="titlebar" { command "nop"; }; }"#,
        r#"binds { Group2+MouseLeft { command "nop"; }; }"#,
    ] {
        assert!(Config::parse_mem(config).is_err(), "{config}");
    }
}

#[test]
fn mouse_bind_regions_parse() {
    let config = Config::parse_mem(
        r#"binds { MouseLeft mouse-regions="titlebar+border+contents" { command "focus"; }; }"#,
    )
    .unwrap();
    assert_eq!(
        config.binds.0[0].mouse_regions,
        crate::binds::MouseRegions::TITLEBAR
            | crate::binds::MouseRegions::BORDER
            | crate::binds::MouseRegions::CONTENTS
    );

    let error =
        Config::parse_mem(r#"binds { MouseLeft mouse-regions="workspace" { command "focus"; }; }"#)
            .unwrap_err();
    assert!(format!("{error:?}").contains("mouse-regions must contain"));
}

#[test]
fn command_binds_reject_empty_commands_and_input_devices() {
    for config in [
        r#"binds { Mod+Q { command "   "; }; }"#,
        r#"binds { Mod+Q input-device="" { command "nop"; }; }"#,
    ] {
        assert!(Config::parse_mem(config).is_err(), "{config}");
    }
}

#[test]
fn sway_command_bind_stores_commands_for_execution_time_validation() {
    let config = Config::parse_mem(
            "binds { Mod+H repeat=false cooldown-ms=150 allow-when-locked=true hotkey-overlay-title=\"Left\" { command \"focus left\"; }; }",
        )
        .unwrap();
    let bind = &config.binds.0[0];
    assert_eq!(bind.action, Action::SwayCommand("focus left".into()));
    assert!(!bind.repeat);
    assert_eq!(bind.cooldown, Some(std::time::Duration::from_millis(150)));
    assert!(bind.allow_when_locked);
    assert_eq!(bind.hotkey_overlay_title, Some(Some("Left".into())));

    let config = Config::parse_mem("binds { Mod+H { command \"frobnicate\"; }; }").unwrap();
    assert_eq!(
        config.binds.0[0].action,
        Action::SwayCommand("frobnicate".into())
    );
}

#[test]
fn default_repeat_params() {
    let config = Config::parse_mem("").unwrap();
    assert_eq!(config.input.keyboard.repeat_delay, 600);
    assert_eq!(config.input.keyboard.repeat_rate, 25);
}

#[track_caller]
fn do_parse(text: &str) -> Config {
    Config::parse_mem(text)
        .map_err(miette::Report::new)
        .unwrap()
}

#[test]
fn window_rule_rejects_toggle_border() {
    for field in ["sway-border", "sway-floating-border"] {
        let error =
            Config::parse_mem(&format!(r#"window-rule {{ {field} "toggle"; }}"#)).unwrap_err();
        assert!(format!("{error:?}").contains("unknown border style `toggle`"));
    }
}

#[test]
fn parse_on_xdg_activate() {
    let parsed = do_parse(
        r#"
            window-rule { on-xdg-activate "ignore"; }
            window-rule { on-xdg-activate "set-urgent"; }
            window-rule { on-xdg-activate "focus"; }
            "#,
    );

    assert_eq!(
        parsed
            .window_rules
            .iter()
            .map(|rule| rule.on_xdg_activate)
            .collect::<Vec<_>>(),
        vec![
            Some(OnXdgActivate::Ignore),
            Some(OnXdgActivate::SetUrgent),
            Some(OnXdgActivate::Focus),
        ]
    );
}

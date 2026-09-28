mod border;
mod color;
mod effects;
mod indicators;
mod titlebar;

pub use border::*;
pub use color::*;
pub use effects::*;
pub use indicators::*;
pub use titlebar::*;

#[cfg(test)]
mod tests {
    use insta::{assert_debug_snapshot, assert_snapshot};

    use super::*;
    use crate::utils::MergeWith;
    use crate::Config;

    #[test]
    fn titlebar_defaults_to_sway_colors() {
        let titlebar = Titlebar::default();

        assert_eq!(
            titlebar.focused.background_color,
            Color::from_rgba8_unpremul(0x28, 0x55, 0x77, 0xff)
        );
        assert_eq!(
            titlebar.focused_inactive.background_color,
            Color::from_rgba8_unpremul(0x5f, 0x67, 0x6a, 0xff)
        );
        assert_eq!(
            titlebar.unfocused.text_color,
            Color::from_rgba8_unpremul(0x88, 0x88, 0x88, 0xff)
        );
        assert_eq!(
            titlebar.urgent.background_color,
            Color::from_rgba8_unpremul(0x90, 0, 0, 0xff)
        );
    }

    #[test]
    fn titlebar_rejects_padding_smaller_than_border() {
        let error = Config::parse_mem(
            r#"layout { titlebar { horizontal-padding 3; border-thickness 4; }; }"#,
        )
        .unwrap_err();

        assert!(format!("{error:?}").contains("titlebar padding cannot be smaller"));
    }

    #[test]
    fn titlebar_parses_pango_markup() {
        let config = Config::parse_mem(
            r#"layout { titlebar { font "monospace 10"; pango-markup true; }; }"#,
        )
        .unwrap();

        assert!(config.layout.titlebar.pango_markup);
    }

    #[test]
    fn parse_gradient_interpolation() {
        assert_eq!(
            "srgb".parse::<GradientInterpolation>().unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::Srgb,
                ..Default::default()
            }
        );
        assert_eq!(
            "srgb-linear".parse::<GradientInterpolation>().unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::SrgbLinear,
                ..Default::default()
            }
        );
        assert_eq!(
            "oklab".parse::<GradientInterpolation>().unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::Oklab,
                ..Default::default()
            }
        );
        assert_eq!(
            "oklch".parse::<GradientInterpolation>().unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::Oklch,
                ..Default::default()
            }
        );
        assert_eq!(
            "oklch shorter hue"
                .parse::<GradientInterpolation>()
                .unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::Oklch,
                hue_interpolation: HueInterpolation::Shorter,
            }
        );
        assert_eq!(
            "oklch longer hue".parse::<GradientInterpolation>().unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::Oklch,
                hue_interpolation: HueInterpolation::Longer,
            }
        );
        assert_eq!(
            "oklch decreasing hue"
                .parse::<GradientInterpolation>()
                .unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::Oklch,
                hue_interpolation: HueInterpolation::Decreasing,
            }
        );
        assert_eq!(
            "oklch increasing hue"
                .parse::<GradientInterpolation>()
                .unwrap(),
            GradientInterpolation {
                color_space: GradientColorSpace::Oklch,
                hue_interpolation: HueInterpolation::Increasing,
            }
        );

        assert!("".parse::<GradientInterpolation>().is_err());
        assert!("srgb shorter hue".parse::<GradientInterpolation>().is_err());
        assert!("oklch shorter".parse::<GradientInterpolation>().is_err());
        assert!("oklch shorter h".parse::<GradientInterpolation>().is_err());
        assert!("oklch a hue".parse::<GradientInterpolation>().is_err());
        assert!("oklch shorter hue a"
            .parse::<GradientInterpolation>()
            .is_err());
    }

    #[test]
    fn test_border_rule_on_off_merging() {
        fn is_on(config: &str, rules: &[&str]) -> String {
            let mut resolved = Border {
                off: config == "off",
                ..Default::default()
            };

            for rule in rules.iter().copied() {
                let rule = BorderRule {
                    off: rule == "off" || rule == "off,on",
                    on: rule == "on" || rule == "off,on",
                    ..Default::default()
                };

                resolved.merge_with(&rule);
            }

            if resolved.off { "off" } else { "on" }.to_owned()
        }

        assert_snapshot!(is_on("off", &[]), @"off");
        assert_snapshot!(is_on("off", &["off"]), @"off");
        assert_snapshot!(is_on("off", &["on"]), @"on");
        assert_snapshot!(is_on("off", &["off,on"]), @"on");

        assert_snapshot!(is_on("on", &[]), @"on");
        assert_snapshot!(is_on("on", &["off"]), @"off");
        assert_snapshot!(is_on("on", &["on"]), @"on");
        assert_snapshot!(is_on("on", &["off,on"]), @"on");

        assert_snapshot!(is_on("off", &["off", "off"]), @"off");
        assert_snapshot!(is_on("off", &["off", "on"]), @"on");
        assert_snapshot!(is_on("off", &["on", "off"]), @"off");
        assert_snapshot!(is_on("off", &["on", "on"]), @"on");

        assert_snapshot!(is_on("on", &["off", "off"]), @"off");
        assert_snapshot!(is_on("on", &["off", "on"]), @"on");
        assert_snapshot!(is_on("on", &["on", "off"]), @"off");
        assert_snapshot!(is_on("on", &["on", "on"]), @"on");
    }

    #[test]
    fn rule_color_can_override_base_gradient() {
        let config = Config::parse_mem(
            r##"
            // Start with gradient set.
            layout {
                border {
                    active-gradient from="#101010" to="#202020"
                    inactive-gradient from="#111111" to="#212121"
                    urgent-gradient from="#121212" to="#222222"
                }
            }

            // Override with color.
            window-rule {
                border {
                    active-color "#abcdef"
                    inactive-color "#123456"
                    urgent-color "#fedcba"
                }
            }
            "##,
        )
        .unwrap();

        let mut border = config.layout.border;
        for rule in &config.window_rules {
            border.merge_with(&rule.border);
        }

        // Gradient should be None because it's overwritten.
        assert_debug_snapshot!(
            (
                border.active_gradient.is_some(),
                border.inactive_gradient.is_some(),
                border.urgent_gradient.is_some(),
            ),
            @r"
        (
            false,
            false,
            false,
        )
        "
        );
    }

    #[test]
    fn rule_color_can_override_rule_gradient() {
        let config = Config::parse_mem(
            r##"
            // Start with gradient set.
            layout {
                border {
                    active-gradient from="#101010" to="#202020"
                    inactive-gradient from="#111111" to="#212121"
                    urgent-gradient from="#121212" to="#222222"
                }
            }

            // Window rule with gradients set.
            window-rule {
                border {
                    active-gradient from="#303030" to="#404040"
                    inactive-gradient from="#313131" to="#414141"
                    urgent-gradient from="#323232" to="#424242"
                }

                tab-indicator {
                    active-gradient from="#505050" to="#606060"
                    inactive-gradient from="#515151" to="#616161"
                    urgent-gradient from="#525252" to="#626262"
                }
            }

            // Override with color.
            window-rule {
                border {
                    active-color "#abcdef"
                    inactive-color "#123456"
                    urgent-color "#fedcba"
                }

                tab-indicator {
                    active-color "#abcdef"
                    inactive-color "#123456"
                    urgent-color "#fedcba"
                }
            }
            "##,
        )
        .unwrap();

        let mut border = config.layout.border;
        let mut tab_indicator_rule = TabIndicatorRule::default();
        for rule in &config.window_rules {
            border.merge_with(&rule.border);
            tab_indicator_rule.merge_with(&rule.tab_indicator);
        }

        // Gradient should be None because it's overwritten.
        assert_debug_snapshot!(
            (
                border.active_gradient.is_some(),
                border.inactive_gradient.is_some(),
                border.urgent_gradient.is_some(),
                tab_indicator_rule.active_gradient.is_some(),
                tab_indicator_rule.inactive_gradient.is_some(),
                tab_indicator_rule.urgent_gradient.is_some(),
            ),
            @r"
        (
            false,
            false,
            false,
            false,
            false,
            false,
        )
        "
        );
    }
}

use super::*;

pub(super) fn parse_client_colors(name: &str, args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err(format!(
            "Invalid {name} command (expected at least 3 arguments, got {})",
            args.len()
        ));
    }
    if args.len() > 5 {
        return Err(format!(
            "Invalid {name} command (expected at most 5 arguments, got {})",
            args.len()
        ));
    }

    let default_indicator = match name {
        "client.focused" | "client.focused_tab_title" => "#2e9ef4ff",
        "client.focused_inactive" => "#484e50ff",
        "client.unfocused" => "#292d2eff",
        "client.urgent" => "#900000ff",
        _ => return Err(format!("Unknown/invalid command '{name}'")),
    };
    let [border, background, text, rest @ ..] = args else {
        return Err(format!(
            "Invalid {name} command (expected at least 3 arguments, got {})",
            args.len()
        ));
    };
    let properties = [
        ("border", *border),
        ("background", *background),
        ("text", *text),
        (
            "indicator",
            rest.first().copied().unwrap_or(default_indicator),
        ),
        ("child_border", rest.get(1).copied().unwrap_or(background)),
    ];
    let mut parsed = [[0; 4]; 5];
    for (slot, (property, value)) in parsed.iter_mut().zip(properties) {
        *slot =
            parse_sway_color(value).ok_or_else(|| format!("Invalid {property} color {value}"))?;
    }
    let [border, background, text, ..] = parsed;

    if name != "client.focused_tab_title" {
        return Err(
            "client colour commands are unsupported because sway window-border colours are not fully rendered"
                .into(),
        );
    }

    Ok(Command::SetClientColors {
        class: ClientColorClass::FocusedTabTitle,
        colors: ClientColors {
            border,
            background,
            text,
        },
    })
}

pub(super) fn parse_sway_color(value: &str) -> Option<[u8; 4]> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if !matches!(value.len(), 6 | 8) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let parsed = u32::from_str_radix(value, 16).ok()?;
    let rgba = if value.len() == 6 {
        (parsed << 8) | 0xff
    } else {
        parsed
    };
    Some(rgba.to_be_bytes())
}

/// Sway's error text for the two-argument defaults form
/// (`sway/sway/commands/gaps.c:46-47`).
pub(super) const GAPS_EXPECTED_DEFAULTS: &str =
    "'gaps inner|outer|horizontal|vertical|top|right|bottom|left <px>'";
/// Sway's error text for the four-argument runtime form
/// (`sway/sway/commands/gaps.c:134-136`).
pub(super) const GAPS_EXPECTED_RUNTIME: &str =
    "'gaps inner|outer|horizontal|vertical|top|right|bottom|left \
     current|all set|plus|minus|toggle <px>'";

/// Sides are ordered `[left, right, top, bottom]` to match
/// [`swayward_config::OuterGaps`]. Sway sets each side independently, so
/// `horizontal` and `vertical` select pairs rather than a distinct kind
/// (`sway/sway/commands/gaps.c:62-84`).
pub(super) fn parse_gaps_kind(kind: &str) -> Option<(bool, [bool; 4])> {
    Some(match kind.to_ascii_lowercase().as_str() {
        "inner" => (true, [false; 4]),
        "outer" => (false, [true; 4]),
        "horizontal" => (false, [true, true, false, false]),
        "vertical" => (false, [false, false, true, true]),
        "left" => (false, [true, false, false, false]),
        "right" => (false, [false, true, false, false]),
        "top" => (false, [false, false, true, false]),
        "bottom" => (false, [false, false, false, true]),
        _ => return None,
    })
}

/// C's `atoi`: optional leading whitespace and sign, then as many decimal
/// digits as follow; anything else ends the number, and no digits give 0.
/// Out-of-range values saturate, where C's behaviour is undefined.
pub(super) fn atoi(raw: &str) -> i32 {
    let raw = raw.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match (raw.strip_prefix('-'), raw.strip_prefix('+')) {
        (Some(digits), _) => (true, digits),
        (None, Some(digits)) => (false, digits),
        (None, None) => (false, raw),
    };
    let magnitude = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |value, digit| {
            (value * 10 + i64::from(digit - b'0')).min(i64::from(i32::MAX) + 1)
        });
    let value = if negative { -magnitude } else { magnitude };
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Sway parses with `strtol` and accepts a bare number or a `px` suffix,
/// rejecting any other trailing text (`sway/sway/commands/gaps.c:55-58`).
pub(super) fn parse_gaps_amount(raw: &str) -> Option<i32> {
    let digits = raw
        .strip_suffix("px")
        .or_else(|| raw.strip_suffix("PX"))
        .or_else(|| raw.strip_suffix("Px"))
        .or_else(|| raw.strip_suffix("pX"))
        .unwrap_or(raw);
    digits.parse::<i64>().ok().map(|amount| amount as i32)
}

pub(super) fn parse_gaps(args: &[&str]) -> Result<Command, String> {
    // Sway dispatches on argument count, and rejects anything that is neither
    // shape with both expectations named (`sway/sway/commands/gaps.c:205-223`).
    match args {
        [kind, raw_amount] => {
            let Some((inner, sides)) = parse_gaps_kind(kind) else {
                return Err(format!("Expected {GAPS_EXPECTED_DEFAULTS}"));
            };
            let Some(amount) = parse_gaps_amount(raw_amount) else {
                return Err(format!("Expected {GAPS_EXPECTED_DEFAULTS}"));
            };
            Ok(Command::GapsDefaults {
                inner,
                sides,
                amount,
            })
        }
        [kind, scope, operation, raw_amount] => {
            let Some((inner, sides)) = parse_gaps_kind(kind) else {
                return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}"));
            };
            let all = match scope.to_ascii_lowercase().as_str() {
                "all" => true,
                "current" => false,
                _ => return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}")),
            };
            let operation = match operation.to_ascii_lowercase().as_str() {
                "set" => GapOperation::Set,
                "plus" => GapOperation::Plus,
                "minus" => GapOperation::Minus,
                "toggle" => GapOperation::Toggle,
                _ => return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}")),
            };
            let Some(amount) = parse_gaps_amount(raw_amount) else {
                return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}"));
            };
            Ok(Command::Gaps {
                inner,
                sides,
                all,
                operation,
                amount,
            })
        }
        args if args.len() < 2 => Err(format!(
            "Invalid gaps command (expected at least 2 arguments, got {})",
            args.len()
        )),
        _ => Err(format!(
            "Expected {GAPS_EXPECTED_RUNTIME} or {GAPS_EXPECTED_DEFAULTS}"
        )),
    }
}

pub(super) fn parse(name: &str, rest: &[&str]) -> Result<Command, String> {
    match name {
        name @ ("client.focused"
        | "client.focused_inactive"
        | "client.focused_tab_title"
        | "client.unfocused"
        | "client.urgent") => parse_client_colors(name, rest),
        "focus_wrapping" => match rest {
            // `sway/sway/commands/focus_wrapping.c`: force and workspace are
            // literal, everything else goes through parse_boolean.
            [value] => {
                let value = value.to_ascii_lowercase();
                let mapped = match value.as_str() {
                    "force" => "force",
                    "workspace" => "workspace",
                    "toggle" => "toggle",
                    other => {
                        if parse_boolean(other, false) {
                            "yes"
                        } else {
                            "no"
                        }
                    }
                };
                Ok(Command::SetLayoutOption(LayoutOption::FocusWrapping(
                    mapped.to_owned(),
                )))
            }
            _ => Err("Expected 'focus_wrapping yes|no|force|workspace'".into()),
        },
        "force_focus_wrapping" => match rest {
            // Deprecated in sway, which keeps it as a boolean alias selecting
            // between force and yes (`sway/sway/commands/force_focus_wrapping.c`).
            [value] => Ok(Command::SetLayoutOption(LayoutOption::ForceFocusWrapping(
                value.to_ascii_lowercase(),
            ))),
            _ => Err("Expected 'force_focus_wrapping <yes|no>'".into()),
        },
        "hide_edge_borders" => {
            // `sway/sway/commands/hide_edge_borders.c:7-45`: an optional --i3
            // only as argv[0], then a strcmp-matched value; later arguments
            // are ignored. --i3 enables hide_lone_tab, which swayward's
            // titlebar model cannot express, so it fails loudly rather than
            // being accepted and dropped (docs/KNOWN_DEVIATIONS.md).
            let usage = "Expected 'hide_edge_borders [--i3] \
                         none|vertical|horizontal|both|smart|smart_no_gaps";
            let (lone_tab, rest) = match rest {
                ["--i3", rest @ ..] => (true, rest),
                rest => (false, rest),
            };
            let option = match rest.first().copied() {
                Some(value @ ("none" | "vertical" | "horizontal" | "both")) => {
                    LayoutOption::HideEdgeBorders(value.to_owned())
                }
                Some("smart") => LayoutOption::HideEdgeBordersSmart("on".to_owned()),
                Some("smart_no_gaps") => LayoutOption::HideEdgeBordersSmart("no-gaps".to_owned()),
                _ => return Err(usage.into()),
            };
            if lone_tab {
                return Err(
                    "hide_edge_borders --i3 is unsupported because swayward cannot hide a lone tab's title bar"
                        .into(),
                );
            }
            Ok(Command::SetLayoutOption(option))
        }
        "smart_borders" => match rest {
            [value] => {
                let value = value.to_ascii_lowercase();
                // sway writes no_gaps; the KDL spelling is no-gaps, and the
                // config crate's FromStr is the single validator.
                let mapped = if value == "no_gaps" || value == "no-gaps" {
                    "no-gaps"
                } else if parse_boolean(&value, true) {
                    "on"
                } else {
                    "off"
                };
                Ok(Command::SetLayoutOption(LayoutOption::SmartBorders(
                    mapped.to_owned(),
                )))
            }
            _ => Err("Expected 'smart_borders on|no_gaps|off'".into()),
        },
        "smart_gaps" => match rest {
            [value] => {
                let value = value.to_ascii_lowercase();
                let mapped = match value.as_str() {
                    "inverse_outer" => "inverse-outer",
                    "toggle" => "toggle",
                    other if parse_boolean(other, true) => "on",
                    _ => "off",
                };
                Ok(Command::SetLayoutOption(LayoutOption::SmartGaps(
                    mapped.into(),
                )))
            }
            _ => Err("Expected 'smart_gaps on|off|toggle|inverse_outer'".into()),
        },
        "show_marks" => match rest.split_first() {
            Some((value, _)) => Ok(Command::SetLayoutOption(LayoutOption::ShowMarks(
                value.to_ascii_lowercase(),
            ))),
            None => Err("Expected 'show_marks yes|no'".into()),
        },
        "title_align" => match rest {
            [value] if matches!(*value, "left" | "center" | "right") => Ok(
                Command::SetLayoutOption(LayoutOption::TitleAlignment((*value).into())),
            ),
            _ => Err("Expected 'title_align left|center|right'".into()),
        },
        "tiling_drag" => match rest {
            [value] => Ok(Command::SetLayoutOption(LayoutOption::TilingDrag(
                value.to_ascii_lowercase(),
            ))),
            _ => Err("Expected 'tiling_drag enable|disable|toggle'".into()),
        },
        "tiling_drag_threshold" => match rest {
            [value] => value
                .parse()
                .map(LayoutOption::TilingDragThreshold)
                .map(Command::SetLayoutOption)
                .map_err(|_| "Invalid threshold specified".into()),
            _ => Err("Expected 'tiling_drag_threshold <threshold>'".into()),
        },
        "force_display_urgency_hint" => {
            // `sway/sway/commands/force_display_urgency_hint.c:12-23`: strtol
            // with nothing after the number but an optional "ms", then an
            // optional argv[1] that must be "ms"; later arguments are
            // ignored. strtol reads a long that sway casts to int, and a
            // negative timeout is stored as 0.
            let [value, rest @ ..] = rest else {
                return Err("Expected 'force_display_urgency_hint <timeout> [ms]'".into());
            };
            let value = value
                .strip_suffix("ms")
                .unwrap_or(value)
                .parse::<i64>()
                .map_err(|_| "timeout integer invalid".to_owned())? as i32;
            if rest.first().is_some_and(|unit| *unit != "ms") {
                return Err("Expected 'force_display_urgency_hint <timeout> [ms]'".into());
            }
            Ok(Command::SetLayoutOption(
                LayoutOption::ForceDisplayUrgencyHint(value.max(0) as u32),
            ))
        }
        "focus_on_window_activation" => match rest {
            [value] if matches!(*value, "smart" | "urgent" | "focus" | "none") => Ok(
                Command::SetLayoutOption(LayoutOption::FocusOnWindowActivation((*value).into())),
            ),
            _ => Err("Expected 'focus_on_window_activation smart|urgent|focus|none'".into()),
        },
        "focus_follows_mouse" => match rest {
            // `sway/sway/commands/focus_follows_mouse.c:9-18` compares with
            // strcmp, so the three names are case-sensitive, and it rejects
            // anything else rather than coercing.
            ["no"] => Ok(Command::SetLayoutOption(LayoutOption::FocusFollowsMouse(
                FocusFollowsMouse::No,
            ))),
            ["yes"] => Ok(Command::SetLayoutOption(LayoutOption::FocusFollowsMouse(
                FocusFollowsMouse::Yes,
            ))),
            ["always"] => Ok(Command::SetLayoutOption(LayoutOption::FocusFollowsMouse(
                FocusFollowsMouse::Always,
            ))),
            _ => Err("Expected 'focus_follows_mouse no|yes|always'".into()),
        },
        "workspace_auto_back_and_forth" => match rest {
            [value] => Ok(Command::SetLayoutOption(
                LayoutOption::WorkspaceAutoBackAndForth(value.to_ascii_lowercase()),
            )),
            _ => Err("Expected 'workspace_auto_back_and_forth <yes|no>'".into()),
        },
        name @ ("default_border" | "default_floating_border" | "new_window" | "new_float") => {
            // `sway/sway/commands/default_border.c`: a style, then an
            // optional width that sway reads with atoi and only for `pixel`
            // and `normal`. new_window and new_float are the older i3
            // spellings of the same two settings.
            let canonical = match name {
                "new_window" => "default_border",
                "new_float" => "default_floating_border",
                _ => name,
            };
            let usage = format!(
                "Expected '{canonical} <none|normal|pixel>' or '{canonical} <normal|pixel> <px>'"
            );
            // Sway accepts at least one argument and reads the width only when
            // there are exactly two (`EXPECTED_AT_LEAST, 1`, `argc == 2`). The
            // style is matched with strcmp, so case matters.
            let (style, width) = match rest {
                [style, width] => (*style, Some(*width)),
                [style, ..] => (*style, None),
                [] => return Err(usage),
            };
            if !matches!(style, "none" | "normal" | "pixel") {
                return Err(usage);
            }
            let style = style.to_owned();
            // Sway reads the width with atoi, so trailing text is ignored and
            // a width without leading digits is 0. A negative width is stored
            // as is and aborts sway when the next window maps
            // (wlr_scene_rect_set_size asserts width >= 0), so there is no
            // sway behaviour to copy: swayward clamps it to 0.
            let width = width.map(|width| atoi(width).clamp(0, i32::from(u16::MAX)) as u16);
            Ok(Command::SetLayoutOption(LayoutOption::DefaultBorder {
                floating: matches!(name, "default_floating_border" | "new_float"),
                style,
                width,
            }))
        }
        "popup_during_fullscreen" => match rest {
            [value]
                if matches!(
                    value.to_ascii_lowercase().as_str(),
                    "smart" | "ignore" | "leave_fullscreen"
                ) =>
            {
                Ok(Command::SetLayoutOption(
                    LayoutOption::PopupDuringFullscreen(value.to_ascii_lowercase()),
                ))
            }
            _ => Err("Expected 'popup_during_fullscreen smart|ignore|leave_fullscreen'".into()),
        },
        "floating_modifier" => {
            // `sway/sway/commands/floating_modifier.c:6-32`: at least one
            // argument, then an optional normal|inverse. `none` returns
            // before the second argument is read, so a trailing word is
            // ignored for it. Extra arguments past the second are ignored too
            // because the check is EXPECTED_AT_LEAST.
            const USAGE: &str = "Usage: floating_modifier <mod> [inverse|normal]";
            let Some((modifier, mode)) = rest.split_first() else {
                return Err(
                    "Invalid floating_modifier command (expected at least 1 argument, got 0)"
                        .into(),
                );
            };
            if modifier.eq_ignore_ascii_case("none") {
                return Ok(Command::SetLayoutOption(LayoutOption::FloatingModifier {
                    modifier: None,
                    inverse: false,
                }));
            }
            // `sway/sway/input/keyboard.c:27-39` is the whole accepted set,
            // matched case-insensitively: Shift, Lock, Control, Ctrl, Alt,
            // Mod1..Mod5 and Super. A $mod variable is expanded before the
            // command is parsed, so only these literals arrive here.
            let lowered = modifier.to_ascii_lowercase();
            let name = match lowered.as_str() {
                "shift" => "shift",
                "control" | "ctrl" => "ctrl",
                "alt" | "mod1" => "alt",
                "super" | "mod4" => "super",
                "mod3" => "iso_level5_shift",
                "mod5" => "iso_level3_shift",
                // Sway maps these to Caps Lock and Num Lock, which swayward's
                // ModKey cannot name. Refused rather than silently dropped to
                // a different modifier.
                "lock" | "mod2" => {
                    return Err(format!(
                        "swayward cannot use {modifier} as a floating modifier because it has no \
                         lock-modifier mod key"
                    ));
                }
                // Sway validates the modifier before the mode, so an invalid
                // modifier wins over an invalid trailing word.
                _ => return Err("Invalid modifier".into()),
            };
            let inverse = match mode.first() {
                None => false,
                Some(mode) if mode.eq_ignore_ascii_case("normal") => false,
                Some(mode) if mode.eq_ignore_ascii_case("inverse") => true,
                Some(_) => return Err(USAGE.into()),
            };
            Ok(Command::SetLayoutOption(LayoutOption::FloatingModifier {
                modifier: Some(name.to_owned()),
                inverse,
            }))
        }
        "mouse_warping" => match rest {
            // `sway/sway/commands/mouse_warping.c:9-16` uses strcasecmp, so
            // these three are case-insensitive, unlike focus_follows_mouse.
            [value] => {
                let mode = if value.eq_ignore_ascii_case("output") {
                    MouseWarping::Output
                } else if value.eq_ignore_ascii_case("container") {
                    MouseWarping::Container
                } else if value.eq_ignore_ascii_case("none") {
                    MouseWarping::No
                } else {
                    return Err("Expected 'mouse_warping output|container|none'".into());
                };
                Ok(Command::SetLayoutOption(LayoutOption::MouseWarping(mode)))
            }
            _ => Err("Expected 'mouse_warping output|container|none'".into()),
        },
        "font" => {
            // `sway/sway/commands/font.c` joins the remaining words and strips
            // a leading `pango:`, then reparses the description.
            if rest.is_empty() {
                return Err("Expected 'font <font>'".into());
            }
            let font = join_words(rest);
            let (font, pango_markup) = font
                .strip_prefix("pango:")
                .map_or((font.as_str(), false), |font| (font, true));
            Ok(Command::SetLayoutOption(LayoutOption::TitlebarFont {
                font: font.to_owned(),
                pango_markup,
            }))
        }
        "titlebar_border_thickness" => {
            const INVALID: &str = "Invalid size specified";
            let [value] = rest else {
                return Err(format!(
                    "Invalid titlebar_border_thickness command (expected 1 argument, got {})",
                    rest.len()
                ));
            };
            let value = value.parse().map_err(|_| INVALID.to_owned())?;
            Ok(Command::SetLayoutOption(
                LayoutOption::TitlebarBorderThickness(value),
            ))
        }
        "titlebar_padding" => {
            // One value sets both axes; two set horizontal then vertical.
            // Negatives are rejected, matching sway's `Invalid size specified`
            // (`sway/sway/commands/titlebar_padding.c:8-38`).
            const INVALID: &str = "Invalid size specified";
            let (horizontal, vertical) = match rest {
                [h] => {
                    let h: i32 = h.parse().map_err(|_| INVALID.to_owned())?;
                    (h, h)
                }
                [h, v] => (
                    h.parse().map_err(|_| INVALID.to_owned())?,
                    v.parse().map_err(|_| INVALID.to_owned())?,
                ),
                _ => return Err("Expected 'titlebar_padding <horizontal> [<vertical>]'".into()),
            };
            if horizontal < 0 || vertical < 0 {
                return Err(INVALID.into());
            }
            Ok(Command::SetLayoutOption(LayoutOption::TitlebarPadding {
                horizontal,
                vertical,
            }))
        }
        name @ ("floating_minimum_size" | "floating_maximum_size") => {
            // `sway/sway/commands/floating_minmax_size.c` wants exactly three
            // words, with a literal `x` between two integers, and rejects a
            // trailing suffix because it uses strtol and checks the remainder.
            let usage = format!("Expected '{name} <width> x <height>'");
            let [width, "x", height] = rest else {
                return Err(usage);
            };
            let (Ok(width), Ok(height)) = (width.parse::<i32>(), height.parse::<i32>()) else {
                return Err(usage);
            };
            Ok(Command::SetLayoutOption(
                if name == "floating_minimum_size" {
                    LayoutOption::FloatingMinimumSize(width, height)
                } else {
                    LayoutOption::FloatingMaximumSize(width, height)
                },
            ))
        }
        _ => Err(format!("Unknown/invalid command '{name}'")),
    }
}

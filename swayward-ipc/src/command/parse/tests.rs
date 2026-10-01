#[cfg(test)]
mod smart_borders_tests {
    use super::super::*;

    #[test]
    fn toggle_maps_to_off_like_sway() {
        assert_eq!(
            parse_one("smart_borders toggle"),
            Ok(Command::SetLayoutOption(LayoutOption::SmartBorders(
                "off".into()
            )))
        );
    }
}

#[cfg(test)]
mod workspace_number_tests {
    use super::super::*;

    #[test]
    fn workspace_numbers_must_start_with_a_digit() {
        for input in [
            "workspace number named",
            "move to workspace number named",
            "rename workspace number named to 2",
        ] {
            assert_eq!(
                parse_one(input),
                Err("Invalid workspace number 'named'".into()),
                "{input}"
            );
        }
        assert_eq!(
            parse_one("workspace number"),
            Err("Expected workspace number".into())
        );
        assert_eq!(
            parse_one(r#"workspace number "3:third""#),
            Ok(Command::Workspace {
                target: WorkspaceTarget::Number("3:third".into()),
                auto_back_and_forth: true,
            })
        );
    }
}

#[cfg(test)]
mod command_list_tests {
    use super::super::*;

    #[test]
    fn malformed_command_errors_match_sway() {
        for (input, expected) in [
            (r#"[app_id="foot" focus"#, "Token 'focus' is not recognized"),
            (
                "workspace",
                "Invalid workspace command (expected at least 1 argument, got 0)",
            ),
            (
                "layout",
                "Invalid layout command (expected at least 1 argument, got 0)",
            ),
            ("layout invalid", LAYOUT_USAGE),
            ("focus invalid", FOCUS_USAGE),
            ("move invalid", MOVE_USAGE),
        ] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some(expected),
                "{input}"
            );
        }
    }

    #[test]
    fn nul_starts_a_new_command_like_sway() {
        let parsed = parse("nop before\0after");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].as_ref().unwrap().command, Command::Nop);
        assert_eq!(
            parsed[1].as_ref().unwrap_err().error.as_deref(),
            Some("Unknown/invalid command 'after'")
        );
    }

    #[test]
    fn unterminated_nop_argument_is_ignored_like_sway() {
        let parsed = parse("nop \"unterminated");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].as_ref().unwrap().command, Command::Nop);
    }

    #[test]
    fn unterminated_quotes_are_tokenized_like_sway() {
        assert_eq!(words("reload \"unterminated"), ["reload", "unterminated"]);
        assert_eq!(
            words("set 'unterminated value"),
            ["set", "unterminated value"]
        );
        assert_eq!(words("focus \\"), ["focus", "\\"]);

        for input in [
            "reload \"unterminated",
            "floating_maximum_size \"unterminated",
            "set \"unterminated",
        ] {
            assert_ne!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some("unterminated quote"),
                "{input}"
            );
        }
    }

    #[test]
    fn comma_does_not_start_new_criteria() {
        for (input, command) in [
            ("floating enable, [title=x] kill", "[title=x]"),
            ("[app_id=x] kill, [app_id=y] kill", "[app_id=y]"),
        ] {
            let parsed = parse(input);
            assert_eq!(parsed.len(), 2, "{input}");
            assert_eq!(
                parsed[1],
                Err(parse_error(format!("Unknown/invalid command '{command}'"))),
                "{input}"
            );
        }
    }
}

#[cfg(test)]
mod exec_tests {
    use super::super::*;

    #[test]
    fn strips_outer_quotes_from_a_single_exec_argument() {
        for (input, expected) in [
            (r#"exec "foot -e htop""#, "foot -e htop"),
            ("exec 'a b'", "a b"),
            (r#"exec foo "bar baz""#, r#"foo "bar baz""#),
        ] {
            assert_eq!(
                parse_one(input),
                Ok(Command::Exec {
                    command: expected.into(),
                    no_startup_id: false,
                }),
                "{input}"
            );
        }
    }
}

#[cfg(test)]
mod criteria_rule_tests {
    use super::super::*;

    #[test]
    fn assign_parses_sway_targets() {
        for (input, expected) in [
            (
                r#"assign [app_id="^term$"] workspace 7: target"#,
                AssignmentTarget::Workspace("7: target".into()),
            ),
            (
                r#"assign [app_id="^term$"] → number 7: target"#,
                AssignmentTarget::WorkspaceNumber("7: target".into()),
            ),
            (
                r#"assign [app_id="^term$"] output HDMI-A-1"#,
                AssignmentTarget::Output("HDMI-A-1".into()),
            ),
        ] {
            let Command::Assign { criteria, target } = parse_one(input).unwrap() else {
                panic!("expected assign command");
            };
            assert_eq!(criteria, r#"[app_id="^term$"]"#);
            assert_eq!(target, expected);
        }
    }

    #[test]
    fn no_focus_parses_native_wayland_criteria() {
        assert_eq!(
            parse_one(r#"no_focus [app_id="^term$" title="dialog"]"#),
            Ok(Command::NoFocus {
                criteria: r#"[app_id="^term$" title="dialog"]"#.into(),
            })
        );
    }

    #[test]
    fn runtime_map_rules_reject_x11_only_criteria() {
        for (field, value) in [
            ("class", "value"),
            ("instance", "value"),
            ("id", "42"),
            ("window_role", "value"),
            ("window_type", "dialog"),
        ] {
            for command in [
                format!("assign [{field}={value}] workspace 2"),
                format!("no_focus [{field}={value}]"),
            ] {
                // `Criteria::parse` rejects these selectors itself and names
                // the offending field, so a rule command inherits that error
                // rather than restating it less precisely.
                assert_eq!(
                    parse_one(&command).unwrap_err(),
                    format!("X11-only criterion '{field}' is unsupported"),
                    "{command}"
                );
            }
        }
    }

    #[test]
    fn runtime_map_rules_reject_missing_or_invalid_arguments() {
        assert!(parse_one("assign [app_id=term]").is_err());
        assert!(parse_one("assign [app_id=term] number named").is_err());
        assert!(parse_one("no_focus [app_id=term] trailing").is_err());
    }
}

#[cfg(test)]
mod split_tests {
    use super::super::*;

    #[test]
    fn output_parses_chained_core_actions_and_power() {
        let parsed = parse_output_command(&[
            "HDMI-A-1",
            "scale",
            "1.5",
            "transform",
            "90",
            "position",
            "10,20",
            "mode",
            "1920",
            "1080",
        ])
        .unwrap();
        let Command::Output { target, actions } = parsed else {
            panic!("expected output command")
        };
        assert_eq!(target, "HDMI-A-1");
        assert_eq!(
            actions,
            [
                crate::OutputAction::Scale {
                    scale: crate::ScaleToSet::Specific(1.5),
                },
                crate::OutputAction::Transform {
                    transform: crate::Transform::_270,
                },
                crate::OutputAction::Position {
                    position: crate::PositionToSet::Specific(crate::ConfiguredPosition {
                        x: 10,
                        y: 20,
                    }),
                },
                crate::OutputAction::Mode {
                    mode: crate::ModeToSet::Specific(crate::ConfiguredMode {
                        width: 1920,
                        height: 1080,
                        refresh: None,
                    }),
                },
            ]
        );
        assert!(matches!(
            parse_output_command(&["*", "power", "off"]),
            Ok(Command::Output { actions, .. })
                if actions == [crate::OutputAction::Power { power: Toggle::Disable }]
        ));
        assert_eq!(
            parse_output_command(&["*", "dpms", "toggle"]),
            Err("Cannot apply toggle to all outputs".into())
        );
        assert_eq!(
            parse_output_command(&["HDMI-A-1", "scale", "NaN"]),
            Err("Invalid scale.".into())
        );
        assert!(matches!(
            parse_output_command(&["HDMI-A-1", "mode", "1920x1080@60hz"]),
            Ok(Command::Output { .. })
        ));
    }

    /// Sway accepts `t` as the toggle alias and `n` as the none alias, and
    /// compares every split argument case-insensitively with `strcasecmp`
    /// (`sway/sway/commands/split.c:54-82`). The rejection text is sway's too.
    #[test]
    fn split_accepts_sways_aliases_and_rejects_with_sways_message() {
        for (argument, expected) in [
            ("t", Some(Layout::ToggleSplit)),
            ("T", Some(Layout::ToggleSplit)),
            ("toggle", Some(Layout::ToggleSplit)),
            ("h", Some(Layout::SplitH)),
            ("vertical", Some(Layout::SplitV)),
            ("n", None),
            ("none", None),
        ] {
            assert_eq!(
                parse_split(&[argument]),
                Ok(Command::Split(expected)),
                "split {argument} must parse like sway"
            );
        }

        assert_eq!(
            parse_split(&["sideways"]),
            Err("Invalid split command (expected either horizontal or vertical).".into()),
            "sway reports this exact text for an unknown split argument"
        );
    }
}

#[cfg(test)]
mod gap_form_tests {
    use super::super::*;

    fn parse_ok(input: &str) -> Command {
        match parse_one(input) {
            Ok(command) => command,
            Err(error) => panic!("{input:?} rejected: {error}"),
        }
    }

    /// Sway dispatches on argument count: two arguments are the defaults form,
    /// four are the runtime form (`sway/sway/commands/gaps.c:205-223`). They
    /// must produce different commands, because they write different state.
    #[test]
    fn gaps_argument_count_selects_the_form() {
        assert_eq!(
            parse_ok("gaps inner 10"),
            Command::GapsDefaults {
                inner: true,
                sides: [false; 4],
                amount: 10,
            }
        );
        assert_eq!(
            parse_ok("gaps inner all set 10"),
            Command::Gaps {
                inner: true,
                sides: [false; 4],
                all: true,
                operation: GapOperation::Set,
                amount: 10,
            }
        );
    }

    /// Every kind spelling sway accepts, in both forms. `horizontal` and
    /// `vertical` select side pairs (`sway/sway/commands/gaps.c:62-84`).
    #[test]
    fn gaps_accepts_every_sway_kind_in_both_forms() {
        // [left, right, top, bottom]
        for (kind, inner, sides) in [
            ("inner", true, [false; 4]),
            ("outer", false, [true; 4]),
            ("horizontal", false, [true, true, false, false]),
            ("vertical", false, [false, false, true, true]),
            ("left", false, [true, false, false, false]),
            ("right", false, [false, true, false, false]),
            ("top", false, [false, false, true, false]),
            ("bottom", false, [false, false, false, true]),
        ] {
            assert_eq!(
                parse_ok(&format!("gaps {kind} 5")),
                Command::GapsDefaults {
                    inner,
                    sides,
                    amount: 5
                },
                "defaults form: {kind}"
            );
            assert_eq!(
                parse_ok(&format!("gaps {kind} current set 5")),
                Command::Gaps {
                    inner,
                    sides,
                    all: false,
                    operation: GapOperation::Set,
                    amount: 5,
                },
                "runtime form: {kind}"
            );
            // Sway compares with strcasecmp throughout.
            assert_eq!(
                parse_ok(&format!("gaps {} 5", kind.to_uppercase())),
                Command::GapsDefaults {
                    inner,
                    sides,
                    amount: 5
                },
                "case-insensitive: {kind}"
            );
        }
    }

    /// Sway parses the amount with `strtol` and permits a `px` suffix in
    /// `cmd_gaps` (`sway/sway/commands/gaps.c:55-58`), including negatives for
    /// outer gaps.
    #[test]
    fn gaps_amount_accepts_px_suffix_negatives_and_sway_overflow() {
        assert_eq!(
            parse_ok("gaps outer -3"),
            Command::GapsDefaults {
                inner: false,
                sides: [true; 4],
                amount: -3,
            }
        );
        assert_eq!(
            parse_ok("gaps inner 12px"),
            Command::GapsDefaults {
                inner: true,
                sides: [false; 4],
                amount: 12,
            }
        );
        assert_eq!(
            parse_ok("gaps inner current set 2147483648"),
            Command::Gaps {
                inner: true,
                sides: [false; 4],
                all: false,
                operation: GapOperation::Set,
                amount: i32::MIN,
            }
        );
        assert!(parse_one("gaps inner 12em").is_err());
        // Oracle: state/settings_gaps_amount_parse. strtol with no digits is
        // 0 and leaves the whole input, so a bare `px` is accepted; a sign is
        // allowed; past the long range strtol saturates, and the int
        // conversion keeps the low 32 bits (`sway/sway/commands/gaps.c:55-58`).
        for (input, amount) in [
            ("gaps inner px", 0),
            ("gaps outer +7PX", 7),
            ("gaps inner 99999999999999999999", -1),
        ] {
            let Command::GapsDefaults { amount: parsed, .. } = parse_ok(input) else {
                panic!("{input} did not parse as gaps defaults");
            };
            assert_eq!(parsed, amount, "{input}");
        }
    }

    /// Sway names the expectation it was testing, and names both when the
    /// argument count matches neither form
    /// (`sway/sway/commands/gaps.c:205-223`).
    #[test]
    fn gaps_rejections_use_sways_text() {
        assert_eq!(
            parse_one("gaps inner").unwrap_err(),
            "Invalid gaps command (expected at least 2 arguments, got 1)"
        );
        assert_eq!(
            parse_one("gaps bogus 10").unwrap_err(),
            format!("Expected {GAPS_EXPECTED_DEFAULTS}")
        );
        assert_eq!(
            parse_one("gaps inner sometimes set 10").unwrap_err(),
            format!("Expected {GAPS_EXPECTED_RUNTIME}")
        );
        assert_eq!(
            parse_one("gaps inner all set 10 extra").unwrap_err(),
            format!("Expected {GAPS_EXPECTED_RUNTIME} or {GAPS_EXPECTED_DEFAULTS}")
        );
    }

    /// `workspace <name> gaps <kind> <px>`
    /// (`sway/sway/commands/workspace.c:57-117`).
    #[test]
    fn workspace_gaps_parses_with_the_name_before_the_keyword() {
        assert_eq!(
            parse_ok("workspace roomy gaps inner 45"),
            Command::WorkspaceGaps {
                name: "roomy".into(),
                inner: true,
                sides: [false; 4],
                amount: 45,
            }
        );
        // Sway joins everything before the keyword into the name.
        assert_eq!(
            parse_ok("workspace my space gaps outer 3"),
            Command::WorkspaceGaps {
                name: "my space".into(),
                inner: false,
                sides: [true; 4],
                amount: 3,
            }
        );
        // A leading `gaps` is the top-level command, not a workspace name.
        assert!(matches!(
            parse_ok("gaps inner 10"),
            Command::GapsDefaults { .. }
        ));
        // Sway's workspace-gaps amount takes no suffix
        // (`sway/sway/commands/workspace.c:76-80`).
        assert!(parse_one("workspace roomy gaps inner 45px").is_err());
    }

    /// Sway collects every word after `output` as a separate entry
    /// (`sway/sway/commands/workspace.c:153-155`).
    #[test]
    fn workspace_output_assignment_keeps_the_output_list() {
        assert_eq!(
            parse_ok("workspace 7 output DP-1 HDMI-A-1"),
            Command::AssignWorkspace {
                target: WorkspaceTarget::Name("7".into()),
                outputs: vec!["DP-1".into(), "HDMI-A-1".into()],
            }
        );
        assert_eq!(
            parse_ok("workspace 7 output DP-1"),
            Command::AssignWorkspace {
                target: WorkspaceTarget::Name("7".into()),
                outputs: vec!["DP-1".into()],
            }
        );
    }
}

#[cfg(test)]
mod untrusted_input_panic_tests {
    use super::super::*;

    #[test]
    fn raw_tail_parsers_survive_multibyte_whitespace_after_empty_quotes() {
        for input in [
            "''\u{3000}exec foo",
            "''''''''''\u{3000}exec_always foo",
            "''''''\u{3000}no_focus [app_id=x]",
            "''\u{3000}for_window [app_id=x] kill",
            "''\u{3000}assign [app_id=x] 1",
        ] {
            let results = parse(input);
            assert!(
                results.iter().all(Result::is_err),
                "{input:?} must not dispatch through a mangled raw tail: {results:?}"
            );
        }
        // Sway splits argv on ASCII whitespace only and never unquotes argv[0]
        // (sway/common/stringop.c:92-140, sway/sway/commands.c:264-277).
        assert_eq!(
            parse("''\u{3000}exec foo"),
            vec![Err(parse_error("Unknown/invalid command '''\u{3000}exec'"))]
        );
    }

    /// Deterministic sweep over short strings from an alphabet that mixes command
    /// names, quotes, separators and multibyte whitespace. Untrusted IPC input
    /// must never panic the parser.
    #[test]
    fn mixed_multibyte_command_text_never_panics() {
        const PIECES: &[&str] = &[
            "exec",
            "exec_always",
            "for_window",
            "assign",
            "no_focus",
            "move",
            "resize",
            "opacity",
            "client.focused",
            "gaps",
            "output",
            "workspace",
            "mark",
            "set",
            "$a",
            "'",
            "\"",
            "''",
            " ",
            "\u{3000}",
            "\u{a0}",
            "\u{2003}",
            "é",
            "日",
            "[",
            "]",
            "=",
            "app_id",
            ";",
            ",",
            "\\",
            "px",
            "ppt",
            "1",
            "-",
            "x",
        ];
        let variables = [("$a".to_owned(), "x".to_owned())];
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        for _ in 0..200_000 {
            let mut input = String::new();
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let len = (state % 7) as usize + 1;
            for _ in 0..len {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                input.push_str(PIECES[(state % PIECES.len() as u64) as usize]);
            }
            let _ = parse(&input);
            let _ = parse_with_variables(&input, &variables, true);
        }
    }
}

#[cfg(test)]
mod quoted_command_name_tests {
    use super::super::*;

    /// Oracle: command-fuzz argv0-single-quoted-focus,
    /// argv0-double-quoted-workspace, argv0-quoted-kill, argv0-quoted-nop,
    /// argv0-quoted-mode and argv0-quoted-set. Sway strips quotes from argv[1..]
    /// only and looks argv[0] up verbatim (sway/sway/commands.c:264-277).
    #[test]
    fn quoted_command_names_are_unknown_commands() {
        for (input, name) in [
            ("'focus' left", "'focus'"),
            ("\"workspace\" oracle-quoted", "\"workspace\""),
            ("\"kill\"", "\"kill\""),
            ("'nop' oracle", "'nop'"),
            ("\"mode\" default", "\"mode\""),
            ("\"set\" $oracle value", "\"set\""),
            ("\"exec\" true", "\"exec\""),
        ] {
            assert_eq!(
                parse(input),
                vec![Err(parse_error(format!(
                    "Unknown/invalid command '{name}'"
                )))],
                "{input}"
            );
            assert_eq!(
                parse_with_variables(input, &[("$oracle".into(), "x".into())], true),
                vec![Err(parse_error(format!(
                    "Unknown/invalid command '{name}'"
                )))],
                "{input} with variables"
            );
        }
        // Quotes inside later arguments are still stripped.
        assert_eq!(
            parse("workspace \"oracle quoted\""),
            parse("workspace 'oracle quoted'")
        );
    }
}

#[cfg(test)]
mod atoi_tests {
    use super::super::settings::atoi;

    #[test]
    fn atoi_reads_a_leading_signed_integer_like_c() {
        for (input, expected) in [
            ("7", 7),
            ("7px", 7),
            ("  -3", -3),
            ("+4", 4),
            ("abc", 0),
            ("", 0),
            ("-", 0),
            ("99999999999", i32::MAX),
            ("-99999999999", i32::MIN),
        ] {
            assert_eq!(atoi(input), expected, "{input:?}");
        }
    }
}

use super::*;

fn command(input: &str) -> Command {
    parse(input).into_iter().next().unwrap().unwrap().command
}

#[test]
fn parses_all_title_format_placeholders() {
    let format = "%title %app_id %class %instance %shell %sandbox_engine %sandbox_app_id %sandbox_instance_id";
    assert_eq!(
        command(&format!("title_format {format}")),
        Command::TitleFormat(format.into())
    );
}

#[test]
fn parses_workspace_rename_forms() {
    assert_eq!(
        command("rename workspace number 5 to 7: web"),
        Command::RenameWorkspace {
            old: Some(WorkspaceTarget::Number("5".into())),
            new_name: "7: web".into(),
        }
    );
    assert_eq!(
        command("rename workspace 5 to 5: foo"),
        Command::RenameWorkspace {
            old: Some(WorkspaceTarget::Name("5".into())),
            new_name: "5: foo".into(),
        }
    );
    assert_eq!(
        command("rename workspace to mail"),
        Command::RenameWorkspace {
            old: None,
            new_name: "mail".into(),
        }
    );
}

#[test]
fn empty_layout_command_failures_match_sway() {
    let mut fixture = crate::tests::fixture::Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let outcome = execute(fixture.niri_state(), "nop before; focus");
    assert!(outcome[0].success);
    assert_eq!(
        outcome[1],
        command_failure("No container to focus was specified.")
    );

    let outcome = execute(fixture.niri_state(), "workspace fuzz; resize");
    assert!(outcome[0].success);
    assert_eq!(
        outcome[1],
        swayward_ipc::command::parse_error("Cannot resize nothing")
    );
}

#[test]
fn parses_focus_output_with_multi_word_name() {
    assert_eq!(
        parse("focus output left monitor")[0]
            .as_ref()
            .unwrap()
            .command,
        Command::FocusOutput("left monitor".into())
    );
    assert_eq!(
        parse("focus output")[0]
            .as_ref()
            .unwrap_err()
            .error
            .as_deref(),
        Some("Expected 'focus output <direction|name>'.")
    );
}

#[test]
fn parses_sway_focus_modes() {
    for input in ["focus tiling", "focus floating", "focus mode_toggle"] {
        assert!(parse(input)[0].is_ok(), "{input}");
    }
    assert_eq!(command("focus next"), Command::FocusNext);
    assert_eq!(command("focus prev"), Command::FocusPrev);
    assert_eq!(command("focus next sibling"), Command::FocusNextSibling);
    assert_eq!(command("focus prev sibling"), Command::FocusPrevSibling);
}

#[test]
fn parses_shortcuts_inhibitor_view_policy_only() {
    assert_eq!(
        command("shortcuts_inhibitor enable"),
        Command::ShortcutsInhibitor(true)
    );
    assert_eq!(
        command("shortcuts_inhibitor disable"),
        Command::ShortcutsInhibitor(false)
    );
    for input in [
        "shortcuts_inhibitor",
        "shortcuts_inhibitor toggle",
        "shortcuts_inhibitor activate",
        "shortcuts_inhibitor deactivate",
        "shortcuts_inhibitor enable extra",
    ] {
        assert_eq!(
            parse(input)[0].as_ref().unwrap_err().error.as_deref(),
            Some("Expected `shortcuts_inhibitor enable|disable`"),
            "{input}"
        );
    }
}

#[test]
fn parses_standalone_split_aliases_with_no_arguments() {
    for (alias, layout) in [
        ("splith", Layout::SplitH),
        ("splitv", Layout::SplitV),
        ("splitt", Layout::ToggleSplit),
    ] {
        assert_eq!(command(alias), Command::Split(Some(layout)));
        assert!(parse(&format!("{alias} extra"))[0].is_err());
    }
}

#[test]
fn parses_exit_and_rejects_arguments() {
    assert_eq!(command("exit"), Command::Exit);
    assert_eq!(
        parse("exit now")[0].as_ref().unwrap_err().error.as_deref(),
        Some("Invalid exit command (expected 0 arguments, got 1)")
    );
}

#[test]
fn parses_opacity_modes_and_sway_errors() {
    assert_eq!(command("opacity 0.5"), Command::Opacity(0.5));
    assert_eq!(command("opacity set 0.75"), Command::Opacity(0.75));
    assert_eq!(command("opacity plus 0.1"), Command::OpacityRelative(0.1));
    assert_eq!(command("opacity minus 0.2"), Command::OpacityRelative(-0.2));

    for (input, error) in [
        (
            "opacity",
            "Invalid opacity command (expected at least 1 argument, got 0)",
        ),
        ("opacity nope", "opacity float invalid"),
        (
            "opacity multiply 0.5",
            "Expected: set|plus|minus <0..1>: multiply",
        ),
    ] {
        assert_eq!(
            parse(input)[0].as_ref().unwrap_err().error.as_deref(),
            Some(error),
            "{input}"
        );
    }
}

#[test]
fn unsupported_runtime_state_commands_fail_loud() {
    assert_eq!(
        parse("inhibit_idle visible")[0]
            .as_ref()
            .unwrap_err()
            .error
            .as_deref(),
        Some("inhibit_idle requires user inhibitor policy support")
    );
}

#[test]
fn runtime_presentation_commands_fail_loud() {
    for (input, error) in [
        (
            "allow_tearing yes",
            "allow_tearing requires immediate presentation support",
        ),
        (
            "max_render_time 1",
            "max_render_time requires per-view render deadline support",
        ),
    ] {
        assert_eq!(
            parse(input)[0].as_ref().unwrap_err().error.as_deref(),
            Some(error),
            "{input}"
        );
    }
    assert_eq!(
        parse("max_render_time")[0]
            .as_ref()
            .unwrap_err()
            .error
            .as_deref(),
        Some("Missing max render time argument.")
    );
}

#[test]
fn parses_create_output_and_rejects_arguments() {
    assert_eq!(command("create_output"), Command::CreateOutput);
    assert!(parse("create_output extra")[0].is_err());
}

#[test]
fn parses_urgent_boolean_modes_and_refuses_request_policy_modes() {
    for mode in [
        "1", "yes", "on", "true", "enable", "enabled", "active", "toggle", "0", "no", "off",
        "false", "disable", "disabled", "inactive", "invalid",
    ] {
        assert_eq!(
            command(&format!("urgent {mode}")),
            Command::Urgent(mode.into()),
            "{mode}"
        );
    }
    for mode in ["allow", "deny"] {
        assert_eq!(
            parse(&format!("urgent {mode}"))[0]
                .as_ref()
                .unwrap_err()
                .error
                .as_deref(),
            Some("urgent allow|deny requires client urgency-request policy support")
        );
    }
}

#[test]
fn parses_sticky_with_exactly_one_argument() {
    assert_eq!(command("sticky enabled"), Command::Sticky("enabled".into()));
    for input in ["sticky", "sticky enable extra"] {
        assert_eq!(
            parse(input)[0].as_ref().unwrap_err().error.as_deref(),
            Some("Expected 'sticky <enable|disable|toggle>'")
        );
    }
}

#[test]
fn move_output_uses_the_first_target_and_ignores_extra_names() {
    assert_eq!(
        command("move window to output fake-1 fake-2"),
        Command::MoveToOutput(OutputTarget::Name("fake-1".into()))
    );
}

#[test]
fn parses_fullscreen_with_sway_boolean_vocabulary() {
    for value in ["1", "yes", "on", "true", "enable", "enabled", "active"] {
        assert_eq!(
            command(&format!("fullscreen {value}")),
            Command::Fullscreen {
                mode: Toggle::Enable,
                global: false,
            }
        );
    }
    for value in [
        "0", "no", "off", "false", "disable", "disabled", "inactive", "nope",
    ] {
        assert_eq!(
            command(&format!("fullscreen {value}")),
            Command::Fullscreen {
                mode: Toggle::Disable,
                global: false,
            }
        );
    }
    assert_eq!(
        command("fullscreen global"),
        Command::Fullscreen {
            mode: Toggle::Toggle,
            global: true,
        }
    );
    assert_eq!(
        command("fullscreen yes global"),
        Command::Fullscreen {
            mode: Toggle::Enable,
            global: true,
        }
    );
    assert_eq!(
        command("fullscreen toggle nope"),
        Command::Fullscreen {
            mode: Toggle::Toggle,
            global: false,
        }
    );
}

#[test]
fn parses_focus_and_mode_command_families() {
    assert_eq!(command("focus"), Command::Focus);
    assert_eq!(command("focus workspace"), Command::FocusWorkspace);
    assert_eq!(
        command("focus left"),
        Command::FocusDirection(Direction::Left)
    );
    assert_eq!(command("focus parent"), Command::FocusParent);
    assert_eq!(command("focus floating"), Command::FocusFloating);
    assert_eq!(command("focus tiling"), Command::FocusTiling);
    assert_eq!(command("focus mode_toggle"), Command::FocusModeToggle);
    assert_eq!(
        command("mode --pango_markup created SET $destination workspace-7"),
        Command::Mode {
            name: "created".into(),
            pango_markup: true,
            subcommand: Some(Box::new(Command::Set {
                name: "$destination".into(),
                value: "workspace-7".into(),
            })),
        }
    );
    assert_eq!(
        parse("mode --pango_markup")[0]
            .as_ref()
            .unwrap_err()
            .error
            .as_deref(),
        Some("Mode name is missing")
    );
}

#[test]
fn parses_move_and_swap_command_families() {
    assert_eq!(
        command("move right 12 px"),
        Command::MoveDirection {
            direction: Direction::Right,
            pixels: Some(12)
        }
    );
    assert_eq!(
        command("move to workspace number 3:web"),
        Command::MoveToWorkspace {
            target: WorkspaceTarget::Number("3:web".into()),
            auto_back_and_forth: true,
        }
    );
    assert_eq!(
        command("move window to output left"),
        Command::MoveToOutput(OutputTarget::Direction(Direction::Left))
    );
    assert_eq!(
        command("move container output HDMI-A-1"),
        Command::MoveToOutput(OutputTarget::Name("HDMI-A-1".into()))
    );
    for input in [
        "move mark target",
        "move to mark target",
        "move window mark target",
        "move window to mark target",
        "move container mark target",
        "move container to mark target",
    ] {
        assert_eq!(
            command(input),
            Command::MoveToMark("target".into()),
            "{input}"
        );
    }
    assert_eq!(
        command("move workspace to output right"),
        Command::MoveWorkspaceToOutput(OutputTarget::Direction(Direction::Right))
    );
    assert_eq!(
        command("move workspace output DP-1"),
        Command::MoveWorkspaceToOutput(OutputTarget::Name("DP-1".into()))
    );
    assert_eq!(command("move scratchpad"), Command::MoveScratchpad);
    assert_eq!(command("move to scratchpad"), Command::MoveScratchpad);
    assert_eq!(command("scratchpad show"), Command::ScratchpadShow);
    assert_eq!(
        command("swap container with con_id 42"),
        Command::Swap(SwapTarget::ConId("42".into()))
    );
    let parsed = parse("swap container with id 42");
    let id = parsed[0].as_ref().unwrap_err();
    assert_eq!(id.parse_error, Some(true));
    assert_eq!(
        id.error.as_deref(),
        Some("swap container with id is unsupported because X11 window IDs are unavailable")
    );
    for input in [
        "swap",
        "swap window with con_id 42",
        "swap container to con_id 42",
        "swap container with nope 42",
    ] {
        assert_eq!(
            parse(input)[0].as_ref().unwrap_err().error.as_deref(),
            Some("Expected 'swap container with id|con_id|mark <arg>'"),
            "{input}"
        );
    }
}

#[test]
fn parses_layout_and_window_command_families() {
    let stacked = parse("layout stacked");
    assert_eq!(
            stacked[0].as_ref().unwrap_err().error.as_deref(),
            Some("Expected 'layout default|tabbed|stacking|splitv|splith' or 'layout toggle [split|all]' or 'layout toggle [split|tabbed|stacking|splitv|splith] [split|tabbed|stacking|splitv|splith]...'")
        );
    assert_eq!(command("layout default"), Command::LayoutDefault);
    assert_eq!(
        command("layout toggle split"),
        Command::LayoutToggle(LayoutToggle::Split)
    );
    assert_eq!(
        command("layout toggle"),
        Command::LayoutToggle(LayoutToggle::Default)
    );
    assert_eq!(
        command("layout toggle all"),
        Command::LayoutToggle(LayoutToggle::All)
    );
    assert_eq!(
        command("layout toggle splitv garbage stacking tabbed"),
        Command::LayoutToggle(LayoutToggle::Cycle(vec![
            LayoutToggleEntry::Layout(Layout::SplitV),
            LayoutToggleEntry::Layout(Layout::Stacked),
            LayoutToggleEntry::Layout(Layout::Tabbed),
        ]))
    );
    assert!(parse("layout toggle stacked")[0].is_err());
    assert_eq!(
        command("layout toggle stacking splitv garbage tabbed"),
        Command::LayoutToggle(LayoutToggle::Cycle(vec![
            LayoutToggleEntry::Layout(Layout::Stacked),
            LayoutToggleEntry::Layout(Layout::SplitV),
            LayoutToggleEntry::Layout(Layout::Tabbed),
        ]))
    );
    assert_eq!(command("split none"), Command::Split(None));
    assert_eq!(
        command("fullscreen enable global"),
        Command::Fullscreen {
            mode: Toggle::Enable,
            global: true
        }
    );
    assert_eq!(
        command("floating toggle"),
        Command::Floating(Toggle::Toggle)
    );
    assert_eq!(
        command("border toggle 10"),
        Command::Border(swayward_ipc::command::Border {
            style: BorderStyle::Toggle,
            width: Some(10)
        })
    );
}

#[test]
fn parses_workspace_resize_and_reload_command_families() {
    assert_eq!(
        command("workspace next_on_output"),
        Command::Workspace {
            target: WorkspaceTarget::NextOnOutput,
            auto_back_and_forth: true,
        }
    );
    assert_eq!(
        command("workspace number 2:chat"),
        Command::Workspace {
            target: WorkspaceTarget::Number("2:chat".into()),
            auto_back_and_forth: true,
        }
    );
    assert_eq!(command("kill"), Command::Kill);
    assert_eq!(command("kill window"), Command::Kill);
    assert_eq!(command("kill client extra arguments"), Command::Kill);
    assert_eq!(
        command("resize shrink height 10 ppt"),
        Command::Resize {
            grow: false,
            axis: ResizeAxis::Height,
            first: ResizeAmount {
                amount: 10,
                unit: ResizeUnit::PercentagePoints,
            },
            second: None,
        }
    );
    assert_eq!(command("reload"), Command::Reload);
    assert_eq!(
        command("gaps outer all set -10px"),
        Command::Gaps {
            inner: false,
            sides: [true; 4],
            all: true,
            operation: swayward_ipc::command::GapOperation::Set,
            amount: -10,
        }
    );
}

#[test]
fn parses_nop_and_exec_command_families() {
    assert_eq!(command("nop anything is ignored"), Command::Nop);
    assert_eq!(
        command("exec --no-startup-id notify-send 'hello; world'"),
        Command::Exec {
            command: "notify-send 'hello; world'".into(),
            no_startup_id: true,
        }
    );
    assert_eq!(
        command("exec_always echo hi"),
        Command::Exec {
            command: "echo hi".into(),
            no_startup_id: false,
        }
    );
    assert_eq!(
        command("exec --no-startup-identity"),
        Command::Exec {
            command: "--no-startup-identity".into(),
            no_startup_id: false,
        }
    );
}

#[test]
fn splits_chains_outside_quotes() {
    let parsed = parse("focus left, move right; exec echo 'a,b;c'");
    assert_eq!(parsed.len(), 3);
    assert_eq!(
        parsed[0].as_ref().unwrap().command,
        Command::FocusDirection(Direction::Left)
    );
    assert_eq!(
        parsed[1].as_ref().unwrap().command,
        Command::MoveDirection {
            direction: Direction::Right,
            pixels: None
        }
    );
    assert_eq!(
        parsed[2].as_ref().unwrap().command,
        Command::Exec {
            command: "echo 'a,b;c'".into(),
            no_startup_id: false,
        }
    );
}

#[test]
fn comma_keeps_criteria_and_semicolon_starts_a_new_scope() {
    let parsed = parse(r#"[app_id="foo,bar"] focus left, focus right; [app_id="baz"] focus up"#);
    assert_eq!(parsed.len(), 3);
    assert_eq!(
        parsed[0].as_ref().unwrap().criteria.as_deref(),
        Some(r#"[app_id="foo,bar"]"#)
    );
    assert!(parsed[0].as_ref().unwrap().criteria_start);
    assert_eq!(
        parsed[1].as_ref().unwrap().criteria.as_deref(),
        Some(r#"[app_id="foo,bar"]"#)
    );
    assert!(!parsed[1].as_ref().unwrap().criteria_start);
    assert_eq!(
        parsed[2].as_ref().unwrap().criteria.as_deref(),
        Some(r#"[app_id="baz"]"#)
    );
    assert!(parsed[2].as_ref().unwrap().criteria_start);
}

#[test]
fn malformed_criteria_after_semicolon_uses_the_criteria_error() {
    let parsed = parse(r#"[app_id="foo"] nop; [con_id=nope] nop"#);
    assert_eq!(parsed.len(), 2);
    let error = parsed[1].as_ref().unwrap_err();
    assert_eq!(error.parse_error, Some(true));
    assert_eq!(
        error.error.as_deref(),
        Some("The value for 'con_id' should be '__focused__' or numeric")
    );
}

#[test]
fn rejects_invalid_criteria_before_executing_commands() {
    for input in [r#"[bogus=\"x\"] nop"#, r#"[app_id=\"(\"] nop"#, "[] nop"] {
        let error = parse(input).into_iter().next().unwrap().unwrap_err();
        assert_eq!(error.parse_error, Some(true), "{input}");
    }
}

#[test]
fn parses_sway_move_positions() {
    let px = |amount| ResizeAmount {
        amount,
        unit: ResizeUnit::Pixels,
    };
    let ppt = |amount| ResizeAmount {
        amount,
        unit: ResizeUnit::PercentagePoints,
    };
    assert_eq!(
        command("move position 5 px 15px"),
        Command::MovePosition(MovePosition::Coordinates {
            x: px(5),
            y: px(15),
            absolute: false,
        })
    );
    assert_eq!(
        command("move position 20 ppt 30ppt"),
        Command::MovePosition(MovePosition::Coordinates {
            x: ppt(20),
            y: ppt(30),
            absolute: false,
        })
    );
    assert_eq!(
        command("move absolute position center"),
        Command::MovePosition(MovePosition::Center { absolute: true })
    );
    for pointer in ["cursor", "mouse", "pointer"] {
        assert_eq!(
            command(&format!("move position {pointer}")),
            Command::MovePosition(MovePosition::Pointer)
        );
    }
}

#[test]
fn parses_move_no_auto_back_and_forth_only_for_workspace_targets() {
    for input in [
        "move --no-auto-back-and-forth workspace 3",
        "move --no-auto-back-and-forth window to workspace 3",
        "move --NO-AUTO-BACK-AND-FORTH CONTAINER workspace 3",
    ] {
        assert_eq!(
            command(input),
            Command::MoveToWorkspace {
                target: WorkspaceTarget::Name("3".into()),
                auto_back_and_forth: false,
            },
            "{input}"
        );
    }
    for input in [
        "move --no-auto-back-and-forth output right",
        "move --no-auto-back-and-forth mark target",
    ] {
        assert!(parse(input)[0].is_err(), "{input}");
    }
}

#[test]
fn parses_sway_move_distances() {
    for (input, pixels) in [
        ("move left", None),
        ("move left 20", Some(20)),
        ("move left 20 px", Some(20)),
        ("move left 20 PX", Some(20)),
        ("move left 20px", Some(20)),
        ("move left px", Some(0)),
        ("move left -20px", Some(-20)),
        ("move left 25 ppt", Some(25)),
    ] {
        assert_eq!(
            command(input),
            Command::MoveDirection {
                direction: Direction::Left,
                pixels,
            },
            "{input}"
        );
    }
    for input in ["move left 20ppt", "move left 20wat"] {
        assert_eq!(
            parse(input)[0].as_ref().unwrap_err().error.as_deref(),
            Some("Invalid distance specified"),
            "{input}"
        );
    }
}

#[test]
fn parses_sway_resize_set_forms_and_rejects_trailing_junk() {
    let amount = |amount, unit| ResizeAmount { amount, unit };
    for (input, width, height) in [
        (
            "resize set 201 131",
            Some(amount(201, ResizeUnit::Default)),
            Some(amount(131, ResizeUnit::Default)),
        ),
        (
            "resize set width 80 ppt",
            Some(amount(80, ResizeUnit::PercentagePoints)),
            None,
        ),
        (
            "resize set height 200 px",
            None,
            Some(amount(200, ResizeUnit::Pixels)),
        ),
        (
            "resize set 75 ppt 200 px",
            Some(amount(75, ResizeUnit::PercentagePoints)),
            Some(amount(200, ResizeUnit::Pixels)),
        ),
        (
            "resize set 0 ppt 75 ppt",
            Some(amount(0, ResizeUnit::PercentagePoints)),
            Some(amount(75, ResizeUnit::PercentagePoints)),
        ),
        (
            "resize set 75 ppt 0 ppt",
            Some(amount(75, ResizeUnit::PercentagePoints)),
            Some(amount(0, ResizeUnit::PercentagePoints)),
        ),
        (
            "resize set -1 px -2 ppt",
            Some(amount(-1, ResizeUnit::Pixels)),
            Some(amount(-2, ResizeUnit::PercentagePoints)),
        ),
    ] {
        assert_eq!(
            command(input),
            Command::ResizeSet { width, height },
            "{input}"
        );
    }
    for input in [
        "resize set width height 10",
        "resize set 100 px height 200 px junk",
    ] {
        assert!(parse(input)[0].is_err(), "{input}");
    }
}

#[test]
fn parses_sway_resize_adjust_forms() {
    assert_eq!(
        command("resize grow up 10 px or 25 ppt"),
        Command::Resize {
            grow: true,
            axis: ResizeAxis::Up,
            first: ResizeAmount {
                amount: 10,
                unit: ResizeUnit::Pixels,
            },
            second: Some(ResizeAmount {
                amount: 25,
                unit: ResizeUnit::PercentagePoints,
            }),
        }
    );
    assert_eq!(
        command("resize shrink left 10px"),
        Command::Resize {
            grow: false,
            axis: ResizeAxis::Left,
            first: ResizeAmount {
                amount: 10,
                unit: ResizeUnit::Pixels,
            },
            second: None,
        }
    );
    assert_eq!(
        command("resize grow right"),
        Command::Resize {
            grow: true,
            axis: ResizeAxis::Right,
            first: ResizeAmount {
                amount: 10,
                unit: ResizeUnit::Default,
            },
            second: None,
        }
    );
    assert_eq!(
        command("resize grow width 10px or 10ppt"),
        Command::Resize {
            grow: true,
            axis: ResizeAxis::Width,
            first: ResizeAmount {
                amount: 10,
                unit: ResizeUnit::Pixels,
            },
            second: Some(ResizeAmount {
                amount: 10,
                unit: ResizeUnit::PercentagePoints,
            }),
        }
    );
}

#[test]
fn parser_is_case_insensitive() {
    assert_eq!(
        command("FOCUS LEFT"),
        Command::FocusDirection(Direction::Left)
    );
    assert_eq!(
        command("resize GROW width 5 PPT"),
        Command::Resize {
            grow: true,
            axis: ResizeAxis::Width,
            first: ResizeAmount {
                amount: 5,
                unit: ResizeUnit::PercentagePoints,
            },
            second: None,
        }
    );
}

#[test]
fn parses_workspace_names_with_spaces() {
    assert_eq!(
        command("workspace number 3: web browser"),
        Command::Workspace {
            target: WorkspaceTarget::Number("3: web browser".into()),
            auto_back_and_forth: true,
        }
    );
    assert_eq!(
        command("workspace 'mail and chat'"),
        Command::Workspace {
            target: WorkspaceTarget::Name("mail and chat".into()),
            auto_back_and_forth: true,
        }
    );
}

#[test]
fn malformed_and_unknown_commands_are_parse_errors() {
    for input in [
        "focus sideways",
        "resize grow width nope px",
        "frobnicate",
        "[app_id=foo focus left",
        "fullscreen enable global extra",
        "exec",
    ] {
        let error = parse(input).into_iter().next().unwrap().unwrap_err();
        assert!(!error.success, "{input}");
        assert_eq!(error.parse_error, Some(true), "{input}");
        assert!(error.error.is_some(), "{input}");
    }
}

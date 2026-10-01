use super::*;

#[test]
fn empty_layout_command_failures_match_sway() {
    let mut fixture = crate::tests::fixture::Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let outcome = execute(fixture.niri_state(), "nop before; focus");
    assert!(outcome[0].success);
    assert_eq!(outcome[1], failure("No container to focus was specified."));

    let outcome = execute(fixture.niri_state(), "workspace fuzz; resize");
    assert!(outcome[0].success);
    assert_eq!(
        outcome[1],
        swayward_ipc::command::parse_error("Cannot resize nothing")
    );
}

#[test]
fn empty_layout_argument_failures_match_sway() {
    let mut fixture = crate::tests::fixture::Fixture::new();
    fixture.add_output(1, (1920, 1080));
    for (input, expected, parse_error) in [
        (
            "mark",
            "Invalid mark command (expected at least 1 argument, got 0)",
            true,
        ),
        (
            "border",
            "Invalid border command (expected at least 1 argument, got 0)",
            true,
        ),
        (
            "move position",
            "Only floating containers can be moved to an absolute position",
            false,
        ),
        (
            "move position 10 px",
            "Only floating containers can be moved to an absolute position",
            false,
        ),
        (
            "move position 10 em 20 px",
            "Only floating containers can be moved to an absolute position",
            false,
        ),
        (
            "rename workspace fuzz",
            "Invalid rename command (expected at least 3 arguments, got 2)",
            true,
        ),
        (
            "output",
            "Invalid output command (expected at least 1 argument, got 0)",
            true,
        ),
    ] {
        let outcome = &execute(fixture.niri_state(), input)[0];
        assert_eq!(outcome.error.as_deref(), Some(expected), "{input}");
        assert_eq!(outcome.parse_error, Some(parse_error), "{input}");
    }
}

#[test]
fn invalid_setting_values_match_sway() {
    let mut fixture = crate::tests::fixture::Fixture::new();
    fixture.add_output(1, (1920, 1080));
    let outcome = &execute(fixture.niri_state(), "focus_follows_mouse oracle_invalid")[0];
    assert_eq!(
        outcome.error.as_deref(),
        Some("Expected 'focus_follows_mouse no|yes|always'")
    );
    assert_eq!(outcome.parse_error, Some(false));
}

#[test]
fn invalid_arguments_follow_sways_handler_error_kind() {
    let mut fixture = crate::tests::fixture::Fixture::new();
    fixture.add_output(1, (1920, 1080));
    for (input, expected, parse_error) in [
        (
            "mouse_warping unterminated",
            "Expected 'mouse_warping output|container|none'",
            false,
        ),
        ("opacity unterminated", "No current container", false),
        (
            "shortcuts_inhibitor unterminated",
            "Only views can have shortcuts inhibitors",
            true,
        ),
        (
            "split unterminated",
            "Invalid split command (expected either horizontal or vertical).",
            false,
        ),
        (
            "title_format unterminated",
            "Only valid containers can have a title_format",
            true,
        ),
        (
            "titlebar_border_thickness unterminated",
            "Invalid size specified",
            false,
        ),
        (
            "unbindswitch unterminated",
            "Invalid unbindswitch command (expected binding with the form <switch>:<state>)",
            false,
        ),
    ] {
        let outcome = &execute(fixture.niri_state(), input)[0];
        assert_eq!(outcome.error.as_deref(), Some(expected), "{input}");
        assert_eq!(outcome.parse_error, Some(parse_error), "{input}");
    }
    let outcome = &execute(fixture.niri_state(), "unbindcode unterminated")[0];
    assert_eq!(
        outcome.error.as_deref(),
        Some("Could not find binding `unterminated` for the given flags")
    );
    assert_eq!(outcome.parse_error, Some(false));
}

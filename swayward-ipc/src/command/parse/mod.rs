use super::ast::*;
use super::lexer::*;
use super::variables::*;
use crate::CommandOutcome;

mod bindings;
mod error;
mod move_resize;
mod output;
mod rules;
mod settings;
mod workspace;

use bindings::*;
use error::*;
use move_resize::*;
#[cfg(test)]
use move_resize::{FOCUS_USAGE, LAYOUT_USAGE, MOVE_USAGE};
use output::*;
use rules::*;
use settings::*;
use workspace::*;

/// Expand sway variables in a command line, as sway does before dispatch.
///
/// Mirrors `do_var_replacement` (`sway/sway/config.c:890-940`):
///
/// - `\$` is escaped and left alone, minus nothing: sway skips the `$` and the backslash survives
///   into the argument, where quote stripping removes it.
/// - `$$` collapses to a single `$`.
/// - the first variable whose name prefixes the text wins. `variables` must be sorted longest name
///   first, which is how sway keeps `config->symbols` (`sway/sway/commands/set.c:13-15`), so
///   `$mod2` is not shadowed by `$mod`.
/// - an unknown `$name` is left verbatim (`sway/sway/config.c:935-937`).
///
/// Substitution is textual and single-pass: a value containing `$` is not
/// re-expanded, because sway resumes scanning after the inserted value
/// (`sway/sway/config.c:931`).
pub fn parse(input: &str) -> Vec<Result<ParsedCommand, CommandOutcome>> {
    parse_with_variables(input, &[], true)
}

/// Parse commands after applying sway's runtime variable substitution.
///
/// Command-list splitting and criteria extraction happen first, exactly as in
/// sway's `execute_command`; substitution then applies to each already-split
/// argument before handler dispatch (`sway/sway/commands.c:230-285`). Because
/// the list is already split, a semicolon or comma inside a variable value
/// remains data and cannot inject another command.
///
/// `has_view` says whether a container is focused. A few sway handlers check
/// for one before they look at their arguments, so without it their bad
/// arguments get the handler's precondition error instead.
pub fn parse_with_variables(
    input: &str,
    variables: &[(String, String)],
    has_view: bool,
) -> Vec<Result<ParsedCommand, CommandOutcome>> {
    let mut results = Vec::new();
    let mut variables = variables.to_vec();
    let mut criteria = None;
    let mut criteria_allowed = true;
    for (text, delimiter) in split_commands(input) {
        let mut text = text.trim();
        if text.is_empty() {
            if matches!(delimiter, Some(';') | Some('\0')) {
                criteria = None;
            }
            criteria_allowed = delimiter != Some(',');
            continue;
        }

        let mut criteria_start = false;
        if criteria_allowed {
            match take_criteria(text) {
                Ok(Some((raw, tail))) => {
                    criteria = Some(raw);
                    criteria_start = true;
                    text = tail;
                }
                Ok(None) => {}
                Err(error) => {
                    results.push(Err(error));
                    break;
                }
            }
        }

        let parsed = if text
            .split_ascii_whitespace()
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("nop"))
        {
            // Sway's nop handler ignores its raw tail, including malformed
            // quoting, instead of asking the generic argument parser to
            // tokenize it (`sway/sway/commands/nop.c`).
            Ok(Command::Nop)
        } else if let Some(name) = quoted_command_name(text) {
            // Sway strips quotes from argv[1..] only and looks argv[0] up
            // verbatim, so a quoted name matches no handler
            // (`sway/sway/commands.c:264-277`).
            Err(format!("Unknown/invalid command '{name}'").into())
        } else if variables.is_empty() {
            // Preserve the exact old path for commands such as `exec` and
            // `for_window`, whose parsers intentionally consume their raw
            // tails rather than a reconstructed argv.
            parse_args(&words(text), text)
        } else {
            parse_one_with_variables(text, &variables)
        };
        match parsed {
            Ok(command) => {
                // Sway executes the list sequentially, so an unscoped set at
                // the front of one IPC payload affects commands later in that
                // same payload. Carry it through the parse for those later
                // segments; execution writes the compositor-global table.
                if criteria.is_none() {
                    carry_set(&command, &mut variables);
                }
                results.push(Ok(ParsedCommand {
                    command,
                    criteria: criteria.clone(),
                    criteria_start,
                }));
            }
            Err(error) => {
                results.push(Err(error.into_outcome(has_view)));
                break;
            }
        }
        if matches!(delimiter, Some(';') | Some('\0')) {
            criteria = None;
        }
        criteria_allowed = delimiter != Some(',');
    }
    results
}

/// Split a leading `[criteria]` off a command, validating it. Returns `None`
/// when the command does not start with criteria.
fn take_criteria(text: &str) -> Result<Option<(String, &str)>, CommandOutcome> {
    if !text.starts_with('[') {
        return Ok(None);
    }
    match criteria_end(text).and_then(|end| text.split_at_checked(end + ']'.len_utf8())) {
        Some((raw, tail)) => {
            if let Err(error) = crate::criteria::Criteria::parse(raw, None) {
                return Err(parse_error(error));
            }
            Ok(Some((raw.to_owned(), tail.trim_start())))
        }
        None => {
            // Sway's criteria parser reports a more specific token or quote
            // error before noticing a missing closing bracket.
            let completed = format!("{text}]");
            let error = crate::criteria::Criteria::parse(&completed, None)
                .err()
                .unwrap_or_else(|| "No closing brace found in criteria".into());
            Err(parse_error(error))
        }
    }
}

/// Record an unscoped `set`, bare or inside `mode`, for later commands in the
/// same list.
fn carry_set(command: &Command, variables: &mut Vec<(String, String)>) {
    let set = match command {
        Command::Set { name, value } => Some((name, value)),
        Command::Mode {
            subcommand: Some(subcommand),
            ..
        } => match subcommand.as_ref() {
            Command::Set { name, value } => Some((name, value)),
            _ => None,
        },
        _ => None,
    };
    if let Some((name, value)) = set {
        set_variable(variables, name.clone(), value.clone());
    }
}

/// Expand all command arguments except the name being defined by `set`.
/// Sway starts at argv[1] normally and argv[2] for `set`
/// (`sway/sway/commands.c:283-285`).
/// Insert or replace a variable and preserve sway's longest-name-first order.
fn parse_one_with_variables(
    input: &str,
    variables: &[(String, String)],
) -> Result<Command, ParseError> {
    let words = words(input);
    let skip = if words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("set"))
    {
        2
    } else {
        1
    };
    let expanded = words
        .into_iter()
        .enumerate()
        .map(|(index, word)| {
            if index < skip {
                word.to_owned()
            } else {
                expand_variables(&word, variables)
            }
        })
        .collect::<Vec<_>>();
    let args = expanded.iter().map(String::as_str).collect::<Vec<_>>();
    let expanded_input = expanded.join(" ");
    parse_words(&args, &expanded_input)
}

pub fn parse_error(error: impl Into<String>) -> CommandOutcome {
    CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: Some(true),
    }
}

#[cfg(test)]
fn parse_one(input: &str) -> Result<Command, String> {
    parse_args(&words(input), input).map_err(ParseError::into_message)
}

fn parse_args(words: &[String], input: &str) -> Result<Command, ParseError> {
    let args = words.iter().map(String::as_str).collect::<Vec<_>>();
    parse_words(&args, input)
}

/// The raw first token when it contains a quote character.
///
/// Sway's `split_args` keeps quote characters in every token, and argv[0] is
/// never unquoted, so any quote in it makes the name unknown. Tokens split on
/// ASCII whitespace only (`sway/common/stringop.c:13,92-140`). Quoted spaces
/// stay inside the token, so `"focus left"` is one name.
fn quoted_command_name(text: &str) -> Option<&str> {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let mut quote = None;
    let mut escaped = false;
    let mut end = text.len();
    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if let Some(open) = quote {
            if character == open {
                quote = None;
            }
        } else if matches!(character, '"' | '\'') {
            quote = Some(character);
        } else if character.is_ascii_whitespace() {
            end = index;
            break;
        }
    }
    let name = text.get(..end)?;
    name.contains(['"', '\'']).then_some(name)
}

/// Parse one command, recording the reply kind sway's handler gives a
/// rejection.
fn parse_words(args: &[&str], input: &str) -> Result<Command, ParseError> {
    use ErrorKind::{Failure, Invalid};

    let Some((name, rest)) = args.split_first() else {
        return Err("expected a command".into());
    };
    let lower = name.to_ascii_lowercase();
    let error = match parse_command(&lower, name, rest, input) {
        Ok(command) => return Ok(command),
        Err(error) => ParseError::from(error),
    };
    Err(match lower.as_str() {
        // Handlers that check for a focused container before they read their
        // arguments: `sway/sway/commands/border.c:61-67`,
        // `sway/sway/commands/shortcuts_inhibitor.c:12-19`,
        // `sway/sway/commands/opacity.c:11-18`,
        // `sway/sway/commands/move.c:784-788` (move position) and
        // `sway/sway/commands/resize.c:556-561`.
        "border" if error.message_is(BORDER_SYNTAX) => {
            error.unless_view("Only views can have borders", Invalid)
        }
        "shortcuts_inhibitor" if error.message_is(SHORTCUTS_INHIBITOR_USAGE) => {
            error.unless_view("Only views can have shortcuts inhibitors", Invalid)
        }
        "opacity" if error.message_is(OPACITY_FLOAT_INVALID) => {
            error.unless_view("No current container", Failure)
        }
        "move"
            if error.message_is(&move_position_usage())
                || error.message_is(INVALID_X_POSITION)
                || error.message_is(INVALID_Y_POSITION) =>
        {
            error.unless_view(
                "Only floating containers can be moved to an absolute position",
                Failure,
            )
        }
        "resize" => error.unless_view("Cannot resize nothing", Invalid),
        // Handlers that report a bad value as CMD_FAILURE:
        // `sway/sway/commands/focus_follows_mouse.c:17-18`,
        // `sway/sway/commands/mouse_warping.c:16-17`,
        // `sway/sway/commands/split.c:80-81` and
        // `sway/sway/commands/titlebar_border_thickness.c:17`,
        // `sway/sway/commands/titlebar_padding.c:17,26`.
        "focus_follows_mouse" if error.message_is(FOCUS_FOLLOWS_MOUSE_USAGE) => {
            error.into_failure()
        }
        "mouse_warping" if error.message_is(MOUSE_WARPING_USAGE) => error.into_failure(),
        "split" if error.message_is(SPLIT_INVALID) => error.into_failure(),
        "titlebar_border_thickness" | "titlebar_padding" if error.message_is(INVALID_SIZE) => {
            error.into_failure()
        }
        _ => error,
    })
}

const SHORTCUTS_INHIBITOR_USAGE: &str = "Expected `shortcuts_inhibitor enable|disable`";

fn parse_command(lower: &str, name: &str, rest: &[&str], input: &str) -> Result<Command, String> {
    check_arity(lower, rest.len())?;
    match lower {
        "focus" => parse_focus(rest),
        "move" => parse_move(rest),
        "layout" => parse_layout(rest),
        "split" => parse_split(rest),
        "splith" => no_args(rest, "splith").map(|()| Command::Split(Some(Layout::SplitH))),
        "splitv" => no_args(rest, "splitv").map(|()| Command::Split(Some(Layout::SplitV))),
        "splitt" => no_args(rest, "splitt").map(|()| Command::Split(Some(Layout::ToggleSplit))),
        "fullscreen" => parse_fullscreen(rest),
        "floating" => match rest {
            [value] => Ok(Command::Floating(parse_boolean_toggle(value))),
            _ => Err(format!(
                "Invalid floating command (expected 1 argument, got {})",
                rest.len()
            )),
        },
        "urgent" => match rest {
            [value] if matches!(*value, "allow" | "deny") => {
                Err("urgent allow|deny requires client urgency-request policy support".into())
            }
            [value] => Ok(Command::Urgent((*value).to_owned())),
            _ => Err(format!(
                "Invalid urgent command (expected 1 argument, got {})",
                rest.len()
            )),
        },
        "border" => parse_border(rest).map(Command::Border),
        "title_format" => {
            if rest.is_empty() {
                Err("Expected 'title_format <format>'".into())
            } else {
                Ok(Command::TitleFormat(join_words(rest)))
            }
        }
        "sticky" => one(rest, "sticky <enable|disable|toggle>")
            .map(|value| Command::Sticky(value.to_owned())),
        "swap" => parse_swap(rest),
        "workspace" => parse_workspace_command(rest),
        "rename" => parse_rename(rest),
        "scratchpad" => match rest {
            [show] if show.eq_ignore_ascii_case("show") => Ok(Command::ScratchpadShow),
            _ => Err("Expected 'scratchpad show'".into()),
        },
        "kill" => Ok(Command::Kill),
        "resize" => parse_resize(rest),
        "reload" => no_args(rest, "reload").map(|()| Command::Reload),
        "exit" => {
            if rest.is_empty() {
                Ok(Command::Exit)
            } else {
                Err(format!(
                    "Invalid exit command (expected 0 arguments, got {})",
                    rest.len()
                ))
            }
        }
        "opacity" => parse_opacity(rest),
        "inhibit_idle" => Err("inhibit_idle requires user inhibitor policy support".into()),
        // Sway's developer-only create_output handler deliberately ignores argv.
        "create_output" => Ok(Command::CreateOutput),
        "input" => {
            if rest.len() < 2 {
                Err(format!(
                    "Invalid input command (expected at least 2 arguments, got {})",
                    rest.len()
                ))
            } else {
                parse_input_command(rest)
            }
        }
        "output" => parse_output_command(rest),
        "allow_tearing" => Err("allow_tearing requires immediate presentation support".into()),
        "max_render_time" if rest.is_empty() => Err("Missing max render time argument.".into()),
        "max_render_time" => {
            Err("max_render_time requires per-view render deadline support".into())
        }
        "shortcuts_inhibitor" => match rest {
            [value] if *value == "enable" => Ok(Command::ShortcutsInhibitor(true)),
            [value] if *value == "disable" => Ok(Command::ShortcutsInhibitor(false)),
            _ => Err(SHORTCUTS_INHIBITOR_USAGE.into()),
        },
        // Session-wide settings; see settings::SETTINGS.
        name if settings::is_setting(name) => settings::parse(name, rest),
        "gaps" => parse_gaps(rest),
        "set" => parse_set(rest),
        "bindsym" => parse_bind_command(rest, false, false),
        "unbindsym" => parse_bind_command(rest, false, true),
        "bindcode" => parse_bind_command(rest, true, false),
        "unbindcode" => parse_bind_command(rest, true, true),
        "bindswitch" => parse_switch_bind_command(rest, false),
        "unbindswitch" => parse_switch_bind_command(rest, true),
        "bindgesture" | "unbindgesture" => {
            Err("gesture events have no sway command-binding model".into())
        }
        "mode" => parse_mode(rest),
        "nop" => Ok(Command::Nop),
        "exec" | "exec_always" => parse_exec(input, name),
        "mark" => parse_mark(rest),
        "unmark" => Ok(Command::Unmark(
            (!rest.is_empty()).then(|| join_words(rest)),
        )),
        "for_window" => parse_for_window(input, name),
        "assign" => parse_assign(input, name),
        "no_focus" => parse_no_focus(input, name),
        _ => Err(format!("Unknown/invalid command '{name}'")),
    }
}

fn check_arity(name: &str, count: usize) -> Result<(), String> {
    let display_name = match name {
        "new_window" => "default_border",
        "new_float" => "default_floating_border",
        _ => name,
    };
    let expected = match name {
        "default_border"
        | "default_floating_border"
        | "new_window"
        | "new_float"
        | "font"
        | "mode"
        | "title_align" => Some(("at least ", 1, false)),
        "for_window" => Some(("at least ", 2, false)),
        "swap" => Some(("at least ", 4, false)),
        "focus_follows_mouse"
        | "focus_on_window_activation"
        | "focus_wrapping"
        | "force_focus_wrapping"
        | "popup_during_fullscreen"
        | "sticky"
        | "tiling_drag"
        | "tiling_drag_threshold" => Some(("", 1, true)),
        "floating_minimum_size" | "floating_maximum_size" => Some(("", 3, true)),
        "reload" | "splith" | "splitv" | "splitt" => Some(("", 0, true)),
        _ => None,
    };
    let Some((qualifier, expected, exact)) = expected else {
        return Ok(());
    };
    if (exact && count != expected) || (!exact && count < expected) {
        return Err(format!(
            "Invalid {display_name} command (expected {qualifier}{expected} argument{}, got {count})",
            if expected == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

fn no_args(args: &[&str], syntax: &str) -> Result<(), String> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(format!("Expected '{syntax}'"))
    }
}

fn one<'a>(args: &'a [&str], syntax: &str) -> Result<&'a str, String> {
    if let [arg] = args {
        Ok(arg)
    } else {
        Err(format!("Expected '{syntax}'"))
    }
}

fn parse_direction(value: &str) -> Option<Direction> {
    match value.to_ascii_lowercase().as_str() {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    }
}

fn parse_boolean_toggle(value: &str) -> Toggle {
    if value.eq_ignore_ascii_case("toggle") {
        Toggle::Toggle
    } else if parse_boolean(value, false) {
        Toggle::Enable
    } else {
        // Sway deliberately treats every other value as false to match i3
        // (`common/util.c`, `parse_boolean`).
        Toggle::Disable
    }
}

pub fn parse_boolean(value: &str, current: bool) -> bool {
    match value.to_ascii_lowercase().as_str() {
        "1" | "yes" | "on" | "true" | "enable" | "enabled" | "active" => true,
        "toggle" => !current,
        _ => false,
    }
}

#[cfg(test)]
mod command_tests;
#[cfg(test)]
mod tests;

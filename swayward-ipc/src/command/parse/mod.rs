use super::ast::*;
use super::lexer::*;
use super::variables::*;
use crate::CommandOutcome;

mod bindings;
mod move_resize;
mod output;
mod rules;
mod settings;
mod workspace;

use bindings::*;
use move_resize::*;
#[cfg(test)]
use move_resize::{FOCUS_USAGE, LAYOUT_USAGE, MOVE_USAGE};
use output::*;
use rules::*;
use settings::*;
use workspace::*;

pub fn validate(input: &str) -> Result<(), String> {
    let parsed = parse(input);
    if parsed.is_empty() {
        return Err("expected a command".into());
    }
    parsed
        .into_iter()
        .find_map(Result::err)
        .map_or(Ok(()), |error| {
            Err(error.error.unwrap_or_else(|| "invalid sway command".into()))
        })
}

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
    parse_with_variables(input, &[])
}

/// Parse commands after applying sway's runtime variable substitution.
///
/// Command-list splitting and criteria extraction happen first, exactly as in
/// sway's `execute_command`; substitution then applies to each already-split
/// argument before handler dispatch (`sway/sway/commands.c:230-285`). Because
/// the list is already split, a semicolon or comma inside a variable value
/// remains data and cannot inject another command.
pub fn parse_with_variables(
    input: &str,
    variables: &[(String, String)],
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
        if criteria_allowed && text.starts_with('[') {
            match criteria_end(text) {
                Some(end) => {
                    let raw = text[..=end].to_owned();
                    if let Err(error) = crate::criteria::Criteria::parse(&raw, None) {
                        results.push(Err(parse_error(error)));
                        break;
                    }
                    criteria = Some(raw);
                    criteria_start = true;
                    text = text[end + 1..].trim_start();
                }
                None => {
                    // Sway's criteria parser reports a more specific token or
                    // quote error before noticing a missing closing bracket.
                    let completed = format!("{text}]");
                    let error = crate::criteria::Criteria::parse(&completed, None)
                        .err()
                        .unwrap_or_else(|| "No closing brace found in criteria".into());
                    results.push(Err(parse_error(error)));
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
        } else if variables.is_empty() {
            // Preserve the exact old path for commands such as `exec` and
            // `for_window`, whose parsers intentionally consume their raw
            // tails rather than a reconstructed argv.
            parse_one(text)
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
                    let set = match &command {
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
                        set_variable(&mut variables, name.clone(), value.clone());
                    }
                }
                results.push(Ok(ParsedCommand {
                    command,
                    criteria: criteria.clone(),
                    criteria_start,
                }));
            }
            Err(error) => {
                results.push(Err(parse_error(error)));
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

/// Expand all command arguments except the name being defined by `set`.
/// Sway starts at argv[1] normally and argv[2] for `set`
/// (`sway/sway/commands.c:283-285`).
/// Insert or replace a variable and preserve sway's longest-name-first order.
fn parse_one_with_variables(
    input: &str,
    variables: &[(String, String)],
) -> Result<Command, String> {
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

fn parse_one(input: &str) -> Result<Command, String> {
    let words = words(input);
    let args = words.iter().map(String::as_str).collect::<Vec<_>>();
    parse_words(&args, input)
}

fn parse_words(args: &[&str], input: &str) -> Result<Command, String> {
    let Some(name) = args.first().copied() else {
        return Err("expected a command".into());
    };
    let (_, rest) = args
        .split_first()
        .ok_or_else(|| "expected a command".to_owned())?;
    let lower = name.to_ascii_lowercase();
    check_arity(&lower, rest.len())?;
    match lower.as_str() {
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
            _ => Err("Expected `shortcuts_inhibitor enable|disable`".into()),
        },
        // Session-wide layout settings. Sway serves these from the same table
        // as the config file (`sway/sway/commands.c:162-173`), so they are
        // runtime commands there; swayward stores the same settings in KDL and
        // re-applies the config after changing one. Accepted values and error
        // strings follow sway's own command files.
        name if matches!(
            name,
            "client.focused"
                | "client.focused_inactive"
                | "client.focused_tab_title"
                | "client.unfocused"
                | "client.urgent"
                | "focus_wrapping"
                | "force_focus_wrapping"
                | "workspace_layout"
                | "default_orientation"
                | "orientation"
                | "hide_edge_borders"
                | "smart_borders"
                | "smart_gaps"
                | "show_marks"
                | "title_align"
                | "tiling_drag"
                | "tiling_drag_threshold"
                | "force_display_urgency_hint"
                | "primary_selection"
                | "focus_on_window_activation"
                | "focus_follows_mouse"
                | "workspace_auto_back_and_forth"
                | "default_border"
                | "default_floating_border"
                | "new_window"
                | "new_float"
                | "popup_during_fullscreen"
                | "floating_modifier"
                | "mouse_warping"
                | "xwayland"
                | "font"
                | "titlebar_border_thickness"
                | "titlebar_padding"
                | "floating_minimum_size"
                | "floating_maximum_size"
        ) =>
        {
            settings::parse(name, rest)
        }
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
mod tests;

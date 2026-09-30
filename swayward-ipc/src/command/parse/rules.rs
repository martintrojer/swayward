use super::*;

/// The raw text after the command name, for handlers that consume their tail verbatim.
///
/// `name` is the lexed first word. It is a prefix of the raw text only when the user typed it
/// bare. Sway never unquotes argv[0], so a quoted or otherwise rewritten name such as `"exec"` or
/// `''<U+3000>exec` names no handler there (`sway/sway/commands.c:264-272`). Refuse it here too
/// rather than slicing the raw text at the lexed length, which can land inside a multibyte
/// character or cut the tail at the wrong place.
fn raw_tail<'a>(input: &'a str, name: &str) -> Result<&'a str, String> {
    input
        .trim_start()
        .strip_prefix(name)
        .map(str::trim_start)
        .ok_or_else(|| {
            let raw_name = input.split_ascii_whitespace().next().unwrap_or_default();
            format!("Unknown/invalid command '{raw_name}'")
        })
}

/// Split `rest`, which starts with `[`, after the criteria block's closing bracket.
fn split_criteria(rest: &str) -> Option<(&str, &str)> {
    let end = criteria_end(rest)?;
    let (criteria, tail) = rest.split_at_checked(end + ']'.len_utf8())?;
    Some((criteria, tail.trim_start()))
}

pub(super) fn parse_exec(input: &str, name: &str) -> Result<Command, String> {
    let mut command = raw_tail(input, name)?;
    const NO_STARTUP_ID: &str = "--no-startup-id";
    let mut no_startup_id = false;
    if let Some(rest) = command.strip_prefix(NO_STARTUP_ID) {
        if rest.chars().next().is_none_or(char::is_whitespace) {
            no_startup_id = true;
            command = rest.trim_start();
        }
    }
    if command.is_empty() {
        return Err(format!("Expected '{name} <command>'"));
    }
    let command = if words(command).len() == 1 && command.starts_with(['\'', '"']) {
        strip_sway_quotes(command)
    } else {
        command.to_owned()
    };
    Ok(Command::Exec {
        command,
        no_startup_id,
    })
}

pub(super) fn strip_sway_quotes(value: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    value
        .chars()
        .filter(|&character| {
            let strip = if character == '\'' && !in_double && !escaped {
                in_single = !in_single;
                true
            } else if character == '"' && !in_single && !escaped {
                in_double = !in_double;
                true
            } else {
                false
            };
            escaped = character == '\\' && !escaped;
            !strip
        })
        .collect()
}

pub(super) fn parse_mark(args: &[&str]) -> Result<Command, String> {
    let mut add = false;
    let mut toggle = false;
    let mut index = 0;
    while let Some(option) = args.get(index).filter(|arg| arg.starts_with("--")) {
        match *option {
            "--add" => add = true,
            "--replace" => add = false,
            "--toggle" => toggle = true,
            _ => return Err(format!("Unrecognized argument '{option}'")),
        }
        index += 1;
    }
    if args.is_empty() {
        return Err("Invalid mark command (expected at least 1 argument, got 0)".into());
    }
    if index == args.len() {
        return Err("Expected '[--add|--replace] [--toggle] <identifier>'".into());
    }
    Ok(Command::Mark {
        add,
        toggle,
        identifier: join_words(args.get(index..).unwrap_or_default()),
    })
}

pub(super) fn parse_rule_criteria<'a>(
    input: &'a str,
    name: &str,
    usage: &str,
) -> Result<(String, &'a str), String> {
    let rest = raw_tail(input, name)?;
    let Some((criteria, tail)) = split_criteria(rest) else {
        return Err(usage.into());
    };
    let criteria = criteria.to_owned();
    crate::criteria::Criteria::parse(&criteria, None)?;
    Ok((criteria, tail))
}

pub(super) fn parse_assign(input: &str, name: &str) -> Result<Command, String> {
    const USAGE: &str = "Expected 'assign <criteria> [workspace|number|output] <target>'";
    let (criteria, target) = parse_rule_criteria(input, name, USAGE)?;
    let target = if let Some(target) = target.strip_prefix("→") {
        target
    } else if let Some(target) = target.strip_prefix("->") {
        target
    } else {
        target
    }
    .trim_start();
    if target.is_empty() {
        return Err(USAGE.into());
    }
    let words = words(target);
    let args = words.iter().map(String::as_str).collect::<Vec<_>>();
    let target = match args.as_slice() {
        [kind, target @ ..] if kind.eq_ignore_ascii_case("output") && !target.is_empty() => {
            AssignmentTarget::Output(join_words(target))
        }
        [kind, number @ ..] if kind.eq_ignore_ascii_case("number") && !number.is_empty() => {
            let number = join_words(number);
            if !number.starts_with(|c: char| c.is_ascii_digit()) {
                return Err(format!("Invalid workspace number '{number}'"));
            }
            AssignmentTarget::WorkspaceNumber(number)
        }
        [kind, number, target @ ..]
            if kind.eq_ignore_ascii_case("workspace")
                && number.eq_ignore_ascii_case("number")
                && !target.is_empty() =>
        {
            let number = join_words(target);
            if !number.starts_with(|c: char| c.is_ascii_digit()) {
                return Err(format!("Invalid workspace number '{number}'"));
            }
            AssignmentTarget::WorkspaceNumber(number)
        }
        [kind, target @ ..] if kind.eq_ignore_ascii_case("workspace") && !target.is_empty() => {
            AssignmentTarget::Workspace(join_words(target))
        }
        [] => return Err(USAGE.into()),
        target => AssignmentTarget::Workspace(join_words(target)),
    };
    Ok(Command::Assign { criteria, target })
}

pub(super) fn parse_no_focus(input: &str, name: &str) -> Result<Command, String> {
    const USAGE: &str = "Expected 'no_focus <criteria>'";
    let rest = raw_tail(input, name)?;
    if !rest.starts_with('[') {
        return Err("No criteria".into());
    }
    let (criteria, trailing) = parse_rule_criteria(input, name, USAGE)?;
    if !trailing.is_empty() {
        return Err(USAGE.into());
    }
    Ok(Command::NoFocus { criteria })
}

pub(super) fn parse_for_window(input: &str, name: &str) -> Result<Command, String> {
    let rest = raw_tail(input, name)?;
    let Some((criteria, command)) = split_criteria(rest) else {
        return Err("Expected 'for_window [criteria] <command>'".into());
    };
    let criteria = criteria.to_owned();
    crate::criteria::Criteria::parse(&criteria, None)?;
    if command.is_empty() {
        return Err("Expected 'for_window [criteria] <command>'".into());
    }
    Ok(Command::ForWindow {
        criteria,
        command: command.to_owned(),
    })
}

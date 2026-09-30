use super::*;

pub(super) fn parse_rename(args: &[&str]) -> Result<Command, String> {
    const SYNTAX: &str =
        "Expected 'rename workspace <old_name> to <new_name>' or 'rename workspace to <new_name>'";
    if args.len() < 3 {
        return Err(format!(
            "Invalid rename command (expected at least 3 arguments, got {})",
            args.len()
        ));
    }
    let [workspace, rest @ ..] = args else {
        return Err(SYNTAX.into());
    };
    if !workspace.eq_ignore_ascii_case("workspace") {
        return Err(SYNTAX.into());
    }
    if let Some((to, new_name)) = rest.split_first() {
        if to.eq_ignore_ascii_case("to") {
            return (!new_name.is_empty())
                .then(|| Command::RenameWorkspace {
                    old: None,
                    new_name: join_words(new_name),
                })
                .ok_or_else(|| SYNTAX.into());
        }
    }
    let Some(to) = rest.iter().position(|arg| arg.eq_ignore_ascii_case("to")) else {
        return Err(SYNTAX.into());
    };
    let (old, new_name) = rest.split_at(to);
    let Some((_, new_name)) = new_name.split_first() else {
        return Err(SYNTAX.into());
    };
    if new_name.is_empty() {
        return Err(SYNTAX.into());
    }
    let old = if old
        .first()
        .is_some_and(|name| name.eq_ignore_ascii_case("number"))
    {
        parse_workspace(old)?
    } else {
        WorkspaceTarget::Name(join_words(old))
    };
    Ok(Command::RenameWorkspace {
        old: Some(old),
        new_name: join_words(new_name),
    })
}

pub(super) fn parse_swap(args: &[&str]) -> Result<Command, String> {
    const SYNTAX: &str = "Expected 'swap container with id|con_id|mark <arg>'";
    let [container, with, kind, value @ ..] = args else {
        return Err(SYNTAX.into());
    };
    if !container.eq_ignore_ascii_case("container")
        || !with.eq_ignore_ascii_case("with")
        || value.is_empty()
    {
        return Err(SYNTAX.into());
    }
    let value = join_words(value);
    let target = if kind.eq_ignore_ascii_case("id") {
        return Err(
            "swap container with id is unsupported because X11 window IDs are unavailable".into(),
        );
    } else if kind.eq_ignore_ascii_case("con_id") {
        SwapTarget::ConId(value)
    } else if kind.eq_ignore_ascii_case("mark") {
        SwapTarget::Mark(value)
    } else {
        return Err(SYNTAX.into());
    };
    Ok(Command::Swap(target))
}

pub(super) fn parse_workspace_command(args: &[&str]) -> Result<Command, String> {
    // Sway scans for `output` and `gaps` independently and lets `output` win
    // when both appear (`sway/sway/commands/workspace.c:127-148`).
    if let Some(index) = args
        .iter()
        .position(|arg| arg.eq_ignore_ascii_case("output"))
    {
        if index == 0 || index + 1 == args.len() {
            return Err("Expected 'workspace <name> output <output>'".into());
        }
        // Every remaining word is a separate output, and sway uses the first
        // one that resolves (`sway/sway/commands/workspace.c:153-155`). Do not
        // join them: that would build one impossible output name.
        let (target, outputs) = args.split_at(index);
        let Some((_, outputs)) = outputs.split_first() else {
            return Err("Expected 'workspace <name> output <output>'".into());
        };
        return Ok(Command::AssignWorkspace {
            target: parse_workspace(target)?,
            outputs: outputs.iter().map(|output| join_words(&[output])).collect(),
        });
    }
    if let Some(index) = args.iter().position(|arg| arg.eq_ignore_ascii_case("gaps")) {
        return parse_workspace_gaps(args, index);
    }
    let (auto_back_and_forth, args) = match args {
        [option, rest @ ..] if option.eq_ignore_ascii_case("--no-auto-back-and-forth") => {
            (false, rest)
        }
        _ => (true, args),
    };
    parse_workspace(args).map(|target| Command::Workspace {
        target,
        auto_back_and_forth,
    })
}

/// `workspace <name> gaps <kind> <px>`.
///
/// Sway requires exactly `gaps_location + 3` arguments and names the whole form
/// in every rejection (`sway/sway/commands/workspace.c:57-117`). The amount here
/// takes no `px` suffix: sway's workspace-gaps parser rejects any trailing text,
/// unlike `cmd_gaps` (`sway/sway/commands/workspace.c:76-80`).
pub(super) fn parse_workspace_gaps(args: &[&str], index: usize) -> Result<Command, String> {
    const EXPECTED: &str = "Expected 'workspace <name> gaps \
         inner|outer|horizontal|vertical|top|right|bottom|left <px>'";
    if index == 0 {
        return Err(EXPECTED.into());
    }
    if args.len() != index + 3 {
        return Err(format!(
            "Invalid workspace command (expected {} arguments, got {})",
            index + 3,
            args.len()
        ));
    }
    let (name, suffix) = args.split_at(index);
    let [_, kind, amount] = suffix else {
        return Err(EXPECTED.into());
    };
    let Some((inner, sides)) = parse_gaps_kind(kind) else {
        return Err(EXPECTED.into());
    };
    let Ok(amount) = amount.parse::<i32>() else {
        return Err(EXPECTED.into());
    };
    Ok(Command::WorkspaceGaps {
        name: join_words(name),
        inner,
        sides,
        amount,
    })
}

pub(super) fn parse_workspace(args: &[&str]) -> Result<WorkspaceTarget, String> {
    match args {
        [name] if name.eq_ignore_ascii_case("next") => Ok(WorkspaceTarget::Next),
        [name] if name.eq_ignore_ascii_case("prev") => Ok(WorkspaceTarget::Prev),
        [name] if name.eq_ignore_ascii_case("next_on_output") => Ok(WorkspaceTarget::NextOnOutput),
        [name] if name.eq_ignore_ascii_case("prev_on_output") => Ok(WorkspaceTarget::PrevOnOutput),
        [name] if name.eq_ignore_ascii_case("back_and_forth") => Ok(WorkspaceTarget::BackAndForth),
        [name] if name.eq_ignore_ascii_case("current") => Ok(WorkspaceTarget::Current),
        [number] if number.eq_ignore_ascii_case("number") => {
            Err("Expected workspace number".into())
        }
        [number, names @ ..] if number.eq_ignore_ascii_case("number") => {
            let name = join_words(names);
            if !name.starts_with(|character: char| character.is_ascii_digit()) {
                return Err(format!("Invalid workspace number '{name}'"));
            }
            Ok(WorkspaceTarget::Number(name))
        }
        [] => Err("Invalid workspace command (expected at least 1 argument, got 0)".into()),
        names => Ok(WorkspaceTarget::Name(join_words(names))),
    }
}

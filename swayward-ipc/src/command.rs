use crate::CommandOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Enable,
    Disable,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    SplitH,
    SplitV,
    Tabbed,
    Stacked,
    ToggleSplit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutToggle {
    Default,
    Split,
    All,
    Cycle(Vec<LayoutToggleEntry>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutToggleEntry {
    Split,
    Layout(Layout),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeAxis {
    Width,
    Height,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeUnit {
    Pixels,
    PercentagePoints,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceTarget {
    Name(String),
    Number(String),
    Next,
    Prev,
    NextOnOutput,
    PrevOnOutput,
    BackAndForth,
    Current,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    FocusDirection(Direction),
    Focus,
    FocusParent,
    FocusChild,
    FocusNext,
    FocusPrev,
    MoveDirection {
        direction: Direction,
        pixels: Option<i32>,
    },
    MoveToWorkspace(WorkspaceTarget),
    MoveScratchpad,
    ScratchpadShow,
    Layout(Layout),
    LayoutToggle(LayoutToggle),
    Split(Option<Layout>),
    Fullscreen {
        mode: Toggle,
        global: bool,
    },
    Floating(Toggle),
    Workspace(WorkspaceTarget),
    AssignWorkspace {
        target: WorkspaceTarget,
        output: String,
    },
    Kill,
    Resize {
        grow: bool,
        axis: ResizeAxis,
        amount: i32,
        unit: ResizeUnit,
    },
    Reload,
    Mode(String),
    Nop,
    Exec(String),
    Mark {
        add: bool,
        toggle: bool,
        identifier: String,
    },
    Unmark(Option<String>),
    ForWindow {
        criteria: String,
        command: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub command: Command,
    pub criteria: Option<String>,
    pub criteria_start: bool,
}

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

pub fn parse(input: &str) -> Vec<Result<ParsedCommand, CommandOutcome>> {
    let mut results = Vec::new();
    let mut criteria = None;
    for (text, delimiter) in split_commands(input) {
        let mut text = text.trim();
        if text.is_empty() {
            if delimiter == Some(';') {
                criteria = None;
            }
            continue;
        }

        let mut criteria_start = false;
        if criteria.is_none() && text.starts_with('[') {
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
                    results.push(Err(parse_error("unterminated criteria")));
                    break;
                }
            }
        }

        match parse_one(text) {
            Ok(command) => results.push(Ok(ParsedCommand {
                command,
                criteria: criteria.clone(),
                criteria_start,
            })),
            Err(error) => {
                results.push(Err(parse_error(error)));
                break;
            }
        }
        if delimiter == Some(';') {
            criteria = None;
        }
    }
    results
}

pub fn parse_error(error: impl Into<String>) -> CommandOutcome {
    CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: Some(true),
    }
}

fn split_commands(input: &str) -> Vec<(&str, Option<char>)> {
    let mut commands = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut brackets = 0;
    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '[' => brackets += 1,
            ']' => brackets = (brackets - 1).max(0),
            ';' | ',' if brackets == 0 => {
                commands.push((&input[start..index], Some(ch)));
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    commands.push((&input[start..], None));
    commands
}

fn criteria_end(input: &str) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in input.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch == ']' {
            return Some(index);
        }
    }
    None
}

fn words(input: &str) -> Result<Vec<&str>, &'static str> {
    let mut words = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            start.get_or_insert(index);
            continue;
        }
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            start.get_or_insert(index);
        } else if ch.is_whitespace() {
            if let Some(start) = start.take() {
                words.push(&input[start..index]);
            }
        } else {
            start.get_or_insert(index);
        }
    }
    if quote.is_some() {
        return Err("unterminated quote");
    }
    if let Some(start) = start {
        words.push(&input[start..]);
    }
    Ok(words)
}

fn parse_one(input: &str) -> Result<Command, String> {
    let args = words(input).map_err(str::to_owned)?;
    let Some(name) = args.first().copied() else {
        return Err("expected a command".into());
    };
    let rest = &args[1..];
    match name.to_ascii_lowercase().as_str() {
        "focus" => parse_focus(rest),
        "move" => parse_move(rest),
        "layout" => parse_layout(rest),
        "split" => parse_split(rest),
        "fullscreen" => parse_fullscreen(rest),
        "floating" => one(rest, "floating <enable|disable|toggle>")
            .and_then(parse_toggle)
            .map(Command::Floating),
        "workspace" => parse_workspace_command(rest),
        "scratchpad" => match rest {
            [show] if show.eq_ignore_ascii_case("show") => Ok(Command::ScratchpadShow),
            _ => Err("Expected 'scratchpad show'".into()),
        },
        "kill" => no_args(rest, "kill").map(|()| Command::Kill),
        "resize" => parse_resize(rest),
        "reload" => no_args(rest, "reload").map(|()| Command::Reload),
        "mode" => one(rest, "mode <name>").map(|name| Command::Mode(join_words(&[name]))),
        "nop" => Ok(Command::Nop),
        "exec" | "exec_always" => parse_exec(input, name),
        "mark" => parse_mark(rest),
        "unmark" => Ok(Command::Unmark(
            (!rest.is_empty()).then(|| join_words(rest)),
        )),
        "for_window" => parse_for_window(input, name),
        _ => Err(format!("Unknown/invalid command '{name}'")),
    }
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

fn parse_toggle(value: &str) -> Result<Toggle, String> {
    match value.to_ascii_lowercase().as_str() {
        "enable" => Ok(Toggle::Enable),
        "disable" => Ok(Toggle::Disable),
        "toggle" => Ok(Toggle::Toggle),
        _ => Err("Expected 'enable', 'disable', or 'toggle'".into()),
    }
}

fn parse_focus(args: &[&str]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Focus);
    }
    let arg = one(args, "focus <left|right|up|down|parent|child|next|prev>")?;
    if let Some(direction) = parse_direction(arg) {
        return Ok(Command::FocusDirection(direction));
    }
    match arg.to_ascii_lowercase().as_str() {
        "parent" => Ok(Command::FocusParent),
        "child" => Ok(Command::FocusChild),
        "next" => Ok(Command::FocusNext),
        "prev" => Ok(Command::FocusPrev),
        _ => Err("Expected 'focus <left|right|up|down|parent|child|next|prev>'".into()),
    }
}

fn parse_move(args: &[&str]) -> Result<Command, String> {
    if matches!(args, [scratchpad] if scratchpad.eq_ignore_ascii_case("scratchpad"))
        || matches!(args, [to, scratchpad]
            if to.eq_ignore_ascii_case("to") && scratchpad.eq_ignore_ascii_case("scratchpad"))
    {
        return Ok(Command::MoveScratchpad);
    }
    if let Some(direction) = args.first().and_then(|arg| parse_direction(arg)) {
        let pixels = match &args[1..] {
            [] => None,
            [amount] => Some(parse_i32(amount, "move distance")?),
            [amount, unit] if unit.eq_ignore_ascii_case("px") => {
                Some(parse_i32(amount, "move distance")?)
            }
            _ => return Err("Expected 'move <left|right|up|down> [px]'".into()),
        };
        return Ok(Command::MoveDirection { direction, pixels });
    }
    let target = match args {
        [workspace, rest @ ..] if workspace.eq_ignore_ascii_case("workspace") => {
            parse_workspace(rest)?
        }
        [to, workspace, rest @ ..]
            if to.eq_ignore_ascii_case("to") && workspace.eq_ignore_ascii_case("workspace") =>
        {
            parse_workspace(rest)?
        }
        _ => {
            return Err(
                "Expected 'move <direction> [px]' or 'move to workspace <name|number>'".into(),
            )
        }
    };
    Ok(Command::MoveToWorkspace(target))
}

fn parse_layout(args: &[&str]) -> Result<Command, String> {
    let direct = |layout: &str| match layout.to_ascii_lowercase().as_str() {
        "splith" => Some(Layout::SplitH),
        "splitv" => Some(Layout::SplitV),
        "tabbed" => Some(Layout::Tabbed),
        "stacked" | "stacking" => Some(Layout::Stacked),
        _ => None,
    };
    if let [layout] = args {
        if let Some(layout) = direct(layout) {
            return Ok(Command::Layout(layout));
        }
    }
    let [toggle, rest @ ..] = args else {
        return Err("Expected 'layout <splith|splitv|tabbed|stacking|toggle>'".into());
    };
    if !toggle.eq_ignore_ascii_case("toggle") {
        return Err("Expected 'layout <splith|splitv|tabbed|stacking|toggle>'".into());
    }
    let toggle = match rest {
        [] => LayoutToggle::Default,
        ["split"] => LayoutToggle::Split,
        ["all"] => LayoutToggle::All,
        [_] => return Err("Expected 'layout toggle [split|all]' or a list of layouts".into()),
        entries => {
            let cycle = entries
                .iter()
                .filter_map(|entry| {
                    if entry.eq_ignore_ascii_case("split") {
                        Some(LayoutToggleEntry::Split)
                    } else {
                        direct(entry).map(LayoutToggleEntry::Layout)
                    }
                })
                .collect::<Vec<_>>();
            if cycle.is_empty() {
                return Err("Expected a valid layout in the toggle list".into());
            }
            LayoutToggle::Cycle(cycle)
        }
    };
    Ok(Command::LayoutToggle(toggle))
}

fn parse_split(args: &[&str]) -> Result<Command, String> {
    let arg = one(args, "split <h|v|none|toggle>")?;
    let layout = match arg.to_ascii_lowercase().as_str() {
        "h" | "horizontal" => Some(Layout::SplitH),
        "v" | "vertical" => Some(Layout::SplitV),
        "toggle" => Some(Layout::ToggleSplit),
        "n" | "none" => None,
        _ => return Err("Expected 'split <h|v|none|toggle>'".into()),
    };
    Ok(Command::Split(layout))
}

fn parse_fullscreen(args: &[&str]) -> Result<Command, String> {
    let syntax = "Expected 'fullscreen [enable|disable|toggle] [global]'";
    let (mode, global) = match args {
        [] => (Toggle::Toggle, false),
        [global] if global.eq_ignore_ascii_case("global") => (Toggle::Toggle, true),
        [mode] => (parse_toggle(mode)?, false),
        [mode, global] if global.eq_ignore_ascii_case("global") => (parse_toggle(mode)?, true),
        _ => return Err(syntax.into()),
    };
    Ok(Command::Fullscreen { mode, global })
}

fn parse_workspace_command(args: &[&str]) -> Result<Command, String> {
    if let Some(index) = args
        .iter()
        .position(|arg| arg.eq_ignore_ascii_case("output"))
    {
        if index == 0 || index + 1 == args.len() {
            return Err("Expected 'workspace <name> output <output>'".into());
        }
        return Ok(Command::AssignWorkspace {
            target: parse_workspace(&args[..index])?,
            output: join_words(&args[index + 1..]),
        });
    }
    parse_workspace(args).map(Command::Workspace)
}

fn parse_workspace(args: &[&str]) -> Result<WorkspaceTarget, String> {
    match args {
        [name] if name.eq_ignore_ascii_case("next") => Ok(WorkspaceTarget::Next),
        [name] if name.eq_ignore_ascii_case("prev") => Ok(WorkspaceTarget::Prev),
        [name] if name.eq_ignore_ascii_case("next_on_output") => Ok(WorkspaceTarget::NextOnOutput),
        [name] if name.eq_ignore_ascii_case("prev_on_output") => Ok(WorkspaceTarget::PrevOnOutput),
        [name] if name.eq_ignore_ascii_case("back_and_forth") => Ok(WorkspaceTarget::BackAndForth),
        [name] if name.eq_ignore_ascii_case("current") => Ok(WorkspaceTarget::Current),
        [number, names @ ..] if number.eq_ignore_ascii_case("number") && !names.is_empty() => {
            Ok(WorkspaceTarget::Number(join_words(names)))
        }
        [] => Err("Expected 'workspace [number] <name>'".into()),
        names => Ok(WorkspaceTarget::Name(join_words(names))),
    }
}

fn parse_resize(args: &[&str]) -> Result<Command, String> {
    let [operation, axis, amount, unit @ ..] = args else {
        return Err("Expected 'resize <grow|shrink> <width|height> <n> [px|ppt]'".into());
    };
    let grow = if operation.eq_ignore_ascii_case("grow") {
        true
    } else if operation.eq_ignore_ascii_case("shrink") {
        false
    } else {
        return Err("Expected 'resize <grow|shrink> <width|height> <n> [px|ppt]'".into());
    };
    let axis = if axis.eq_ignore_ascii_case("width") {
        ResizeAxis::Width
    } else if axis.eq_ignore_ascii_case("height") {
        ResizeAxis::Height
    } else {
        return Err("Expected resize axis 'width' or 'height'".into());
    };
    let amount = parse_i32(amount, "resize amount")?;
    let unit = match unit {
        [] => ResizeUnit::Pixels,
        [unit] if unit.eq_ignore_ascii_case("px") => ResizeUnit::Pixels,
        [unit] if unit.eq_ignore_ascii_case("ppt") => ResizeUnit::PercentagePoints,
        _ => return Err("Expected resize unit 'px' or 'ppt'".into()),
    };
    Ok(Command::Resize {
        grow,
        axis,
        amount,
        unit,
    })
}

fn parse_i32(value: &str, name: &str) -> Result<i32, String> {
    value
        .parse()
        .map_err(|_| format!("Invalid {name} '{value}'"))
}

fn parse_exec(input: &str, name: &str) -> Result<Command, String> {
    let mut command = input[name.len()..].trim_start();
    if let Some(rest) = command.strip_prefix("--no-startup-id") {
        command = rest.trim_start();
    }
    if command.is_empty() {
        Err(format!("Expected '{name} <command>'"))
    } else {
        Ok(Command::Exec(command.to_owned()))
    }
}

fn parse_mark(args: &[&str]) -> Result<Command, String> {
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
    if index == args.len() {
        return Err("Expected '[--add|--replace] [--toggle] <identifier>'".into());
    }
    Ok(Command::Mark {
        add,
        toggle,
        identifier: join_words(&args[index..]),
    })
}

fn parse_for_window(input: &str, name: &str) -> Result<Command, String> {
    let rest = input[name.len()..].trim_start();
    let Some(end) = criteria_end(rest) else {
        return Err("Expected 'for_window [criteria] <command>'".into());
    };
    let criteria = rest[..=end].to_owned();
    crate::criteria::Criteria::parse(&criteria, None)?;
    let command = rest[end + 1..].trim_start();
    if command.is_empty() {
        return Err("Expected 'for_window [criteria] <command>'".into());
    }
    Ok(Command::ForWindow {
        criteria,
        command: command.to_owned(),
    })
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn join_words(words: &[&str]) -> String {
    unquote(&words.join(" ")).to_owned()
}

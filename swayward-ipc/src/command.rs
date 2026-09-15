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
pub enum BorderStyle {
    Normal,
    None,
    Pixel,
    Csd,
    Toggle,
}

impl std::str::FromStr for BorderStyle {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "normal" => Ok(Self::Normal),
            "none" => Ok(Self::None),
            "pixel" => Ok(Self::Pixel),
            "csd" => Ok(Self::Csd),
            "toggle" => Ok(Self::Toggle),
            _ => Err(format!("unknown border style `{value}`")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Border {
    pub style: BorderStyle,
    pub width: Option<u16>,
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
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeUnit {
    Default,
    Pixels,
    PercentagePoints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeAmount {
    pub amount: i32,
    pub unit: ResizeUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePosition {
    Coordinates {
        x: ResizeAmount,
        y: ResizeAmount,
        absolute: bool,
    },
    Center {
        absolute: bool,
    },
    Pointer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
    Name(String),
    Direction(Direction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapTarget {
    Id(String),
    ConId(String),
    Mark(String),
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
    FocusOutput(String),
    Focus,
    FocusWorkspace,
    FocusParent,
    FocusChild,
    FocusNext,
    FocusPrev,
    FocusNextSibling,
    FocusPrevSibling,
    FocusFloating,
    FocusTiling,
    FocusModeToggle,
    MoveDirection {
        direction: Direction,
        pixels: Option<i32>,
    },
    MovePosition(MovePosition),
    MoveToWorkspace(WorkspaceTarget),
    MoveToOutput(OutputTarget),
    MoveToMark(String),
    MoveWorkspaceToOutput(OutputTarget),
    MoveScratchpad,
    ScratchpadShow,
    Layout(Layout),
    LayoutDefault,
    LayoutToggle(LayoutToggle),
    Split(Option<Layout>),
    Fullscreen {
        mode: Toggle,
        global: bool,
    },
    Floating(Toggle),
    Border(Border),
    Sticky(String),
    Swap(SwapTarget),
    Workspace {
        target: WorkspaceTarget,
        auto_back_and_forth: bool,
    },
    AssignWorkspace {
        target: WorkspaceTarget,
        output: String,
    },
    RenameWorkspace {
        old: Option<WorkspaceTarget>,
        new_name: String,
    },
    Kill,
    Resize {
        grow: bool,
        axis: ResizeAxis,
        first: ResizeAmount,
        second: Option<ResizeAmount>,
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
        "splith" => no_args(rest, "splith").map(|()| Command::Split(Some(Layout::SplitH))),
        "splitv" => no_args(rest, "splitv").map(|()| Command::Split(Some(Layout::SplitV))),
        "splitt" => no_args(rest, "splitt").map(|()| Command::Split(Some(Layout::ToggleSplit))),
        "fullscreen" => parse_fullscreen(rest),
        "floating" => one(rest, "floating <enable|disable|toggle>")
            .and_then(parse_toggle)
            .map(Command::Floating),
        "border" => parse_border(rest).map(Command::Border),
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

pub fn parse_boolean(value: &str, current: bool) -> bool {
    match value.to_ascii_lowercase().as_str() {
        "1" | "yes" | "on" | "true" | "enable" | "enabled" | "active" => true,
        "toggle" => !current,
        _ => false,
    }
}

fn parse_border(args: &[&str]) -> Result<Border, String> {
    const SYNTAX: &str =
        "Expected 'border <none|normal|pixel|csd|toggle>' or 'border <normal|pixel|toggle> <px>'";
    let Some(style) = args.first() else {
        return Err(SYNTAX.into());
    };
    let (style, mut width) = match style.to_ascii_lowercase().as_str() {
        "normal" => (BorderStyle::Normal, None),
        "none" => (BorderStyle::None, None),
        "pixel" => (BorderStyle::Pixel, None),
        "csd" => (BorderStyle::Csd, None),
        "toggle" => (BorderStyle::Toggle, None),
        _ => return Err(SYNTAX.into()),
    };
    match args {
        [_] => {}
        [_, value] if !matches!(style, BorderStyle::None | BorderStyle::Csd) => {
            width = Some(value.parse().map_err(|_| SYNTAX.to_owned())?);
        }
        _ => return Err(SYNTAX.into()),
    }
    Ok(Border { style, width })
}

fn parse_focus(args: &[&str]) -> Result<Command, String> {
    if args.is_empty() {
        return Ok(Command::Focus);
    }
    if args[0].eq_ignore_ascii_case("output") {
        return match &args[1..] {
            [] => Err("Expected 'focus output <direction|name>'.".into()),
            output => Ok(Command::FocusOutput(join_words(output))),
        };
    }
    if let [direction, sibling] = args {
        if sibling.eq_ignore_ascii_case("sibling") {
            return match direction.to_ascii_lowercase().as_str() {
                "next" => Ok(Command::FocusNextSibling),
                "prev" => Ok(Command::FocusPrevSibling),
                _ => Err("Expected 'focus next|prev [sibling]'".into()),
            };
        }
    }
    let arg = one(
        args,
        "focus <left|right|up|down|parent|child|next|prev|floating|tiling|mode_toggle>",
    )?;
    if let Some(direction) = parse_direction(arg) {
        return Ok(Command::FocusDirection(direction));
    }
    match arg.to_ascii_lowercase().as_str() {
        "parent" => Ok(Command::FocusParent),
        "child" => Ok(Command::FocusChild),
        "next" => Ok(Command::FocusNext),
        "prev" => Ok(Command::FocusPrev),
        "floating" => Ok(Command::FocusFloating),
        "tiling" => Ok(Command::FocusTiling),
        "mode_toggle" => Ok(Command::FocusModeToggle),
        "workspace" => Ok(Command::FocusWorkspace),
        _ => Err(
            "Expected 'focus <left|right|up|down|parent|child|next|prev|floating|tiling|mode_toggle>'"
                .into(),
        ),
    }
}

fn parse_move(args: &[&str]) -> Result<Command, String> {
    let args = match args {
        [kind, rest @ ..]
            if kind.eq_ignore_ascii_case("window") || kind.eq_ignore_ascii_case("container") =>
        {
            rest
        }
        args => args,
    };
    let args = match args {
        [to, rest @ ..] if to.eq_ignore_ascii_case("to") => rest,
        args => args,
    };
    if let [workspace, rest @ ..] = args {
        if workspace.eq_ignore_ascii_case("workspace")
            && matches!(rest.first(), Some(value) if value.eq_ignore_ascii_case("to") || value.eq_ignore_ascii_case("output"))
        {
            let target = rest
                .iter()
                .skip_while(|value| {
                    value.eq_ignore_ascii_case("to") || value.eq_ignore_ascii_case("output")
                })
                .copied()
                .collect::<Vec<_>>();
            return parse_output(&target).map(Command::MoveWorkspaceToOutput);
        }
        if workspace.eq_ignore_ascii_case("output") {
            return parse_output(rest).map(Command::MoveToOutput);
        }
        if workspace.eq_ignore_ascii_case("mark") {
            return one(rest, "move [window|container] [to] mark <mark>")
                .map(|mark| Command::MoveToMark(join_words(&[mark])));
        }
    }
    if matches!(args, [scratchpad] if scratchpad.eq_ignore_ascii_case("scratchpad"))
        || matches!(args, [to, scratchpad]
            if to.eq_ignore_ascii_case("to") && scratchpad.eq_ignore_ascii_case("scratchpad"))
    {
        return Ok(Command::MoveScratchpad);
    }
    if let Some(direction) = args.first().and_then(|arg| parse_direction(arg)) {
        let pixels = args
            .get(1)
            .map(|amount| parse_move_distance(amount))
            .transpose()?;
        return Ok(Command::MoveDirection { direction, pixels });
    }
    if args.first().is_some_and(|arg| {
        arg.eq_ignore_ascii_case("position") || arg.eq_ignore_ascii_case("absolute")
    }) {
        return parse_move_position(args).map(Command::MovePosition);
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

fn parse_move_position(args: &[&str]) -> Result<MovePosition, String> {
    let (absolute, args) = match args {
        [absolute, rest @ ..] if absolute.eq_ignore_ascii_case("absolute") => (true, rest),
        args => (false, args),
    };
    let [position, args @ ..] = args else {
        return Err(move_position_usage());
    };
    if !position.eq_ignore_ascii_case("position") {
        return Err(move_position_usage());
    }
    if matches!(args, [value] if value.eq_ignore_ascii_case("center")) {
        return Ok(MovePosition::Center { absolute });
    }
    if matches!(args, [value] if value.eq_ignore_ascii_case("cursor") || value.eq_ignore_ascii_case("mouse") || value.eq_ignore_ascii_case("pointer"))
    {
        return (!absolute)
            .then_some(MovePosition::Pointer)
            .ok_or_else(move_position_usage);
    }
    if args.len() < 2 {
        return Err(move_position_usage());
    }
    let (x, consumed) = parse_resize_amount(args).map_err(|_| "Invalid x position specified")?;
    let args = &args[consumed..];
    if args.is_empty() {
        return Err(move_position_usage());
    }
    let (y, consumed) = parse_resize_amount(args).map_err(|_| "Invalid y position specified")?;
    if consumed != args.len() {
        return Err(move_position_usage());
    }
    Ok(MovePosition::Coordinates { x, y, absolute })
}

fn move_position_usage() -> String {
    "Expected 'move [absolute] position <x> [px] <y> [px]' or 'move [absolute] position center' or 'move position cursor|mouse|pointer'".into()
}

fn parse_output(args: &[&str]) -> Result<OutputTarget, String> {
    let Some(value) = args.first() else {
        return Err(
            "Expected 'move [window|container|workspace] [to] output <name|direction>'".into(),
        );
    };
    Ok(parse_direction(value).map_or_else(
        || OutputTarget::Name((*value).to_owned()),
        OutputTarget::Direction,
    ))
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
        if layout.eq_ignore_ascii_case("default") {
            return Ok(Command::LayoutDefault);
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
    let mode = |value: &str| {
        if value.eq_ignore_ascii_case("toggle") {
            Toggle::Toggle
        } else if parse_boolean(value, false) {
            Toggle::Enable
        } else {
            Toggle::Disable
        }
    };
    let (mode, global) = match args {
        [] => (Toggle::Toggle, false),
        [global] if global.eq_ignore_ascii_case("global") => (Toggle::Toggle, true),
        [value] => (mode(value), false),
        [value, global] => (mode(value), global.eq_ignore_ascii_case("global")),
        _ => return Err(syntax.into()),
    };
    Ok(Command::Fullscreen { mode, global })
}

fn parse_rename(args: &[&str]) -> Result<Command, String> {
    const SYNTAX: &str =
        "Expected 'rename workspace <old_name> to <new_name>' or 'rename workspace to <new_name>'";
    let [workspace, rest @ ..] = args else {
        return Err(SYNTAX.into());
    };
    if !workspace.eq_ignore_ascii_case("workspace") {
        return Err(SYNTAX.into());
    }
    if rest
        .first()
        .is_some_and(|arg| arg.eq_ignore_ascii_case("to"))
    {
        return (rest.len() > 1)
            .then(|| Command::RenameWorkspace {
                old: None,
                new_name: join_words(&rest[1..]),
            })
            .ok_or_else(|| SYNTAX.into());
    }
    let Some(to) = rest.iter().position(|arg| arg.eq_ignore_ascii_case("to")) else {
        return Err(SYNTAX.into());
    };
    if to + 1 == rest.len() {
        return Err(SYNTAX.into());
    }
    Ok(Command::RenameWorkspace {
        old: Some(parse_workspace(&rest[..to])?),
        new_name: join_words(&rest[to + 1..]),
    })
}

fn parse_swap(args: &[&str]) -> Result<Command, String> {
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
        SwapTarget::Id(value)
    } else if kind.eq_ignore_ascii_case("con_id") {
        SwapTarget::ConId(value)
    } else if kind.eq_ignore_ascii_case("mark") {
        SwapTarget::Mark(value)
    } else {
        return Err(SYNTAX.into());
    };
    Ok(Command::Swap(target))
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
    let [operation, axis, rest @ ..] = args else {
        return Err(resize_usage());
    };
    let grow = if operation.eq_ignore_ascii_case("grow") {
        true
    } else if operation.eq_ignore_ascii_case("shrink") {
        false
    } else {
        return Err(resize_usage());
    };
    let axis = if axis.eq_ignore_ascii_case("width") || axis.eq_ignore_ascii_case("horizontal") {
        ResizeAxis::Width
    } else if axis.eq_ignore_ascii_case("height") || axis.eq_ignore_ascii_case("vertical") {
        ResizeAxis::Height
    } else if axis.eq_ignore_ascii_case("up") {
        ResizeAxis::Up
    } else if axis.eq_ignore_ascii_case("down") {
        ResizeAxis::Down
    } else if axis.eq_ignore_ascii_case("left") {
        ResizeAxis::Left
    } else if axis.eq_ignore_ascii_case("right") {
        ResizeAxis::Right
    } else {
        return Err(resize_usage());
    };

    let (first, consumed) = if rest.is_empty() {
        (
            ResizeAmount {
                amount: 10,
                unit: ResizeUnit::Default,
            },
            0,
        )
    } else {
        parse_resize_amount(rest)?
    };
    let rest = &rest[consumed..];
    let second = if rest.is_empty() {
        None
    } else {
        let Some(rest) = rest.strip_prefix(&["or"]) else {
            return Err(resize_usage());
        };
        let (amount, consumed) = parse_resize_amount(rest)?;
        if consumed != rest.len() {
            return Err(resize_usage());
        }
        Some(amount)
    };
    Ok(Command::Resize {
        grow,
        axis,
        first,
        second,
    })
}

fn parse_move_distance(value: &str) -> Result<i32, String> {
    let bytes = value.as_bytes();
    let mut split = usize::from(
        matches!(bytes.first(), Some(b'+' | b'-')) && bytes.get(1).is_some_and(u8::is_ascii_digit),
    );
    while bytes.get(split).is_some_and(u8::is_ascii_digit) {
        split += 1;
    }
    let amount = if split == 0 {
        0
    } else {
        parse_i32(&value[..split], "move distance")?
    };
    let suffix = &value[split..];
    if suffix.is_empty() || suffix.eq_ignore_ascii_case("px") {
        Ok(amount)
    } else {
        Err("Invalid distance specified".into())
    }
}

fn parse_resize_amount(args: &[&str]) -> Result<(ResizeAmount, usize), String> {
    let value = args.first().ok_or_else(resize_usage)?;
    let split = value
        .find(|character: char| !character.is_ascii_digit() && character != '-')
        .unwrap_or(value.len());
    let amount = parse_i32(&value[..split], "resize amount")?;
    let attached_unit = &value[split..];
    let (unit, consumed) = if attached_unit.eq_ignore_ascii_case("px") {
        (ResizeUnit::Pixels, 1)
    } else if attached_unit.eq_ignore_ascii_case("ppt") {
        (ResizeUnit::PercentagePoints, 1)
    } else if !attached_unit.is_empty() {
        return Err(resize_usage());
    } else if args
        .get(1)
        .is_some_and(|unit| unit.eq_ignore_ascii_case("px"))
    {
        (ResizeUnit::Pixels, 2)
    } else if args
        .get(1)
        .is_some_and(|unit| unit.eq_ignore_ascii_case("ppt"))
    {
        (ResizeUnit::PercentagePoints, 2)
    } else {
        (ResizeUnit::Default, 1)
    };
    Ok((ResizeAmount { amount, unit }, consumed))
}

fn resize_usage() -> String {
    "Expected 'resize grow|shrink <direction> [<amount> px|ppt [or <amount> px|ppt]]'".into()
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

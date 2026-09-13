use swayward_config::Action;
use swayward_ipc::legacy::SizeChange;
use swayward_ipc::CommandOutcome;

use crate::swayward::State;
use crate::utils::spawning::spawn_sh;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    FocusDirection(Direction),
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
    criteria_start: bool,
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

fn parse_error(error: impl Into<String>) -> CommandOutcome {
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
        "layout" => parse_layout(rest).map(Command::Layout),
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
    if matches!(args, [scratchpad] if scratchpad.eq_ignore_ascii_case("scratchpad")) {
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

fn parse_layout(args: &[&str]) -> Result<Layout, String> {
    match args {
        [layout] if layout.eq_ignore_ascii_case("splith") => Ok(Layout::SplitH),
        [layout] if layout.eq_ignore_ascii_case("splitv") => Ok(Layout::SplitV),
        [layout] if layout.eq_ignore_ascii_case("tabbed") => Ok(Layout::Tabbed),
        [layout] if matches!(layout.to_ascii_lowercase().as_str(), "stacked" | "stacking") => {
            Ok(Layout::Stacked)
        }
        [toggle, split]
            if toggle.eq_ignore_ascii_case("toggle") && split.eq_ignore_ascii_case("split") =>
        {
            Ok(Layout::ToggleSplit)
        }
        _ => Err("Expected 'layout <splith|splitv|tabbed|stacked|toggle split>'".into()),
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandTarget {
    Window(crate::window::mapped::MappedId),
    Container(
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
}

pub fn execute(state: &mut State, input: &str) -> Vec<CommandOutcome> {
    let mut parsed = parse(input);
    for item in &mut parsed {
        if let Ok(command) = item {
            if let Some(raw) = &command.criteria {
                if let Err(error) = crate::criteria::Criteria::parse(raw, focused_con_id(state)) {
                    *item = Err(parse_error(error));
                }
            }
        }
    }
    let mut retained_targets = None;
    parsed
        .into_iter()
        .map(|parsed| match parsed {
            Ok(parsed) => {
                if parsed.criteria_start {
                    retained_targets = None;
                }
                execute_one(state, parsed, &mut retained_targets)
            }
            Err(error) => error,
        })
        .collect()
}

fn execute_one(
    state: &mut State,
    parsed: ParsedCommand,
    retained_targets: &mut Option<Vec<CommandTarget>>,
) -> CommandOutcome {
    let targets = match parsed.criteria.as_deref() {
        Some(raw) => match crate::criteria::Criteria::parse(raw, focused_con_id(state)) {
            Ok(criteria) => retained_targets
                .get_or_insert_with(|| matching_targets(state, &criteria))
                .clone(),
            Err(error) => return failure(error),
        },
        None => Vec::new(),
    };
    if parsed.criteria.is_some() {
        if targets.is_empty() {
            return failure("No matching node.");
        }
        if let Command::Unmark(identifier) = &parsed.command {
            for target in targets {
                unmark_target(state, target, identifier.as_deref());
            }
            return success();
        }
        for target in targets {
            let outcome = execute_targeted(state, &parsed.command, target);
            if !outcome.success {
                return outcome;
            }
        }
        return success();
    }

    if matches!(parsed.command, Command::Mark { .. }) && focused_target(state).is_none() {
        return success();
    }

    let action = match parsed.command {
        Command::FocusDirection(Direction::Left) => Some(Action::FocusColumnLeft),
        Command::FocusDirection(Direction::Right) => Some(Action::FocusColumnRight),
        Command::FocusDirection(Direction::Up) => Some(Action::FocusWindowUp),
        Command::FocusDirection(Direction::Down) => Some(Action::FocusWindowDown),
        Command::FocusParent => {
            state.swayward.layout.focus_parent();
            state.swayward.queue_redraw_all();
            None
        }
        Command::FocusChild => {
            state.swayward.layout.focus_child();
            state.swayward.queue_redraw_all();
            None
        }
        Command::FocusNext => Some(Action::FocusColumnRightOrFirst),
        Command::FocusPrev => Some(Action::FocusColumnLeftOrLast),
        Command::MoveDirection {
            direction,
            pixels: None,
        }
        | Command::MoveDirection {
            direction,
            pixels: Some(10),
        } => Some(match direction {
            Direction::Left => Action::MoveColumnLeft,
            Direction::Right => Action::MoveColumnRight,
            Direction::Up => Action::MoveWindowUp,
            Direction::Down => Action::MoveWindowDown,
        }),
        Command::MoveDirection {
            pixels: Some(_), ..
        } => return failure("custom floating move distances are not implemented yet"),
        Command::MoveToWorkspace(target) => {
            if let Err(error) = state.swayward.layout.move_to_sway_workspace(target) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::MoveScratchpad => {
            state.swayward.layout.move_to_scratchpad(None);
            state.swayward.queue_redraw_all();
            None
        }
        Command::ScratchpadShow => {
            state.swayward.layout.show_scratchpad(None);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Layout(layout) => {
            match layout {
                Layout::SplitH => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::SplitH),
                Layout::SplitV => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::SplitV),
                Layout::Tabbed => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::Tabbed),
                Layout::Stacked => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::Stacked),
                Layout::ToggleSplit => state.swayward.layout.toggle_focused_split(),
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(Some(Layout::SplitH)) => {
            state
                .swayward
                .layout
                .split_focused(crate::layout::tiling_tree::Layout::SplitH);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(Some(Layout::SplitV)) => {
            state
                .swayward
                .layout
                .split_focused(crate::layout::tiling_tree::Layout::SplitV);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(Some(Layout::ToggleSplit)) => {
            state.swayward.layout.toggle_focused_split();
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(None) => return failure("container flattening is not implemented yet"),
        Command::Split(Some(Layout::Tabbed | Layout::Stacked)) => {
            return failure("invalid split layout");
        }
        Command::Fullscreen { global: true, .. } => {
            return failure("global fullscreen is not implemented yet");
        }
        Command::Fullscreen {
            mode,
            global: false,
        } => {
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
                return success();
            };
            match mode {
                Toggle::Enable => state.swayward.layout.set_fullscreen(&window, true),
                Toggle::Disable => state.swayward.layout.set_fullscreen(&window, false),
                Toggle::Toggle => state.swayward.layout.toggle_fullscreen(&window),
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Floating(mode) => {
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
                return success();
            };
            match mode {
                Toggle::Enable => state
                    .swayward
                    .layout
                    .set_window_floating(Some(&window), true),
                Toggle::Disable => state
                    .swayward
                    .layout
                    .set_window_floating(Some(&window), false),
                Toggle::Toggle => state.swayward.layout.toggle_window_floating(Some(&window)),
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Workspace(target) => {
            if let Err(error) = state.swayward.layout.activate_sway_workspace(target) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::AssignWorkspace { target, output } => {
            if let Err(error) = state.swayward.layout.assign_sway_workspace(target, &output) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Kill => Some(Action::CloseWindow),
        Command::Resize {
            grow,
            axis,
            amount,
            unit,
        } => {
            let sign = if grow { 1 } else { -1 };
            let amount = amount.saturating_mul(sign);
            let change = match unit {
                ResizeUnit::Pixels => SizeChange::AdjustFixed(amount),
                ResizeUnit::PercentagePoints => SizeChange::AdjustProportion(f64::from(amount)),
            };
            Some(match axis {
                ResizeAxis::Width => Action::SetWindowWidth(change),
                ResizeAxis::Height => Action::SetWindowHeight(change),
            })
        }
        Command::Reload => {
            let Some(watcher) = &state.swayward.config_file_watcher else {
                return failure("config reload is not available without a config file watcher");
            };
            watcher.load_config(None);
            None
        }
        Command::Nop => None,
        Command::Exec(command) => {
            let (token, _) = state.swayward.activation_state.create_external_token(None);
            spawn_sh(command, Some(token.clone()));
            None
        }
        Command::Mark {
            add,
            toggle,
            identifier,
        } => {
            if let Some(target) = focused_target(state) {
                mark_target(state, target, &identifier, add, toggle);
            }
            None
        }
        Command::Unmark(identifier) => {
            if parsed.criteria.is_some() {
                for target in targets {
                    unmark_target(state, target, identifier.as_deref());
                }
            } else {
                state.swayward.unmark(None, identifier.as_deref());
            }
            None
        }
        Command::ForWindow { criteria, command } => {
            let parsed = match crate::criteria::Criteria::parse(&criteria, focused_con_id(state)) {
                Ok(criteria) => criteria,
                Err(error) => return failure(error),
            };
            if !state
                .swayward
                .for_window
                .iter()
                .any(|(raw, existing, _)| raw == &criteria && existing == &command)
            {
                state.swayward.for_window.push((criteria, command, parsed));
            }
            None
        }
    };

    if let Some(action) = action {
        state.do_action(action, false);
    }
    state.ipc_refresh_layout();
    success()
}

fn execute_targeted(state: &mut State, command: &Command, target: CommandTarget) -> CommandOutcome {
    match command {
        Command::Mark {
            add,
            toggle,
            identifier,
        } => mark_target(state, target, identifier, *add, *toggle),
        Command::Unmark(identifier) => unmark_target(state, target, identifier.as_deref()),
        Command::Fullscreen {
            mode,
            global: false,
        } => {
            let CommandTarget::Window(target) = target else {
                return failure("command requires a window target");
            };
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
            };
            match mode {
                Toggle::Enable => state.swayward.layout.set_fullscreen(&window, true),
                Toggle::Disable => state.swayward.layout.set_fullscreen(&window, false),
                Toggle::Toggle => state.swayward.layout.toggle_fullscreen(&window),
            }
            state.swayward.queue_redraw_all();
        }
        Command::Floating(mode) => {
            let CommandTarget::Window(target) = target else {
                return failure("command requires a window target");
            };
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
            };
            match mode {
                Toggle::Enable => state
                    .swayward
                    .layout
                    .set_window_floating(Some(&window), true),
                Toggle::Disable => state
                    .swayward
                    .layout
                    .set_window_floating(Some(&window), false),
                Toggle::Toggle => state.swayward.layout.toggle_window_floating(Some(&window)),
            }
            state.swayward.queue_redraw_all();
        }
        Command::Layout(layout) => {
            let CommandTarget::Container(_, node) = target else {
                return failure("command requires a container target");
            };
            let layout = match layout {
                Layout::SplitH => crate::layout::tiling_tree::Layout::SplitH,
                Layout::SplitV => crate::layout::tiling_tree::Layout::SplitV,
                Layout::Tabbed => crate::layout::tiling_tree::Layout::Tabbed,
                Layout::Stacked => crate::layout::tiling_tree::Layout::Stacked,
                Layout::ToggleSplit => {
                    return failure("targeted toggle split is not implemented yet")
                }
            };
            state.swayward.layout.set_tiling_node_layout(node, layout);
            state.swayward.queue_redraw_all();
        }
        Command::Nop => {}
        _ => return failure("criteria targets are not implemented for this command yet"),
    }
    state.ipc_refresh_layout();
    success()
}

fn focused_target(state: &State) -> Option<CommandTarget> {
    if let Some(window) = focused_id(state) {
        return Some(CommandTarget::Window(window));
    }
    let workspace = state.swayward.layout.active_workspace()?;
    workspace
        .focused_tiling_node()
        .map(|node| CommandTarget::Container(workspace.id(), node))
}

fn mark_target(state: &mut State, target: CommandTarget, mark: &str, add: bool, toggle: bool) {
    match target {
        CommandTarget::Window(window) => state.swayward.set_mark(window, mark, add, toggle),
        CommandTarget::Container(workspace, node) => {
            let marks = state
                .swayward
                .marks_by_container
                .entry((workspace, node))
                .or_default();
            if !add {
                marks.clear();
            }
            if let Some(index) = marks.iter().position(|existing| existing == mark) {
                if toggle {
                    marks.remove(index);
                }
            } else {
                marks.push(mark.to_owned());
            }
        }
    }
}

fn unmark_target(state: &mut State, target: CommandTarget, mark: Option<&str>) {
    match target {
        CommandTarget::Window(window) => state.swayward.unmark(Some(window), mark),
        CommandTarget::Container(workspace, node) => {
            if let Some(mark) = mark {
                if let Some(marks) = state
                    .swayward
                    .marks_by_container
                    .get_mut(&(workspace, node))
                {
                    marks.retain(|existing| existing != mark);
                }
            } else {
                state.swayward.marks_by_container.remove(&(workspace, node));
            }
        }
    }
}

fn focused_id(state: &State) -> Option<crate::window::mapped::MappedId> {
    state.swayward.layout.focus().map(|mapped| mapped.id())
}

fn focused_con_id(state: &State) -> Option<u64> {
    state
        .swayward
        .layout
        .focused_tiling_node()
        .map(|id| crate::ipc::tree::container_id(id) as u64)
        .or_else(|| focused_id(state).map(|id| crate::ipc::tree::window_id(id) as u64))
}

type WindowSnapshot = (
    crate::window::mapped::MappedId,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    bool,
    Option<i32>,
);

fn snapshot_info<'a>(
    state: &'a State,
    snapshot: &'a WindowSnapshot,
) -> crate::criteria::WindowInfo<'a> {
    crate::criteria::WindowInfo {
        title: snapshot.1.as_deref(),
        shell: Some("xdg_shell"),
        app_id: snapshot.2.as_deref(),
        marks: state
            .swayward
            .marks_by_window
            .get(&snapshot.0)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        con_id: crate::ipc::tree::window_id(snapshot.0) as u64,
        floating: snapshot.4,
        urgent: snapshot.5,
        workspace: snapshot.3.as_deref(),
        pid: snapshot.6.and_then(|pid| u32::try_from(pid).ok()),
        ..Default::default()
    }
}

fn matching_targets(state: &State, criteria: &crate::criteria::Criteria) -> Vec<CommandTarget> {
    use crate::utils::with_toplevel_role;

    let focused_id = focused_id(state);
    let mut snapshots = Vec::new();
    state
        .swayward
        .layout
        .with_windows(|mapped, _, workspace_id, _| {
            let (title, app_id) = with_toplevel_role(mapped.toplevel(), |role| {
                (role.title.clone(), role.app_id.clone())
            });
            let workspace = workspace_id.and_then(|id| {
                state
                    .swayward
                    .layout
                    .workspaces()
                    .find_map(|(_, _, ws)| (ws.id() == id).then(|| ws.sway_name()).flatten())
            });
            snapshots.push((
                mapped.id(),
                title,
                app_id,
                workspace,
                mapped.is_floating(),
                mapped.is_urgent(),
                mapped.credentials().map(|c| c.pid),
            ));
        });
    let focused = snapshots
        .iter()
        .find(|snapshot| Some(snapshot.0) == focused_id);
    let focused_info = focused
        .map(|snapshot| snapshot_info(state, snapshot))
        .unwrap_or_default();
    let mut targets = snapshots
        .iter()
        .filter(|snapshot| criteria.matches(&snapshot_info(state, snapshot), &focused_info))
        .map(|snapshot| CommandTarget::Window(snapshot.0))
        .collect::<Vec<_>>();
    for (_, _, workspace) in state.swayward.layout.workspaces() {
        for (node, value) in workspace.ipc_tiling_tree().nodes() {
            if matches!(value, crate::layout::tiling_tree::IpcNodeKind::Split) {
                let marks = state
                    .swayward
                    .marks_by_container
                    .get(&(workspace.id(), node))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if criteria.matches_container(crate::ipc::tree::container_id(node) as u64, marks) {
                    targets.push(CommandTarget::Container(workspace.id(), node));
                }
            }
        }
    }
    targets
}

fn matching_ids(
    state: &State,
    criteria: &crate::criteria::Criteria,
) -> Vec<crate::window::mapped::MappedId> {
    matching_targets(state, criteria)
        .into_iter()
        .filter_map(|target| match target {
            CommandTarget::Window(id) => Some(id),
            CommandTarget::Container(_, _) => None,
        })
        .collect()
}

pub fn run_for_window(state: &mut State, id: crate::window::mapped::MappedId) {
    let commands = state
        .swayward
        .for_window
        .iter()
        .filter_map(|(_, command, criteria)| {
            matching_ids(state, criteria)
                .contains(&id)
                .then_some(command.clone())
        })
        .collect::<Vec<_>>();
    for command in commands {
        let targeted = format!("[con_id={}] {command}", crate::ipc::tree::window_id(id));
        let _ = execute(state, &targeted);
    }
}

fn success() -> CommandOutcome {
    CommandOutcome {
        success: true,
        error: None,
        parse_error: None,
    }
}

fn failure(error: impl Into<String>) -> CommandOutcome {
    CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(input: &str) -> Command {
        parse(input).into_iter().next().unwrap().unwrap().command
    }

    #[test]
    fn parses_every_supported_command_family() {
        assert_eq!(
            command("focus left"),
            Command::FocusDirection(Direction::Left)
        );
        assert_eq!(command("focus parent"), Command::FocusParent);
        assert_eq!(
            command("move right 12 px"),
            Command::MoveDirection {
                direction: Direction::Right,
                pixels: Some(12)
            }
        );
        assert_eq!(
            command("move to workspace number 3:web"),
            Command::MoveToWorkspace(WorkspaceTarget::Number("3:web".into()))
        );
        assert_eq!(command("move scratchpad"), Command::MoveScratchpad);
        assert_eq!(command("scratchpad show"), Command::ScratchpadShow);
        assert_eq!(command("layout stacked"), Command::Layout(Layout::Stacked));
        assert_eq!(
            command("layout toggle split"),
            Command::Layout(Layout::ToggleSplit)
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
            command("workspace next_on_output"),
            Command::Workspace(WorkspaceTarget::NextOnOutput)
        );
        assert_eq!(
            command("workspace number 2:chat"),
            Command::Workspace(WorkspaceTarget::Number("2:chat".into()))
        );
        assert_eq!(command("kill"), Command::Kill);
        assert_eq!(
            command("resize shrink height 10 ppt"),
            Command::Resize {
                grow: false,
                axis: ResizeAxis::Height,
                amount: 10,
                unit: ResizeUnit::PercentagePoints
            }
        );
        assert_eq!(command("reload"), Command::Reload);
        assert_eq!(command("nop anything is ignored"), Command::Nop);
        assert_eq!(
            command("exec --no-startup-id notify-send 'hello; world'"),
            Command::Exec("notify-send 'hello; world'".into())
        );
        assert_eq!(
            command("exec_always echo hi"),
            Command::Exec("echo hi".into())
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
            Command::Exec("echo 'a,b;c'".into())
        );
    }

    #[test]
    fn comma_keeps_criteria_and_semicolon_clears_it() {
        let parsed = parse(r#"[app_id="foo,bar"] focus left, focus right; focus up"#);
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0].as_ref().unwrap().criteria.as_deref(),
            Some(r#"[app_id="foo,bar"]"#)
        );
        assert_eq!(
            parsed[1].as_ref().unwrap().criteria.as_deref(),
            Some(r#"[app_id="foo,bar"]"#)
        );
        assert_eq!(parsed[2].as_ref().unwrap().criteria, None);
    }

    #[test]
    fn rejects_invalid_criteria_before_executing_commands() {
        for input in [r#"[bogus=\"x\"] nop"#, r#"[app_id=\"(\"] nop"#, "[] nop"] {
            let error = parse(input).into_iter().next().unwrap().unwrap_err();
            assert_eq!(error.parse_error, Some(true), "{input}");
        }
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
                amount: 5,
                unit: ResizeUnit::PercentagePoints,
            }
        );
    }

    #[test]
    fn parses_workspace_names_with_spaces() {
        assert_eq!(
            command("workspace number 3: web browser"),
            Command::Workspace(WorkspaceTarget::Number("3: web browser".into()))
        );
        assert_eq!(
            command("workspace 'mail and chat'"),
            Command::Workspace(WorkspaceTarget::Name("mail and chat".into()))
        );
    }

    #[test]
    fn malformed_and_unknown_commands_are_parse_errors() {
        for input in [
            "focus sideways",
            "resize grow width nope px",
            "frobnicate",
            "[app_id=foo focus left",
            "fullscreen enable nope",
            "exec",
        ] {
            let error = parse(input).into_iter().next().unwrap().unwrap_err();
            assert!(!error.success, "{input}");
            assert_eq!(error.parse_error, Some(true), "{input}");
            assert!(error.error.is_some(), "{input}");
        }
    }
}

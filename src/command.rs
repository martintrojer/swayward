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
    Layout(Layout),
    Split(Option<Layout>),
    Fullscreen {
        mode: Toggle,
        global: bool,
    },
    Floating(Toggle),
    Workspace(WorkspaceTarget),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    pub command: Command,
    pub criteria: Option<String>,
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

        if criteria.is_none() && text.starts_with('[') {
            match criteria_end(text) {
                Some(end) => {
                    criteria = Some(text[..=end].to_owned());
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
        "workspace" => parse_workspace(rest).map(Command::Workspace),
        "kill" => no_args(rest, "kill").map(|()| Command::Kill),
        "resize" => parse_resize(rest),
        "reload" => no_args(rest, "reload").map(|()| Command::Reload),
        "nop" => Ok(Command::Nop),
        "exec" | "exec_always" => parse_exec(input, name),
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

pub fn execute(state: &mut State, input: &str) -> Vec<CommandOutcome> {
    parse(input)
        .into_iter()
        .map(|parsed| match parsed {
            Ok(parsed) => execute_one(state, parsed),
            Err(error) => error,
        })
        .collect()
}

fn execute_one(state: &mut State, parsed: ParsedCommand) -> CommandOutcome {
    if parsed.criteria.is_some() {
        return failure("criteria are not implemented yet");
    }

    let action = match parsed.command {
        Command::FocusDirection(Direction::Left) => Some(Action::FocusColumnLeft),
        Command::FocusDirection(Direction::Right) => Some(Action::FocusColumnRight),
        Command::FocusDirection(Direction::Up) => Some(Action::FocusWindowUp),
        Command::FocusDirection(Direction::Down) => Some(Action::FocusWindowDown),
        Command::FocusParent | Command::FocusChild => {
            return failure("container focus is not implemented yet");
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
        Command::MoveToWorkspace(_) => {
            return failure("global workspace movement is not implemented yet");
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
        Command::Workspace(_) => return failure("global workspaces are not implemented yet"),
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
    };

    if let Some(action) = action {
        state.do_action(action, false);
    }
    success()
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

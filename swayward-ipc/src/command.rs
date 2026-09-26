use crate::CommandOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XkbLayoutTarget {
    Next,
    Prev,
    Index(u32),
}

/// A session-wide layout setting changed at runtime.
///
/// Each variant names a sway directive that swayward also accepts in KDL. The
/// string is validated by the config crate's own `FromStr`, so IPC and the
/// config file accept exactly the same values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutOption {
    FocusWrapping(String),
    ForceFocusWrapping(String),
    WorkspaceLayout(String),
    DefaultOrientation(String),
    HideEdgeBorders(String),
    SmartBorders(String),
    SmartGaps(String),
    ShowMarks(String),
    TitleAlignment(String),
    TilingDrag(String),
    TilingDragThreshold(u32),
    ForceDisplayUrgencyHint(u32),
    PrimarySelection(bool),
    FocusOnWindowActivation(String),
    /// `focus_follows_mouse no|yes|always`.
    ///
    /// Sway stores three distinct states and `always` is not `yes`: it
    /// re-focuses the hovered window even when the hovered window did not
    /// change (`sway/sway/input/seatop_default.c:590-598`).
    FocusFollowsMouse(FocusFollowsMouse),
    WorkspaceAutoBackAndForth(String),
    FloatingMinimumSize(i32, i32),
    FloatingMaximumSize(i32, i32),
    TitlebarFont {
        font: String,
        pango_markup: bool,
    },
    TitlebarPadding {
        horizontal: i32,
        vertical: i32,
    },
    TitlebarBorderThickness(u16),
    /// `xwayland <enable|disable|force>`.
    ///
    /// Sway accepts the command but refuses a change that would take effect
    /// after startup, answering "xwayland can only be enabled/disabled at
    /// launch" (`sway/sway/commands/xwayland.c:7-36`). Setting the value it
    /// already has succeeds.
    Xwayland {
        enabled: bool,
    },
    /// `mouse_warping output|container|none`.
    ///
    /// Sway keeps the three modes apart: `output` warps only when the focused
    /// target sits on an output that does not contain the pointer, while
    /// `container` warps on every qualifying focus change
    /// (`sway/sway/input/seat.c:1526-1547`).
    MouseWarping(MouseWarping),
    PopupDuringFullscreen(String),
    /// `floating_modifier <mod> [inverse|normal]`.
    ///
    /// The modifier and the inverse bit are independent pieces of state in
    /// sway, and `none` is a value rather than a key name
    /// (`sway/sway/commands/floating_modifier.c:11-32`).
    FloatingModifier {
        /// `None` is sway's `none`, which disables the drag.
        modifier: Option<String>,
        inverse: bool,
    },
    /// `default_border` / `default_floating_border`, and the deprecated
    /// `new_window` / `new_float` spellings sway still accepts.
    DefaultBorder {
        floating: bool,
        style: String,
        width: Option<u16>,
    },
}

/// Sway's three `focus_follows_mouse` states
/// (`sway/include/sway/config.h:458-462`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusFollowsMouse {
    No,
    Yes,
    Always,
}

/// Sway's three `mouse_warping` states
/// (`sway/include/sway/config.h:471-475`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseWarping {
    No,
    Output,
    Container,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientColorClass {
    Focused,
    FocusedInactive,
    FocusedTabTitle,
    Unfocused,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientColors {
    pub border: [u8; 4],
    pub background: [u8; 4],
    pub text: [u8; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapTarget {
    Id(String),
    ConId(String),
    Mark(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignmentTarget {
    Workspace(String),
    WorkspaceNumber(String),
    Output(String),
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

#[derive(Debug, Clone, PartialEq)]
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
    MoveToWorkspace {
        target: WorkspaceTarget,
        auto_back_and_forth: bool,
    },
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
    Urgent(String),
    Border(Border),
    TitleFormat(String),
    Sticky(String),
    ShortcutsInhibitor(bool),
    /// A sway directive that sets a layout option for the whole session.
    ///
    /// Sway serves the config file and IPC from one command table
    /// (`sway/sway/commands.c:162-173`), so these are runtime commands there
    /// as well as config lines. swayward keeps the setting in KDL and applies
    /// the same value here, then re-runs the normal config apply path.
    SetLayoutOption(LayoutOption),
    SetClientColors {
        class: ClientColorClass,
        colors: ClientColors,
    },
    Swap(SwapTarget),
    Workspace {
        target: WorkspaceTarget,
        auto_back_and_forth: bool,
    },
    AssignWorkspace {
        target: WorkspaceTarget,
        /// Sway accepts a LIST and uses the first output that resolves
        /// (`sway/sway/commands/workspace.c:153-155`;
        /// `sway/sway/tree/workspace.c:244-250`). Never empty.
        outputs: Vec<String>,
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
    ResizeSet {
        width: Option<ResizeAmount>,
        height: Option<ResizeAmount>,
    },
    Reload,
    Exit,
    CreateOutput,
    InputSwitchLayout {
        identifier: String,
        target: XkbLayoutTarget,
    },
    Output {
        target: String,
        actions: Vec<crate::OutputAction>,
    },
    Gaps {
        inner: bool,
        sides: [bool; 4],
        all: bool,
        operation: GapOperation,
        amount: i32,
    },
    /// `gaps <kind> <px>`: sway's two-argument form, which sets the DEFAULT for
    /// workspaces created later and leaves existing ones alone
    /// (`sway/sway/commands/gaps.c:48-91`). Distinct state from [`Command::Gaps`],
    /// which mutates live workspaces.
    GapsDefaults {
        inner: bool,
        sides: [bool; 4],
        amount: i32,
    },
    /// `workspace <name> gaps <kind> <px>`: a per-workspace-name default, applied
    /// when a workspace of that name is created
    /// (`sway/sway/commands/workspace.c:57-117`; `sway/sway/tree/workspace.c:224-242`).
    WorkspaceGaps {
        name: String,
        inner: bool,
        sides: [bool; 4],
        amount: i32,
    },
    /// `set $name value`: define or replace a runtime variable
    /// (`sway/sway/commands/set.c:26-57`).
    Set {
        name: String,
        value: String,
    },
    Bind {
        key: String,
        command: Option<String>,
        keycode: bool,
        release: bool,
        locked: bool,
        inhibited: bool,
        no_repeat: bool,
        input_device: String,
    },
    SwitchBind {
        switch: String,
        command: Option<String>,
        locked: bool,
    },
    Mode {
        name: String,
        pango_markup: bool,
        subcommand: Option<Box<Command>>,
    },
    Nop,
    Exec {
        command: String,
        no_startup_id: bool,
    },
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
    Assign {
        criteria: String,
        target: AssignmentTarget,
    },
    NoFocus {
        criteria: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapOperation {
    Set,
    Plus,
    Minus,
    Toggle,
}

#[derive(Debug, Clone, PartialEq)]
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
pub fn expand_variables(input: &str, variables: &[(String, String)]) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            out.push(input[i..].chars().next().unwrap());
            i += input[i..].chars().next().unwrap().len_utf8();
            continue;
        }
        // An escaped `$` keeps its backslash; sway leaves both in place here
        // and strips the escape later with the quotes.
        if i > 0 && bytes[i - 1] == b'\\' {
            out.push('$');
            i += 1;
            continue;
        }
        // `$$` unescapes to one `$`.
        if bytes.get(i + 1) == Some(&b'$') {
            out.push('$');
            i += 2;
            continue;
        }
        match variables
            .iter()
            .find(|(name, _)| input[i..].starts_with(name.as_str()))
        {
            Some((name, value)) => {
                out.push_str(value);
                i += name.len();
            }
            None => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

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
                    results.push(Err(parse_error("unterminated criteria")));
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
pub fn set_variable(variables: &mut Vec<(String, String)>, name: String, value: String) {
    match variables.iter_mut().find(|(existing, _)| *existing == name) {
        Some(slot) => slot.1 = value,
        None => variables.push((name, value)),
    }
    variables.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
}

fn parse_one_with_variables(
    input: &str,
    variables: &[(String, String)],
) -> Result<Command, String> {
    let words = words(input).map_err(str::to_owned)?;
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
                expand_variables(word, variables)
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
            ';' | ',' | '\0' if brackets == 0 => {
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
    parse_words(&args, input)
}

fn parse_words(args: &[&str], input: &str) -> Result<Command, String> {
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
        "opacity" => Err("opacity requires mutable per-container opacity support".into()),
        "inhibit_idle" => Err("inhibit_idle requires user inhibitor policy support".into()),
        "create_output" => no_args(rest, "create_output").map(|()| Command::CreateOutput),
        "input" => parse_input_command(rest),
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
        name @ ("client.focused"
        | "client.focused_inactive"
        | "client.focused_tab_title"
        | "client.unfocused"
        | "client.urgent") => parse_client_colors(name, rest),
        "focus_wrapping" => match rest {
            // `sway/sway/commands/focus_wrapping.c`: force and workspace are
            // literal, everything else goes through parse_boolean.
            [value] => {
                let value = value.to_ascii_lowercase();
                let mapped = match value.as_str() {
                    "force" => "force",
                    "workspace" => "workspace",
                    "toggle" => "toggle",
                    other => {
                        if parse_boolean(other, false) {
                            "yes"
                        } else {
                            "no"
                        }
                    }
                };
                Ok(Command::SetLayoutOption(LayoutOption::FocusWrapping(
                    mapped.to_owned(),
                )))
            }
            _ => Err("Expected 'focus_wrapping yes|no|force|workspace'".into()),
        },
        "force_focus_wrapping" => match rest {
            // Deprecated in sway, which keeps it as a boolean alias selecting
            // between force and yes (`sway/sway/commands/force_focus_wrapping.c`).
            [value] => Ok(Command::SetLayoutOption(LayoutOption::ForceFocusWrapping(
                value.to_ascii_lowercase(),
            ))),
            _ => Err("Expected 'force_focus_wrapping <yes|no>'".into()),
        },
        "workspace_layout" => match rest {
            [value]
                if matches!(
                    value.to_ascii_lowercase().as_str(),
                    "default" | "stacking" | "tabbed"
                ) =>
            {
                Ok(Command::SetLayoutOption(LayoutOption::WorkspaceLayout(
                    value.to_ascii_lowercase(),
                )))
            }
            _ => Err("Expected 'workspace_layout <default|stacking|tabbed>'".into()),
        },
        "default_orientation" | "orientation" => match rest {
            [value]
                if matches!(
                    value.to_ascii_lowercase().as_str(),
                    "horizontal" | "vertical" | "auto"
                ) =>
            {
                Ok(Command::SetLayoutOption(LayoutOption::DefaultOrientation(
                    value.to_ascii_lowercase(),
                )))
            }
            _ => Err("Expected 'orientation <horizontal|vertical|auto>'".into()),
        },
        "hide_edge_borders" => {
            // `sway/sway/commands/hide_edge_borders.c` accepts an --i3 flag
            // before the value; it selects i3's smart behaviour, which
            // swayward expresses through smart_borders.
            let rest: Vec<&str> = rest.iter().copied().filter(|a| *a != "--i3").collect();
            match rest.as_slice() {
                [value]
                    if matches!(
                        *value,
                        "none" | "vertical" | "horizontal" | "both" | "smart" | "smart_no_gaps"
                    ) =>
                {
                    // smart and smart_no_gaps are the smart-border toggle in
                    // sway, not edge-border values.
                    let option = match *value {
                        "smart" => LayoutOption::SmartBorders("on".to_owned()),
                        "smart_no_gaps" => LayoutOption::SmartBorders("no-gaps".to_owned()),
                        other => LayoutOption::HideEdgeBorders(other.to_owned()),
                    };
                    Ok(Command::SetLayoutOption(option))
                }
                _ => Err("Expected 'hide_edge_borders [--i3] \
                          none|vertical|horizontal|both|smart|smart_no_gaps'"
                    .into()),
            }
        }
        "smart_borders" => match rest {
            [value] => {
                let value = value.to_ascii_lowercase();
                // sway writes no_gaps; the KDL spelling is no-gaps, and the
                // config crate's FromStr is the single validator.
                let mapped = if value == "no_gaps" || value == "no-gaps" {
                    "no-gaps"
                } else if parse_boolean(&value, true) {
                    "on"
                } else {
                    "off"
                };
                Ok(Command::SetLayoutOption(LayoutOption::SmartBorders(
                    mapped.to_owned(),
                )))
            }
            _ => Err("Expected 'smart_borders on|no_gaps|off'".into()),
        },
        "smart_gaps" => match rest {
            [value] => {
                let value = value.to_ascii_lowercase();
                let mapped = match value.as_str() {
                    "inverse_outer" => "inverse-outer",
                    "toggle" => "toggle",
                    other if parse_boolean(other, true) => "on",
                    _ => "off",
                };
                Ok(Command::SetLayoutOption(LayoutOption::SmartGaps(
                    mapped.into(),
                )))
            }
            _ => Err("Expected 'smart_gaps on|off|toggle|inverse_outer'".into()),
        },
        "show_marks" => match rest.split_first() {
            Some((value, _)) => Ok(Command::SetLayoutOption(LayoutOption::ShowMarks(
                value.to_ascii_lowercase(),
            ))),
            None => Err("Expected 'show_marks yes|no'".into()),
        },
        "title_align" => match rest {
            [value] if matches!(*value, "left" | "center" | "right") => Ok(
                Command::SetLayoutOption(LayoutOption::TitleAlignment((*value).into())),
            ),
            _ => Err("Expected 'title_align <left|center|right>'".into()),
        },
        "tiling_drag" => match rest {
            [value] => Ok(Command::SetLayoutOption(LayoutOption::TilingDrag(
                value.to_ascii_lowercase(),
            ))),
            _ => Err("Expected 'tiling_drag enable|disable|toggle'".into()),
        },
        "tiling_drag_threshold" => match rest {
            [value] => value
                .parse()
                .map(LayoutOption::TilingDragThreshold)
                .map(Command::SetLayoutOption)
                .map_err(|_| "Invalid threshold specified".into()),
            _ => Err("Expected 'tiling_drag_threshold <threshold>'".into()),
        },
        "force_display_urgency_hint" => {
            let value = match rest {
                [value] | [value, "ms"] => value.trim_end_matches("ms"),
                _ => return Err("Expected 'force_display_urgency_hint <timeout> [ms]'".into()),
            };
            let value: i64 = value
                .parse()
                .map_err(|_| "timeout integer invalid".to_owned())?;
            Ok(Command::SetLayoutOption(
                LayoutOption::ForceDisplayUrgencyHint(value.max(0).min(i64::from(u32::MAX)) as u32),
            ))
        }
        "primary_selection" => match rest {
            [value] => Ok(Command::SetLayoutOption(LayoutOption::PrimarySelection(
                parse_boolean(value, true),
            ))),
            _ => Err("Expected 'primary_selection enabled|disabled'".into()),
        },
        "focus_on_window_activation" => match rest {
            [value] if matches!(*value, "smart" | "urgent" | "focus" | "none") => Ok(
                Command::SetLayoutOption(LayoutOption::FocusOnWindowActivation((*value).into())),
            ),
            _ => Err("Expected 'focus_on_window_activation smart|urgent|focus|none'".into()),
        },
        "focus_follows_mouse" => match rest {
            // `sway/sway/commands/focus_follows_mouse.c:9-18` compares with
            // strcmp, so the three names are case-sensitive, and it rejects
            // anything else rather than coercing.
            ["no"] => Ok(Command::SetLayoutOption(LayoutOption::FocusFollowsMouse(
                FocusFollowsMouse::No,
            ))),
            ["yes"] => Ok(Command::SetLayoutOption(LayoutOption::FocusFollowsMouse(
                FocusFollowsMouse::Yes,
            ))),
            ["always"] => Ok(Command::SetLayoutOption(LayoutOption::FocusFollowsMouse(
                FocusFollowsMouse::Always,
            ))),
            _ => Err("Expected 'focus_follows_mouse no|yes|always'".into()),
        },
        "workspace_auto_back_and_forth" => match rest {
            [value] => Ok(Command::SetLayoutOption(
                LayoutOption::WorkspaceAutoBackAndForth(value.to_ascii_lowercase()),
            )),
            _ => Err("Expected 'workspace_auto_back_and_forth <yes|no>'".into()),
        },
        name @ ("default_border" | "default_floating_border" | "new_window" | "new_float") => {
            // `sway/sway/commands/default_border.c`: a style, then an
            // optional width that sway reads with atoi and only for `pixel`
            // and `normal`. new_window and new_float are the older i3
            // spellings of the same two settings.
            let usage =
                format!("Expected '{name} <none|normal|pixel>' or '{name} <normal|pixel> <px>'");
            let (style, width) = match rest {
                [style] => (style, None),
                [style, width] => {
                    let width: u16 = width.parse().map_err(|_| usage.clone())?;
                    (style, Some(width))
                }
                _ => return Err(usage),
            };
            let style = style.to_ascii_lowercase();
            if !matches!(style.as_str(), "none" | "normal" | "pixel") {
                return Err(usage);
            }
            Ok(Command::SetLayoutOption(LayoutOption::DefaultBorder {
                floating: matches!(name, "default_floating_border" | "new_float"),
                style,
                width,
            }))
        }
        "popup_during_fullscreen" => match rest {
            [value]
                if matches!(
                    value.to_ascii_lowercase().as_str(),
                    "smart" | "ignore" | "leave_fullscreen"
                ) =>
            {
                Ok(Command::SetLayoutOption(
                    LayoutOption::PopupDuringFullscreen(value.to_ascii_lowercase()),
                ))
            }
            _ => Err("Expected 'popup_during_fullscreen smart|ignore|leave_fullscreen'".into()),
        },
        "floating_modifier" => {
            // `sway/sway/commands/floating_modifier.c:6-32`: at least one
            // argument, then an optional normal|inverse. `none` returns
            // before the second argument is read, so a trailing word is
            // ignored for it. Extra arguments past the second are ignored too
            // because the check is EXPECTED_AT_LEAST.
            const USAGE: &str = "Usage: floating_modifier <mod> [inverse|normal]";
            let Some((modifier, mode)) = rest.split_first() else {
                return Err(
                    "Invalid floating_modifier command (expected at least 1 argument, got 0)"
                        .into(),
                );
            };
            if modifier.eq_ignore_ascii_case("none") {
                return Ok(Command::SetLayoutOption(LayoutOption::FloatingModifier {
                    modifier: None,
                    inverse: false,
                }));
            }
            // `sway/sway/input/keyboard.c:27-39` is the whole accepted set,
            // matched case-insensitively: Shift, Lock, Control, Ctrl, Alt,
            // Mod1..Mod5 and Super. A $mod variable is expanded before the
            // command is parsed, so only these literals arrive here.
            let lowered = modifier.to_ascii_lowercase();
            let name = match lowered.as_str() {
                "shift" => "shift",
                "control" | "ctrl" => "ctrl",
                "alt" | "mod1" => "alt",
                "super" | "mod4" => "super",
                "mod3" => "iso_level5_shift",
                "mod5" => "iso_level3_shift",
                // Sway maps these to Caps Lock and Num Lock, which swayward's
                // ModKey cannot name. Refused rather than silently dropped to
                // a different modifier.
                "lock" | "mod2" => {
                    return Err(format!(
                        "swayward cannot use {modifier} as a floating modifier because it has no \
                         lock-modifier mod key"
                    ));
                }
                // Sway validates the modifier before the mode, so an invalid
                // modifier wins over an invalid trailing word.
                _ => return Err("Invalid modifier".into()),
            };
            let inverse = match mode.first() {
                None => false,
                Some(mode) if mode.eq_ignore_ascii_case("normal") => false,
                Some(mode) if mode.eq_ignore_ascii_case("inverse") => true,
                Some(_) => return Err(USAGE.into()),
            };
            Ok(Command::SetLayoutOption(LayoutOption::FloatingModifier {
                modifier: Some(name.to_owned()),
                inverse,
            }))
        }
        "mouse_warping" => match rest {
            // `sway/sway/commands/mouse_warping.c:9-16` uses strcasecmp, so
            // these three are case-insensitive, unlike focus_follows_mouse.
            [value] => {
                let mode = if value.eq_ignore_ascii_case("output") {
                    MouseWarping::Output
                } else if value.eq_ignore_ascii_case("container") {
                    MouseWarping::Container
                } else if value.eq_ignore_ascii_case("none") {
                    MouseWarping::No
                } else {
                    return Err("Expected 'mouse_warping output|container|none'".into());
                };
                Ok(Command::SetLayoutOption(LayoutOption::MouseWarping(mode)))
            }
            _ => Err("Expected 'mouse_warping output|container|none'".into()),
        },
        "xwayland" => match rest {
            [value] => Ok(Command::SetLayoutOption(LayoutOption::Xwayland {
                // sway treats `force` as enabled-immediately and routes
                // everything else through parse_boolean with a true default.
                enabled: *value == "force" || parse_boolean(value, true),
            })),
            _ => Err("Invalid xwayland command (expected 1 argument, got 0)".into()),
        },
        "font" => {
            // `sway/sway/commands/font.c` joins the remaining words and strips
            // a leading `pango:`, then reparses the description.
            if rest.is_empty() {
                return Err("Expected 'font <font>'".into());
            }
            let font = join_words(rest);
            let (font, pango_markup) = font
                .strip_prefix("pango:")
                .map_or((font.as_str(), false), |font| (font, true));
            Ok(Command::SetLayoutOption(LayoutOption::TitlebarFont {
                font: font.to_owned(),
                pango_markup,
            }))
        }
        "titlebar_border_thickness" => {
            const INVALID: &str = "Invalid size specified";
            let [value] = rest else {
                return Err(format!(
                    "Invalid titlebar_border_thickness command (expected 1 argument, got {})",
                    rest.len()
                ));
            };
            let value = value.parse().map_err(|_| INVALID.to_owned())?;
            Ok(Command::SetLayoutOption(
                LayoutOption::TitlebarBorderThickness(value),
            ))
        }
        "titlebar_padding" => {
            // One value sets both axes; two set horizontal then vertical.
            // Negatives are rejected, matching sway's `Invalid size specified`
            // (`sway/sway/commands/titlebar_padding.c:8-38`).
            const INVALID: &str = "Invalid size specified";
            let (horizontal, vertical) = match rest {
                [h] => {
                    let h: i32 = h.parse().map_err(|_| INVALID.to_owned())?;
                    (h, h)
                }
                [h, v] => (
                    h.parse().map_err(|_| INVALID.to_owned())?,
                    v.parse().map_err(|_| INVALID.to_owned())?,
                ),
                _ => return Err("Expected 'titlebar_padding <horizontal> [<vertical>]'".into()),
            };
            if horizontal < 0 || vertical < 0 {
                return Err(INVALID.into());
            }
            Ok(Command::SetLayoutOption(LayoutOption::TitlebarPadding {
                horizontal,
                vertical,
            }))
        }
        name @ ("floating_minimum_size" | "floating_maximum_size") => {
            // `sway/sway/commands/floating_minmax_size.c` wants exactly three
            // words, with a literal `x` between two integers, and rejects a
            // trailing suffix because it uses strtol and checks the remainder.
            let usage = format!("Expected '{name} <width> x <height>'");
            let [width, "x", height] = rest else {
                return Err(usage);
            };
            let (Ok(width), Ok(height)) = (width.parse::<i32>(), height.parse::<i32>()) else {
                return Err(usage);
            };
            Ok(Command::SetLayoutOption(
                if name == "floating_minimum_size" {
                    LayoutOption::FloatingMinimumSize(width, height)
                } else {
                    LayoutOption::FloatingMaximumSize(width, height)
                },
            ))
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

fn parse_input_command(args: &[&str]) -> Result<Command, String> {
    let [identifier, "xkb_switch_layout", target] = args else {
        return Err(
            "only input <identifier> xkb_switch_layout <next|prev|index> is supported".into(),
        );
    };
    let target = match *target {
        "next" => XkbLayoutTarget::Next,
        "prev" => XkbLayoutTarget::Prev,
        value => {
            let index: i64 = value.parse().map_err(|_| "Invalid argument.")?;
            if index < 0 {
                return Err("Invalid layout index.".into());
            }
            XkbLayoutTarget::Index(index.try_into().map_err(|_| "Invalid argument.")?)
        }
    };
    Ok(Command::InputSwitchLayout {
        identifier: (*identifier).to_owned(),
        target,
    })
}

fn parse_output_command(args: &[&str]) -> Result<Command, String> {
    let Some((target, mut args)) = args.split_first() else {
        return Err("Expected 'output <name> <subcommand>'".into());
    };
    if args.is_empty() {
        return Err("Expected 'output <name> <subcommand>'".into());
    }

    let mut actions = Vec::new();
    while let Some((name, rest)) = args.split_first() {
        let (action, consumed) = match name.to_ascii_lowercase().as_str() {
            "enable" => (crate::OutputAction::On, 0),
            "disable" => (crate::OutputAction::Off, 0),
            "mode" | "res" | "resolution" => {
                let (custom, rest) = match rest {
                    [flag, rest @ ..] if *flag == "--custom" => (true, rest),
                    rest => (false, rest),
                };
                let Some(value) = rest.first() else {
                    return Err("Missing mode argument.".into());
                };
                let (mode, consumed) = if value.contains('x') {
                    let value = ["Hz", "HZ", "hz", "hZ"]
                        .into_iter()
                        .find_map(|suffix| value.strip_suffix(suffix))
                        .unwrap_or(value);
                    (
                        value
                            .parse::<crate::ConfiguredMode>()
                            .map_err(str::to_owned)?,
                        1,
                    )
                } else {
                    let Some(height) = rest.get(1) else {
                        return Err("Missing mode argument (height).".into());
                    };
                    (
                        crate::ConfiguredMode {
                            width: value.parse().map_err(|_| "Invalid mode width.")?,
                            height: height.parse().map_err(|_| "Invalid mode height.")?,
                            refresh: None,
                        },
                        2,
                    )
                };
                let action = if custom {
                    crate::OutputAction::CustomMode { mode }
                } else {
                    crate::OutputAction::Mode {
                        mode: crate::ModeToSet::Specific(mode),
                    }
                };
                (action, usize::from(custom) + consumed)
            }
            "scale" => {
                let Some(value) = rest.first() else {
                    return Err("Missing scale argument.".into());
                };
                let scale = value
                    .parse::<f64>()
                    .map_err(|_| "Invalid scale.".to_owned())?;
                if !scale.is_finite() || scale <= 0. {
                    return Err("Invalid scale.".into());
                }
                (
                    crate::OutputAction::Scale {
                        scale: crate::ScaleToSet::Specific(scale),
                    },
                    1,
                )
            }
            "transform" => {
                let Some(value) = rest.first() else {
                    return Err("Missing transform argument.".into());
                };
                let transform = if *value == "0" {
                    crate::Transform::Normal
                } else {
                    value.parse().map_err(|_| "Invalid output transform.")?
                };
                let transform = match transform {
                    crate::Transform::_90 => crate::Transform::_270,
                    crate::Transform::_270 => crate::Transform::_90,
                    crate::Transform::Flipped90 => crate::Transform::Flipped270,
                    crate::Transform::Flipped270 => crate::Transform::Flipped90,
                    transform => transform,
                };
                (crate::OutputAction::Transform { transform }, 1)
            }
            "position" | "pos" => {
                let Some(value) = rest.first() else {
                    return Err("Missing position argument.".into());
                };
                let (x, y, consumed) = if let Some((x, y)) = value.split_once(',') {
                    (x, y, 1)
                } else {
                    let Some(y) = rest.get(1) else {
                        return Err("Missing position argument (y).".into());
                    };
                    (*value, *y, 2)
                };
                let position = crate::ConfiguredPosition {
                    x: x.parse().map_err(|_| "Invalid position x.")?,
                    y: y.parse().map_err(|_| "Invalid position y.")?,
                };
                (
                    crate::OutputAction::Position {
                        position: crate::PositionToSet::Specific(position),
                    },
                    consumed,
                )
            }
            "adaptive_sync" => {
                let Some(value) = rest.first() else {
                    return Err("Missing adaptive_sync argument".into());
                };
                if value.eq_ignore_ascii_case("toggle") {
                    return Err(if *target == "*" {
                        "Cannot apply toggle to all outputs"
                    } else {
                        "adaptive_sync toggle is not implemented"
                    }
                    .into());
                }
                (
                    crate::OutputAction::Vrr {
                        vrr: crate::VrrToSet {
                            vrr: parse_boolean(value, true),
                            on_demand: false,
                        },
                    },
                    1,
                )
            }
            "render_bit_depth" => {
                let Some(value) = rest.first() else {
                    return Err("Missing bit depth argument.".into());
                };
                let max_bpc = match *value {
                    "6" => crate::MaxBpc::_6,
                    "8" => crate::MaxBpc::_8,
                    "10" => crate::MaxBpc::_10,
                    _ => return Err("Invalid bit depth. Must be a value in (6|8|10).".into()),
                };
                (crate::OutputAction::MaxBpc { max_bpc }, 1)
            }
            "modeline" => {
                let [clock, hdisplay, hsync_start, hsync_end, htotal, vdisplay, vsync_start, vsync_end, vtotal, hsync_polarity, vsync_polarity, ..] =
                    rest
                else {
                    return Err("Invalid modeline".into());
                };
                let action = crate::OutputAction::Modeline {
                    clock: clock.parse().map_err(|_| "Invalid modeline")?,
                    hdisplay: hdisplay.parse().map_err(|_| "Invalid modeline")?,
                    hsync_start: hsync_start.parse().map_err(|_| "Invalid modeline")?,
                    hsync_end: hsync_end.parse().map_err(|_| "Invalid modeline")?,
                    htotal: htotal.parse().map_err(|_| "Invalid modeline")?,
                    vdisplay: vdisplay.parse().map_err(|_| "Invalid modeline")?,
                    vsync_start: vsync_start.parse().map_err(|_| "Invalid modeline")?,
                    vsync_end: vsync_end.parse().map_err(|_| "Invalid modeline")?,
                    vtotal: vtotal.parse().map_err(|_| "Invalid modeline")?,
                    hsync_polarity: hsync_polarity
                        .to_ascii_lowercase()
                        .parse()
                        .map_err(str::to_owned)?,
                    vsync_polarity: vsync_polarity
                        .to_ascii_lowercase()
                        .parse()
                        .map_err(str::to_owned)?,
                };
                (action, 11)
            }
            "power" | "dpms" => {
                let Some(value) = rest.first() else {
                    return Err("Missing power argument".into());
                };
                let power = parse_boolean_toggle(value);
                if *target == "*" && power == Toggle::Toggle {
                    return Err("Cannot apply toggle to all outputs".into());
                }
                (crate::OutputAction::Power { power }, 1)
            }
            _ => return Err(format!("Invalid output subcommand: {name}.")),
        };
        action.validate()?;
        actions.push(action);
        args = &rest[consumed..];
    }

    Ok(Command::Output {
        target: (*target).to_owned(),
        actions,
    })
}

fn parse_client_colors(name: &str, args: &[&str]) -> Result<Command, String> {
    if args.len() < 3 {
        return Err(format!(
            "Invalid {name} command (expected at least 3 arguments, got {})",
            args.len()
        ));
    }
    if args.len() > 5 {
        return Err(format!(
            "Invalid {name} command (expected at most 5 arguments, got {})",
            args.len()
        ));
    }

    let default_indicator = match name {
        "client.focused" | "client.focused_tab_title" => "#2e9ef4ff",
        "client.focused_inactive" => "#484e50ff",
        "client.unfocused" => "#292d2eff",
        "client.urgent" => "#900000ff",
        _ => unreachable!(),
    };
    let properties = [
        ("border", args[0]),
        ("background", args[1]),
        ("text", args[2]),
        (
            "indicator",
            args.get(3).copied().unwrap_or(default_indicator),
        ),
        ("child_border", args.get(4).copied().unwrap_or(args[1])),
    ];
    let mut parsed = [[0; 4]; 5];
    for (index, (property, value)) in properties.into_iter().enumerate() {
        parsed[index] =
            parse_sway_color(value).ok_or_else(|| format!("Invalid {property} color {value}"))?;
    }

    if name != "client.focused_tab_title" {
        return Err(
            "client colour commands are unsupported because sway window-border colours are not fully rendered"
                .into(),
        );
    }

    Ok(Command::SetClientColors {
        class: ClientColorClass::FocusedTabTitle,
        colors: ClientColors {
            border: parsed[0],
            background: parsed[1],
            text: parsed[2],
        },
    })
}

fn parse_sway_color(value: &str) -> Option<[u8; 4]> {
    let value = value.strip_prefix('#').unwrap_or(value);
    if !matches!(value.len(), 6 | 8) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let parsed = u32::from_str_radix(value, 16).ok()?;
    let rgba = if value.len() == 6 {
        (parsed << 8) | 0xff
    } else {
        parsed
    };
    Some(rgba.to_be_bytes())
}

/// Sway's error text for the two-argument defaults form
/// (`sway/sway/commands/gaps.c:46-47`).
const GAPS_EXPECTED_DEFAULTS: &str =
    "'gaps inner|outer|horizontal|vertical|top|right|bottom|left <px>'";
/// Sway's error text for the four-argument runtime form
/// (`sway/sway/commands/gaps.c:134-136`).
const GAPS_EXPECTED_RUNTIME: &str = "'gaps inner|outer|horizontal|vertical|top|right|bottom|left \
     current|all set|plus|minus|toggle <px>'";

/// Sides are ordered `[left, right, top, bottom]` to match
/// [`swayward_config::OuterGaps`]. Sway sets each side independently, so
/// `horizontal` and `vertical` select pairs rather than a distinct kind
/// (`sway/sway/commands/gaps.c:62-84`).
fn parse_gaps_kind(kind: &str) -> Option<(bool, [bool; 4])> {
    Some(match kind.to_ascii_lowercase().as_str() {
        "inner" => (true, [false; 4]),
        "outer" => (false, [true; 4]),
        "horizontal" => (false, [true, true, false, false]),
        "vertical" => (false, [false, false, true, true]),
        "left" => (false, [true, false, false, false]),
        "right" => (false, [false, true, false, false]),
        "top" => (false, [false, false, true, false]),
        "bottom" => (false, [false, false, false, true]),
        _ => return None,
    })
}

/// Sway parses with `strtol` and accepts a bare number or a `px` suffix,
/// rejecting any other trailing text (`sway/sway/commands/gaps.c:55-58`).
fn parse_gaps_amount(raw: &str) -> Option<i32> {
    let digits = raw
        .strip_suffix("px")
        .or_else(|| raw.strip_suffix("PX"))
        .or_else(|| raw.strip_suffix("Px"))
        .or_else(|| raw.strip_suffix("pX"))
        .unwrap_or(raw);
    digits.parse().ok()
}

/// `set $name value...`
///
/// Sway requires at least two arguments and a leading `$`
/// (`sway/sway/commands/set.c:27-34`), then joins the remainder as the value.
fn parse_switch_bind_command(args: &[&str], unbind: bool) -> Result<Command, String> {
    let name = if unbind { "unbindswitch" } else { "bindswitch" };
    let minimum = if unbind { 1 } else { 2 };
    if args.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} arguments, got {})",
            args.len()
        ));
    }
    let mut locked = false;
    let mut index = 0;
    while let Some(option) = args.get(index).filter(|arg| arg.starts_with("--")) {
        match *option {
            "--locked" => locked = true,
            "--no-warn" => {}
            // `--reload` is not part of switch-binding identity, so sway's
            // unbind accepts and ignores it. Adding such a binding would
            // require replaying tracked switch state during reload.
            "--reload" if unbind => {}
            "--reload" => {
                return Err(
                    "bindswitch --reload requires tracked per-device switch state during reload"
                        .into(),
                )
            }
            option => return Err(format!("unsupported {name} option {option}")),
        }
        index += 1;
    }
    let remaining = &args[index..];
    if remaining.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} non-option arguments, got {})",
            remaining.len()
        ));
    }
    let combo = join_words(&remaining[..1]);
    let Some((switch, state)) = combo.split_once(':') else {
        return Err(format!(
            "Invalid {name} command (expected binding with the form <switch>:<state>)"
        ));
    };
    if !matches!(switch, "lid" | "tablet") {
        return Err(format!(
            "Invalid {name} command (expected switch binding: unknown switch {switch})"
        ));
    }
    if !matches!(state, "on" | "off" | "toggle") {
        return Err(format!(
            "Invalid {name} command (expected switch state: unknown state {state})"
        ));
    }
    Ok(Command::SwitchBind {
        switch: combo,
        command: (!unbind).then(|| join_words(&remaining[1..])),
        locked,
    })
}

fn parse_bind_command(args: &[&str], keycode: bool, unbind: bool) -> Result<Command, String> {
    let name = match (keycode, unbind) {
        (false, false) => "bindsym",
        (false, true) => "unbindsym",
        (true, false) => "bindcode",
        (true, true) => "unbindcode",
    };
    let minimum = if unbind { 1 } else { 2 };
    if args.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} arguments, got {})",
            args.len()
        ));
    }

    let mut release = false;
    let mut locked = false;
    let mut inhibited = false;
    let mut no_repeat = false;
    let mut input_device = "*".to_owned();
    let mut index = 0;
    while let Some(option) = args.get(index).filter(|arg| arg.starts_with("--")) {
        match *option {
            "--release" => release = true,
            "--locked" => locked = true,
            "--inhibited" => inhibited = true,
            "--no-repeat" => no_repeat = true,
            "--no-warn" => {}
            option if option.starts_with("--input-device=") => {
                input_device = unquote(&option["--input-device=".len()..]).to_owned();
            }
            // These sway forms target mouse regions or translate keysyms via
            // the live XKB keymap. The runtime command remains fail-loud until
            // those exact semantics have a backing path.
            option => return Err(format!("unsupported {name} option {option}")),
        }
        index += 1;
    }
    let remaining = &args[index..];
    if remaining.len() < minimum {
        return Err(format!(
            "Invalid {name} command (expected at least {minimum} non-option arguments, got {})",
            remaining.len()
        ));
    }
    Ok(Command::Bind {
        key: join_words(&remaining[..1]),
        command: (!unbind).then(|| join_words(&remaining[1..])),
        keycode,
        release,
        locked,
        inhibited,
        no_repeat,
        input_device,
    })
}

fn parse_mode(args: &[&str]) -> Result<Command, String> {
    let (pango_markup, args) = match args {
        [flag, rest @ ..] if *flag == "--pango_markup" => (true, rest),
        _ => (false, args),
    };
    let Some((name, subcommand)) = args.split_first() else {
        return Err(if pango_markup {
            "Mode name is missing"
        } else {
            "Expected 'mode <name>'"
        }
        .into());
    };
    let name = join_words(&[name]);
    let subcommand = match subcommand {
        [] => None,
        [name, rest @ ..] if name.eq_ignore_ascii_case("set") => Some(Box::new(parse_set(rest)?)),
        // Sway dispatches the nested word through the same handlers as the
        // top level, with `config->current_mode` pointed at this mode for the
        // duration (`sway/sway/commands/mode.c:11-21,80-84`).
        [name, rest @ ..]
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "bindcode"
                    | "bindgesture"
                    | "bindswitch"
                    | "bindsym"
                    | "unbindcode"
                    | "unbindgesture"
                    | "unbindswitch"
                    | "unbindsym"
            ) =>
        {
            Some(Box::new(match name.to_ascii_lowercase().as_str() {
                "bindsym" => parse_bind_command(rest, false, false)?,
                "unbindsym" => parse_bind_command(rest, false, true)?,
                "bindcode" => parse_bind_command(rest, true, false)?,
                "unbindcode" => parse_bind_command(rest, true, true)?,
                "bindswitch" => parse_switch_bind_command(rest, false)?,
                "unbindswitch" => parse_switch_bind_command(rest, true)?,
                // Same refusal as the top-level form: swayward has no gesture
                // command-binding table to insert into.
                _ => return Err("gesture events have no sway command-binding model".into()),
            }))
        }
        [name, ..] => return Err(format!("Unknown/invalid command '{name}'")),
    };
    Ok(Command::Mode {
        name,
        pango_markup,
        subcommand,
    })
}

fn parse_set(args: &[&str]) -> Result<Command, String> {
    if args.len() < 2 {
        return Err(format!(
            "Invalid set command (expected at least 2 arguments, got {})",
            args.len()
        ));
    }
    let name = args[0];
    if !name.starts_with('$') {
        return Err(format!("variable '{name}' must start with $"));
    }
    Ok(Command::Set {
        name: name.to_owned(),
        value: join_words(&args[1..]),
    })
}

fn parse_gaps(args: &[&str]) -> Result<Command, String> {
    // Sway dispatches on argument count, and rejects anything that is neither
    // shape with both expectations named (`sway/sway/commands/gaps.c:205-223`).
    match args {
        [kind, raw_amount] => {
            let Some((inner, sides)) = parse_gaps_kind(kind) else {
                return Err(format!("Expected {GAPS_EXPECTED_DEFAULTS}"));
            };
            let Some(amount) = parse_gaps_amount(raw_amount) else {
                return Err(format!("Expected {GAPS_EXPECTED_DEFAULTS}"));
            };
            Ok(Command::GapsDefaults {
                inner,
                sides,
                amount,
            })
        }
        [kind, scope, operation, raw_amount] => {
            let Some((inner, sides)) = parse_gaps_kind(kind) else {
                return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}"));
            };
            let all = match scope.to_ascii_lowercase().as_str() {
                "all" => true,
                "current" => false,
                _ => return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}")),
            };
            let operation = match operation.to_ascii_lowercase().as_str() {
                "set" => GapOperation::Set,
                "plus" => GapOperation::Plus,
                "minus" => GapOperation::Minus,
                "toggle" => GapOperation::Toggle,
                _ => return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}")),
            };
            let Some(amount) = parse_gaps_amount(raw_amount) else {
                return Err(format!("Expected {GAPS_EXPECTED_RUNTIME}"));
            };
            Ok(Command::Gaps {
                inner,
                sides,
                all,
                operation,
                amount,
            })
        }
        args if args.len() < 2 => Err(format!(
            "Invalid gaps command (expected at least 2 arguments, got {})",
            args.len()
        )),
        _ => Err(format!(
            "Expected {GAPS_EXPECTED_RUNTIME} or {GAPS_EXPECTED_DEFAULTS}"
        )),
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
    let (no_auto_back_and_forth, args) = match args {
        [flag, rest @ ..] if flag.eq_ignore_ascii_case("--no-auto-back-and-forth") => (true, rest),
        args => (false, args),
    };
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
    if no_auto_back_and_forth
        && !matches!(args.first(), Some(value) if value.eq_ignore_ascii_case("workspace"))
    {
        return Err("Expected 'move [--no-auto-back-and-forth] [window|container] [to] workspace <name|number>'".into());
    }
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
    Ok(Command::MoveToWorkspace {
        target,
        auto_back_and_forth: !no_auto_back_and_forth,
    })
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
        "stacking" => Some(Layout::Stacked),
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
        "t" | "toggle" => Some(Layout::ToggleSplit),
        "n" | "none" => None,
        _ => return Err("Invalid split command (expected either horizontal or vertical).".into()),
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

fn parse_workspace_command(args: &[&str]) -> Result<Command, String> {
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
        return Ok(Command::AssignWorkspace {
            target: parse_workspace(&args[..index])?,
            outputs: args[index + 1..]
                .iter()
                .map(|output| join_words(&[output]))
                .collect(),
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
fn parse_workspace_gaps(args: &[&str], index: usize) -> Result<Command, String> {
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
    let Some((inner, sides)) = parse_gaps_kind(args[index + 1]) else {
        return Err(EXPECTED.into());
    };
    let Ok(amount) = args[index + 2].parse::<i32>() else {
        return Err(EXPECTED.into());
    };
    Ok(Command::WorkspaceGaps {
        name: join_words(&args[..index]),
        inner,
        sides,
        amount,
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
        [] => Err("Expected 'workspace [number] <name>'".into()),
        names => Ok(WorkspaceTarget::Name(join_words(names))),
    }
}

fn parse_resize(args: &[&str]) -> Result<Command, String> {
    let [operation, rest @ ..] = args else {
        return Err(resize_usage());
    };
    if operation.eq_ignore_ascii_case("set") {
        return parse_resize_set(rest);
    }
    let [axis, rest @ ..] = rest else {
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

fn parse_resize_set(mut args: &[&str]) -> Result<Command, String> {
    let usage = || {
        "Expected 'resize set [width] <width> [px|ppt]' or 'resize set height <height> [px|ppt]' or 'resize set [width] <width> [px|ppt] [height] <height> [px|ppt]".to_owned()
    };
    if args.is_empty() {
        return Err(usage());
    }

    let mut width = None;
    if args.len() >= 2 && args[0] == "width" && args[1] != "height" {
        args = &args[1..];
    }
    if args[0] != "height" {
        let (amount, consumed) = parse_resize_amount(args).map_err(|_| usage())?;
        width = Some(amount);
        args = &args[consumed..];
    }

    let mut height = None;
    if !args.is_empty() {
        if args.len() >= 2 && args[0] == "height" {
            args = &args[1..];
        }
        let (amount, consumed) = parse_resize_amount(args).map_err(|_| usage())?;
        if consumed != args.len() {
            return Err(usage());
        }
        height = Some(amount);
    }

    Ok(Command::ResizeSet { width, height })
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
    const NO_STARTUP_ID: &str = "--no-startup-id";
    let rest = command.strip_prefix(NO_STARTUP_ID);
    let no_startup_id =
        rest.is_some_and(|rest| rest.chars().next().is_none_or(char::is_whitespace));
    if no_startup_id {
        command = command[NO_STARTUP_ID.len()..].trim_start();
    }
    if command.is_empty() {
        return Err(format!("Expected '{name} <command>'"));
    }
    let command =
        if words(command).map_err(str::to_owned)?.len() == 1 && command.starts_with(['\'', '"']) {
            strip_sway_quotes(command)
        } else {
            command.to_owned()
        };
    Ok(Command::Exec {
        command,
        no_startup_id,
    })
}

fn strip_sway_quotes(value: &str) -> String {
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

fn parse_rule_criteria<'a>(
    input: &'a str,
    name: &str,
    usage: &str,
) -> Result<(String, &'a str), String> {
    let rest = input[name.len()..].trim_start();
    let Some(end) = criteria_end(rest) else {
        return Err(usage.into());
    };
    let criteria = rest[..=end].to_owned();
    crate::criteria::Criteria::parse(&criteria, None)?;
    Ok((criteria, rest[end + 1..].trim_start()))
}

fn parse_assign(input: &str, name: &str) -> Result<Command, String> {
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
    let words = words(target).map_err(str::to_owned)?;
    let target = match words.as_slice() {
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

fn parse_no_focus(input: &str, name: &str) -> Result<Command, String> {
    const USAGE: &str = "Expected 'no_focus <criteria>'";
    let (criteria, trailing) = parse_rule_criteria(input, name, USAGE)?;
    if !trailing.is_empty() {
        return Err(USAGE.into());
    }
    Ok(Command::NoFocus { criteria })
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

#[cfg(test)]
mod smart_borders_tests {
    use super::*;

    #[test]
    fn toggle_maps_to_off_like_sway() {
        assert_eq!(
            parse_one("smart_borders toggle"),
            Ok(Command::SetLayoutOption(LayoutOption::SmartBorders(
                "off".into()
            )))
        );
    }
}

#[cfg(test)]
mod workspace_number_tests {
    use super::*;

    #[test]
    fn workspace_numbers_must_start_with_a_digit() {
        for input in [
            "workspace number named",
            "move to workspace number named",
            "rename workspace number named to 2",
        ] {
            assert_eq!(
                parse_one(input),
                Err("Invalid workspace number 'named'".into()),
                "{input}"
            );
        }
        assert_eq!(
            parse_one("workspace number"),
            Err("Expected workspace number".into())
        );
        assert_eq!(
            parse_one(r#"workspace number "3:third""#),
            Ok(Command::Workspace {
                target: WorkspaceTarget::Number("3:third".into()),
                auto_back_and_forth: true,
            })
        );
    }
}

#[cfg(test)]
mod command_list_tests {
    use super::*;

    #[test]
    fn nul_starts_a_new_command_like_sway() {
        let parsed = parse("nop before\0after");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].as_ref().unwrap().command, Command::Nop);
        assert_eq!(
            parsed[1].as_ref().unwrap_err().error.as_deref(),
            Some("Unknown/invalid command 'after'")
        );
    }

    #[test]
    fn unterminated_nop_argument_is_ignored_like_sway() {
        let parsed = parse("nop \"unterminated");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].as_ref().unwrap().command, Command::Nop);
    }

    #[test]
    fn comma_does_not_start_new_criteria() {
        for (input, command) in [
            ("floating enable, [title=x] kill", "[title=x]"),
            ("[app_id=x] kill, [app_id=y] kill", "[app_id=y]"),
        ] {
            let parsed = parse(input);
            assert_eq!(parsed.len(), 2, "{input}");
            assert_eq!(
                parsed[1],
                Err(parse_error(format!("Unknown/invalid command '{command}'"))),
                "{input}"
            );
        }
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;

    #[test]
    fn strips_outer_quotes_from_a_single_exec_argument() {
        for (input, expected) in [
            (r#"exec "foot -e htop""#, "foot -e htop"),
            ("exec 'a b'", "a b"),
            (r#"exec foo "bar baz""#, r#"foo "bar baz""#),
        ] {
            assert_eq!(
                parse_one(input),
                Ok(Command::Exec {
                    command: expected.into(),
                    no_startup_id: false,
                }),
                "{input}"
            );
        }
    }
}

#[cfg(test)]
mod criteria_rule_tests {
    use super::*;

    #[test]
    fn assign_parses_sway_targets() {
        for (input, expected) in [
            (
                r#"assign [app_id="^term$"] workspace 7: target"#,
                AssignmentTarget::Workspace("7: target".into()),
            ),
            (
                r#"assign [app_id="^term$"] → number 7: target"#,
                AssignmentTarget::WorkspaceNumber("7: target".into()),
            ),
            (
                r#"assign [app_id="^term$"] output HDMI-A-1"#,
                AssignmentTarget::Output("HDMI-A-1".into()),
            ),
        ] {
            let Command::Assign { criteria, target } = parse_one(input).unwrap() else {
                panic!("expected assign command");
            };
            assert_eq!(criteria, r#"[app_id="^term$"]"#);
            assert_eq!(target, expected);
        }
    }

    #[test]
    fn no_focus_parses_native_wayland_criteria() {
        assert_eq!(
            parse_one(r#"no_focus [app_id="^term$" title="dialog"]"#),
            Ok(Command::NoFocus {
                criteria: r#"[app_id="^term$" title="dialog"]"#.into(),
            })
        );
    }

    #[test]
    fn runtime_map_rules_reject_x11_only_criteria() {
        for (field, value) in [
            ("class", "value"),
            ("instance", "value"),
            ("id", "42"),
            ("window_role", "value"),
            ("window_type", "dialog"),
        ] {
            for command in [
                format!("assign [{field}={value}] workspace 2"),
                format!("no_focus [{field}={value}]"),
            ] {
                // `Criteria::parse` rejects these selectors itself and names
                // the offending field, so a rule command inherits that error
                // rather than restating it less precisely.
                assert_eq!(
                    parse_one(&command).unwrap_err(),
                    format!("X11-only criterion '{field}' is unsupported"),
                    "{command}"
                );
            }
        }
    }

    #[test]
    fn runtime_map_rules_reject_missing_or_invalid_arguments() {
        assert!(parse_one("assign [app_id=term]").is_err());
        assert!(parse_one("assign [app_id=term] number named").is_err());
        assert!(parse_one("no_focus [app_id=term] trailing").is_err());
    }
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[test]
    fn output_parses_chained_core_actions_and_power() {
        let parsed = parse_output_command(&[
            "HDMI-A-1",
            "scale",
            "1.5",
            "transform",
            "90",
            "position",
            "10,20",
            "mode",
            "1920",
            "1080",
        ])
        .unwrap();
        let Command::Output { target, actions } = parsed else {
            panic!("expected output command")
        };
        assert_eq!(target, "HDMI-A-1");
        assert_eq!(
            actions,
            [
                crate::OutputAction::Scale {
                    scale: crate::ScaleToSet::Specific(1.5),
                },
                crate::OutputAction::Transform {
                    transform: crate::Transform::_270,
                },
                crate::OutputAction::Position {
                    position: crate::PositionToSet::Specific(crate::ConfiguredPosition {
                        x: 10,
                        y: 20,
                    }),
                },
                crate::OutputAction::Mode {
                    mode: crate::ModeToSet::Specific(crate::ConfiguredMode {
                        width: 1920,
                        height: 1080,
                        refresh: None,
                    }),
                },
            ]
        );
        assert!(matches!(
            parse_output_command(&["*", "power", "off"]),
            Ok(Command::Output { actions, .. })
                if actions == [crate::OutputAction::Power { power: Toggle::Disable }]
        ));
        assert_eq!(
            parse_output_command(&["*", "dpms", "toggle"]),
            Err("Cannot apply toggle to all outputs".into())
        );
        assert_eq!(
            parse_output_command(&["HDMI-A-1", "scale", "NaN"]),
            Err("Invalid scale.".into())
        );
        assert!(matches!(
            parse_output_command(&["HDMI-A-1", "mode", "1920x1080@60hz"]),
            Ok(Command::Output { .. })
        ));
    }

    /// Sway accepts `t` as the toggle alias and `n` as the none alias, and
    /// compares every split argument case-insensitively with `strcasecmp`
    /// (`sway/sway/commands/split.c:54-82`). The rejection text is sway's too.
    #[test]
    fn split_accepts_sways_aliases_and_rejects_with_sways_message() {
        for (argument, expected) in [
            ("t", Some(Layout::ToggleSplit)),
            ("T", Some(Layout::ToggleSplit)),
            ("toggle", Some(Layout::ToggleSplit)),
            ("h", Some(Layout::SplitH)),
            ("vertical", Some(Layout::SplitV)),
            ("n", None),
            ("none", None),
        ] {
            assert_eq!(
                parse_split(&[argument]),
                Ok(Command::Split(expected)),
                "split {argument} must parse like sway"
            );
        }

        assert_eq!(
            parse_split(&["sideways"]),
            Err("Invalid split command (expected either horizontal or vertical).".into()),
            "sway reports this exact text for an unknown split argument"
        );
    }
}

#[cfg(test)]
mod gap_form_tests {
    use super::*;

    fn parse_ok(input: &str) -> Command {
        match parse_one(input) {
            Ok(command) => command,
            Err(error) => panic!("{input:?} rejected: {error}"),
        }
    }

    /// Sway dispatches on argument count: two arguments are the defaults form,
    /// four are the runtime form (`sway/sway/commands/gaps.c:205-223`). They
    /// must produce different commands, because they write different state.
    #[test]
    fn gaps_argument_count_selects_the_form() {
        assert_eq!(
            parse_ok("gaps inner 10"),
            Command::GapsDefaults {
                inner: true,
                sides: [false; 4],
                amount: 10,
            }
        );
        assert_eq!(
            parse_ok("gaps inner all set 10"),
            Command::Gaps {
                inner: true,
                sides: [false; 4],
                all: true,
                operation: GapOperation::Set,
                amount: 10,
            }
        );
    }

    /// Every kind spelling sway accepts, in both forms. `horizontal` and
    /// `vertical` select side pairs (`sway/sway/commands/gaps.c:62-84`).
    #[test]
    fn gaps_accepts_every_sway_kind_in_both_forms() {
        // [left, right, top, bottom]
        for (kind, inner, sides) in [
            ("inner", true, [false; 4]),
            ("outer", false, [true; 4]),
            ("horizontal", false, [true, true, false, false]),
            ("vertical", false, [false, false, true, true]),
            ("left", false, [true, false, false, false]),
            ("right", false, [false, true, false, false]),
            ("top", false, [false, false, true, false]),
            ("bottom", false, [false, false, false, true]),
        ] {
            assert_eq!(
                parse_ok(&format!("gaps {kind} 5")),
                Command::GapsDefaults {
                    inner,
                    sides,
                    amount: 5
                },
                "defaults form: {kind}"
            );
            assert_eq!(
                parse_ok(&format!("gaps {kind} current set 5")),
                Command::Gaps {
                    inner,
                    sides,
                    all: false,
                    operation: GapOperation::Set,
                    amount: 5,
                },
                "runtime form: {kind}"
            );
            // Sway compares with strcasecmp throughout.
            assert_eq!(
                parse_ok(&format!("gaps {} 5", kind.to_uppercase())),
                Command::GapsDefaults {
                    inner,
                    sides,
                    amount: 5
                },
                "case-insensitive: {kind}"
            );
        }
    }

    /// Sway parses the amount with `strtol` and permits a `px` suffix in
    /// `cmd_gaps` (`sway/sway/commands/gaps.c:55-58`), including negatives for
    /// outer gaps.
    #[test]
    fn gaps_amount_accepts_px_suffix_and_negatives() {
        assert_eq!(
            parse_ok("gaps outer -3"),
            Command::GapsDefaults {
                inner: false,
                sides: [true; 4],
                amount: -3,
            }
        );
        assert_eq!(
            parse_ok("gaps inner 12px"),
            Command::GapsDefaults {
                inner: true,
                sides: [false; 4],
                amount: 12,
            }
        );
        assert!(parse_one("gaps inner 12em").is_err());
    }

    /// Sway names the expectation it was testing, and names both when the
    /// argument count matches neither form
    /// (`sway/sway/commands/gaps.c:205-223`).
    #[test]
    fn gaps_rejections_use_sways_text() {
        assert_eq!(
            parse_one("gaps inner").unwrap_err(),
            "Invalid gaps command (expected at least 2 arguments, got 1)"
        );
        assert_eq!(
            parse_one("gaps bogus 10").unwrap_err(),
            format!("Expected {GAPS_EXPECTED_DEFAULTS}")
        );
        assert_eq!(
            parse_one("gaps inner sometimes set 10").unwrap_err(),
            format!("Expected {GAPS_EXPECTED_RUNTIME}")
        );
        assert_eq!(
            parse_one("gaps inner all set 10 extra").unwrap_err(),
            format!("Expected {GAPS_EXPECTED_RUNTIME} or {GAPS_EXPECTED_DEFAULTS}")
        );
    }

    /// `workspace <name> gaps <kind> <px>`
    /// (`sway/sway/commands/workspace.c:57-117`).
    #[test]
    fn workspace_gaps_parses_with_the_name_before_the_keyword() {
        assert_eq!(
            parse_ok("workspace roomy gaps inner 45"),
            Command::WorkspaceGaps {
                name: "roomy".into(),
                inner: true,
                sides: [false; 4],
                amount: 45,
            }
        );
        // Sway joins everything before the keyword into the name.
        assert_eq!(
            parse_ok("workspace my space gaps outer 3"),
            Command::WorkspaceGaps {
                name: "my space".into(),
                inner: false,
                sides: [true; 4],
                amount: 3,
            }
        );
        // A leading `gaps` is the top-level command, not a workspace name.
        assert!(matches!(
            parse_ok("gaps inner 10"),
            Command::GapsDefaults { .. }
        ));
        // Sway's workspace-gaps amount takes no suffix
        // (`sway/sway/commands/workspace.c:76-80`).
        assert!(parse_one("workspace roomy gaps inner 45px").is_err());
    }

    /// Sway collects every word after `output` as a separate entry
    /// (`sway/sway/commands/workspace.c:153-155`).
    #[test]
    fn workspace_output_assignment_keeps_the_output_list() {
        assert_eq!(
            parse_ok("workspace 7 output DP-1 HDMI-A-1"),
            Command::AssignWorkspace {
                target: WorkspaceTarget::Name("7".into()),
                outputs: vec!["DP-1".into(), "HDMI-A-1".into()],
            }
        );
        assert_eq!(
            parse_ok("workspace 7 output DP-1"),
            Command::AssignWorkspace {
                target: WorkspaceTarget::Name("7".into()),
                outputs: vec!["DP-1".into()],
            }
        );
    }
}

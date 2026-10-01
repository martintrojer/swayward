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
/// Each variant names a sway directive that swayward also accepts in KDL.
/// String-valued variants are checked when the setting is applied, against
/// the config crate's `FromStr` or a hand-written match in the executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutOption {
    FocusWrapping(FocusWrappingArg),
    ForceFocusWrapping(String),
    HideEdgeBorders(String),
    SmartBorders(String),
    /// `hide_edge_borders smart|smart_no_gaps`: sway sets the smart-border
    /// mode and resets the edge mode to none in one command
    /// (`sway/sway/commands/hide_edge_borders.c:34-39`).
    HideEdgeBordersSmart(String),
    SmartGaps(SmartGapsArg),
    ShowMarks(String),
    TitleAlignment(String),
    TilingDrag(String),
    TilingDragThreshold(u32),
    ForceDisplayUrgencyHint(u32),
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
    /// `mouse_warping output|container|none`.
    ///
    /// Sway keeps the three modes apart: `output` warps only when the focused
    /// target sits on an output that does not contain the pointer, while
    /// `container` warps on every qualifying focus change
    /// (`sway/sway/input/seat.c:1526-1547`).
    MouseWarping(MouseWarping),
    PopupDuringFullscreen(PopupDuringFullscreen),
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

/// A `focus_wrapping` argument. `Toggle` depends on the current value, so it
/// is resolved when the setting is applied
/// (`sway/sway/commands/focus_wrapping.c:12-21`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusWrappingArg {
    Yes,
    No,
    Force,
    Workspace,
    Toggle,
}

/// A `smart_gaps` argument. `Toggle` depends on the current value, so it is
/// resolved when the setting is applied
/// (`sway/sway/commands/smart_gaps.c:18-23`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmartGapsArg {
    On,
    Off,
    InverseOuter,
    Toggle,
}

/// Sway's three `popup_during_fullscreen` modes
/// (`sway/sway/commands/popup_during_fullscreen.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupDuringFullscreen {
    Smart,
    Ignore,
    LeaveFullscreen,
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
    Opacity(f32),
    OpacityRelative(f32),
    /// A sway directive that sets a layout option for the whole session.
    ///
    /// These come from sway's shared `handlers` table, which serves both the
    /// config file and IPC (`sway/sway/commands.c:43-100,160-173`). The
    /// config-only `config_handlers` are excluded at run time
    /// (commands.c:102-110,156-163) and are not represented here. swayward
    /// keeps the setting in KDL and applies the same value, then re-runs the
    /// normal config apply path.
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

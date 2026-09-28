use knuffel::errors::DecodeError;
use swayward_ipc::{
    ColumnDisplay, LayoutSwitchTarget, PositionChange, SizeChange, WorkspaceReferenceArg,
};

use crate::recent_windows::{MruDirection, MruFilter, MruScope};

// Remember to add new actions to the CLI enum too.
#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub enum Action {
    #[knuffel(skip)]
    SwayCommand(String),
    Quit(#[knuffel(property(name = "skip-confirmation"), default)] bool),
    #[knuffel(skip)]
    ChangeVt(i32),
    Suspend,
    PowerOffMonitors,
    PowerOnMonitors,
    ToggleDebugTint,
    DebugToggleOpaqueRegions,
    DebugToggleDamage,
    Spawn(#[knuffel(arguments)] Vec<String>),
    SpawnSh(#[knuffel(argument)] String),
    DoScreenTransition(#[knuffel(property(name = "delay-ms"))] Option<u16>),
    #[knuffel(skip)]
    ConfirmScreenshot {
        write_to_disk: bool,
    },
    #[knuffel(skip)]
    CancelScreenshot,
    #[knuffel(skip)]
    ScreenshotTogglePointer,
    Screenshot(
        #[knuffel(property(name = "show-pointer"), default = true)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    ScreenshotScreen(
        #[knuffel(property(name = "write-to-disk"), default = true)] bool,
        #[knuffel(property(name = "show-pointer"), default = true)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    ScreenshotWindow(
        #[knuffel(property(name = "write-to-disk"), default = true)] bool,
        #[knuffel(property(name = "show-pointer"), default = false)] bool,
        // Path; not settable from knuffel
        Option<String>,
    ),
    #[knuffel(skip)]
    ScreenshotWindowById {
        id: u64,
        write_to_disk: bool,
        show_pointer: bool,
        path: Option<String>,
    },
    ToggleKeyboardShortcutsInhibit,
    CloseWindow,
    #[knuffel(skip)]
    CloseWindowById(u64),
    FullscreenWindow,
    #[knuffel(skip)]
    FullscreenWindowById(u64),
    ToggleWindowedFullscreen,
    #[knuffel(skip)]
    ToggleWindowedFullscreenById(u64),
    #[knuffel(skip)]
    FocusWindow(u64),
    FocusWindowInColumn(#[knuffel(argument)] u8),
    FocusWindowPrevious,
    FocusColumnLeft,
    #[knuffel(skip)]
    FocusColumnLeftUnderMouse,
    FocusColumnRight,
    #[knuffel(skip)]
    FocusColumnRightUnderMouse,
    FocusColumnFirst,
    FocusColumnLast,
    FocusColumnRightOrFirst,
    FocusColumnLeftOrLast,
    FocusColumn(#[knuffel(argument)] usize),
    FocusWindowOrMonitorUp,
    FocusWindowOrMonitorDown,
    FocusColumnOrMonitorLeft,
    FocusColumnOrMonitorRight,
    FocusWindowDown,
    FocusWindowUp,
    FocusWindowDownOrColumnLeft,
    FocusWindowDownOrColumnRight,
    FocusWindowUpOrColumnLeft,
    FocusWindowUpOrColumnRight,
    FocusWindowOrWorkspaceDown,
    FocusWindowOrWorkspaceUp,
    FocusWindowTop,
    FocusWindowBottom,
    FocusWindowDownOrTop,
    FocusWindowUpOrBottom,
    MoveColumnLeft,
    MoveColumnRight,
    MoveColumnToFirst,
    MoveColumnToLast,
    MoveColumnLeftOrToMonitorLeft,
    MoveColumnRightOrToMonitorRight,
    MoveColumnToIndex(#[knuffel(argument)] usize),
    MoveWindowDown,
    MoveWindowUp,
    MoveWindowDownOrToWorkspaceDown,
    MoveWindowUpOrToWorkspaceUp,
    ConsumeOrExpelWindowLeft,
    #[knuffel(skip)]
    ConsumeOrExpelWindowLeftById(u64),
    ConsumeOrExpelWindowRight,
    #[knuffel(skip)]
    ConsumeOrExpelWindowRightById(u64),
    ConsumeWindowIntoColumn,
    ExpelWindowFromColumn,
    SwapWindowLeft,
    SwapWindowRight,
    ToggleColumnTabbedDisplay,
    SetColumnDisplay(#[knuffel(argument, str)] ColumnDisplay),
    CenterColumn,
    CenterWindow,
    #[knuffel(skip)]
    CenterWindowById(u64),
    CenterVisibleColumns,
    FocusWorkspaceDown,
    #[knuffel(skip)]
    FocusWorkspaceDownUnderMouse,
    FocusWorkspaceUp,
    #[knuffel(skip)]
    FocusWorkspaceUpUnderMouse,
    FocusWorkspace(#[knuffel(argument)] WorkspaceReference),
    FocusWorkspacePrevious,
    MoveWindowToWorkspaceDown(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveWindowToWorkspaceUp(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveWindowToWorkspace(
        #[knuffel(argument)] WorkspaceReference,
        #[knuffel(property(name = "focus"), default = true)] bool,
    ),
    #[knuffel(skip)]
    MoveWindowToWorkspaceById {
        window_id: u64,
        reference: WorkspaceReference,
        focus: bool,
    },
    MoveColumnToWorkspaceDown(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveColumnToWorkspaceUp(#[knuffel(property(name = "focus"), default = true)] bool),
    MoveColumnToWorkspace(
        #[knuffel(argument)] WorkspaceReference,
        #[knuffel(property(name = "focus"), default = true)] bool,
    ),
    MoveWorkspaceDown,
    MoveWorkspaceUp,
    MoveWorkspaceToIndex(#[knuffel(argument)] usize),
    #[knuffel(skip)]
    MoveWorkspaceToIndexByRef {
        new_idx: usize,
        reference: WorkspaceReference,
    },
    #[knuffel(skip)]
    MoveWorkspaceToMonitorByRef {
        output_name: String,
        reference: WorkspaceReference,
    },
    MoveWorkspaceToMonitor(#[knuffel(argument)] String),
    SetWorkspaceName(#[knuffel(argument)] String),
    #[knuffel(skip)]
    SetWorkspaceNameByRef {
        name: String,
        reference: WorkspaceReference,
    },
    UnsetWorkspaceName,
    #[knuffel(skip)]
    UnsetWorkSpaceNameByRef(#[knuffel(argument)] WorkspaceReference),
    FocusMonitorLeft,
    FocusMonitorRight,
    FocusMonitorDown,
    FocusMonitorUp,
    FocusMonitorPrevious,
    FocusMonitorNext,
    FocusMonitor(#[knuffel(argument)] String),
    MoveWindowToMonitorLeft,
    MoveWindowToMonitorRight,
    MoveWindowToMonitorDown,
    MoveWindowToMonitorUp,
    MoveWindowToMonitorPrevious,
    MoveWindowToMonitorNext,
    MoveWindowToMonitor(#[knuffel(argument)] String),
    #[knuffel(skip)]
    MoveWindowToMonitorById {
        id: u64,
        output: String,
    },
    MoveColumnToMonitorLeft,
    MoveColumnToMonitorRight,
    MoveColumnToMonitorDown,
    MoveColumnToMonitorUp,
    MoveColumnToMonitorPrevious,
    MoveColumnToMonitorNext,
    MoveColumnToMonitor(#[knuffel(argument)] String),
    SetWindowWidth(#[knuffel(argument, str)] SizeChange),
    #[knuffel(skip)]
    SetWindowWidthById {
        id: u64,
        change: SizeChange,
    },
    SetWindowHeight(#[knuffel(argument, str)] SizeChange),
    #[knuffel(skip)]
    SetWindowHeightById {
        id: u64,
        change: SizeChange,
    },
    ResetWindowHeight,
    #[knuffel(skip)]
    ResetWindowHeightById(u64),
    SwitchPresetColumnWidth,
    SwitchPresetColumnWidthBack,
    SwitchPresetWindowWidth,
    SwitchPresetWindowWidthBack,
    #[knuffel(skip)]
    SwitchPresetWindowWidthById(u64),
    #[knuffel(skip)]
    SwitchPresetWindowWidthBackById(u64),
    SwitchPresetWindowHeight,
    SwitchPresetWindowHeightBack,
    #[knuffel(skip)]
    SwitchPresetWindowHeightById(u64),
    #[knuffel(skip)]
    SwitchPresetWindowHeightBackById(u64),
    MaximizeColumn,
    MaximizeWindowToEdges,
    #[knuffel(skip)]
    MaximizeWindowToEdgesById(u64),
    SetColumnWidth(#[knuffel(argument, str)] SizeChange),
    ExpandColumnToAvailableWidth,
    SwitchLayout(#[knuffel(argument, str)] LayoutSwitchTarget),
    ShowHotkeyOverlay,
    MoveWorkspaceToMonitorLeft,
    MoveWorkspaceToMonitorRight,
    MoveWorkspaceToMonitorDown,
    MoveWorkspaceToMonitorUp,
    MoveWorkspaceToMonitorPrevious,
    MoveWorkspaceToMonitorNext,
    ToggleWindowFloating,
    #[knuffel(skip)]
    ToggleWindowFloatingById(u64),
    MoveWindowToFloating,
    #[knuffel(skip)]
    MoveWindowToFloatingById(u64),
    MoveWindowToTiling,
    #[knuffel(skip)]
    MoveWindowToTilingById(u64),
    FocusFloating,
    FocusTiling,
    SwitchFocusBetweenFloatingAndTiling,
    #[knuffel(skip)]
    MoveFloatingWindowById {
        id: Option<u64>,
        x: PositionChange,
        y: PositionChange,
    },
    ToggleWindowRuleOpacity,
    #[knuffel(skip)]
    ToggleWindowRuleOpacityById(u64),
    SetDynamicCastWindow,
    #[knuffel(skip)]
    SetDynamicCastWindowById(u64),
    SetDynamicCastMonitor(#[knuffel(argument)] Option<String>),
    ClearDynamicCastTarget,
    #[knuffel(skip)]
    StopCast(u64),
    ToggleOverview,
    OpenOverview,
    CloseOverview,
    #[knuffel(skip)]
    ToggleWindowUrgent(u64),
    #[knuffel(skip)]
    SetWindowUrgent(u64),
    #[knuffel(skip)]
    UnsetWindowUrgent(u64),
    #[knuffel(skip)]
    LoadConfigFile(#[knuffel(argument)] Option<String>),
    #[knuffel(skip)]
    MruAdvance {
        direction: MruDirection,
        scope: Option<MruScope>,
        filter: Option<MruFilter>,
    },
    #[knuffel(skip)]
    MruConfirm,
    #[knuffel(skip)]
    MruCancel,
    #[knuffel(skip)]
    MruCloseCurrentWindow,
    #[knuffel(skip)]
    MruFirst,
    #[knuffel(skip)]
    MruLast,
    #[knuffel(skip)]
    MruSetScope(MruScope),
    #[knuffel(skip)]
    MruCycleScope,
}

impl From<swayward_ipc::Action> for Action {
    fn from(value: swayward_ipc::Action) -> Self {
        match value {
            swayward_ipc::Action::Quit { skip_confirmation } => Self::Quit(skip_confirmation),
            swayward_ipc::Action::PowerOffMonitors {} => Self::PowerOffMonitors,
            swayward_ipc::Action::PowerOnMonitors {} => Self::PowerOnMonitors,
            swayward_ipc::Action::Spawn { command } => Self::Spawn(command),
            swayward_ipc::Action::SpawnSh { command } => Self::SpawnSh(command),
            swayward_ipc::Action::DoScreenTransition { delay_ms } => {
                Self::DoScreenTransition(delay_ms)
            }
            swayward_ipc::Action::Screenshot { show_pointer, path } => {
                Self::Screenshot(show_pointer, path)
            }
            swayward_ipc::Action::ScreenshotScreen {
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotScreen(write_to_disk, show_pointer, path),
            swayward_ipc::Action::ScreenshotWindow {
                id: None,
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotWindow(write_to_disk, show_pointer, path),
            swayward_ipc::Action::ScreenshotWindow {
                id: Some(id),
                write_to_disk,
                show_pointer,
                path,
            } => Self::ScreenshotWindowById {
                id,
                write_to_disk,
                show_pointer,
                path,
            },
            swayward_ipc::Action::ToggleKeyboardShortcutsInhibit {} => {
                Self::ToggleKeyboardShortcutsInhibit
            }
            swayward_ipc::Action::CloseWindow { id: None } => Self::CloseWindow,
            swayward_ipc::Action::CloseWindow { id: Some(id) } => Self::CloseWindowById(id),
            swayward_ipc::Action::FullscreenWindow { id: None } => Self::FullscreenWindow,
            swayward_ipc::Action::FullscreenWindow { id: Some(id) } => {
                Self::FullscreenWindowById(id)
            }
            swayward_ipc::Action::ToggleWindowedFullscreen { id: None } => {
                Self::ToggleWindowedFullscreen
            }
            swayward_ipc::Action::ToggleWindowedFullscreen { id: Some(id) } => {
                Self::ToggleWindowedFullscreenById(id)
            }
            swayward_ipc::Action::FocusWindow { id } => Self::FocusWindow(id),
            swayward_ipc::Action::FocusWindowInColumn { index } => Self::FocusWindowInColumn(index),
            swayward_ipc::Action::FocusWindowPrevious {} => Self::FocusWindowPrevious,
            swayward_ipc::Action::FocusColumnLeft {} => Self::FocusColumnLeft,
            swayward_ipc::Action::FocusColumnRight {} => Self::FocusColumnRight,
            swayward_ipc::Action::FocusColumnFirst {} => Self::FocusColumnFirst,
            swayward_ipc::Action::FocusColumnLast {} => Self::FocusColumnLast,
            swayward_ipc::Action::FocusColumnRightOrFirst {} => Self::FocusColumnRightOrFirst,
            swayward_ipc::Action::FocusColumnLeftOrLast {} => Self::FocusColumnLeftOrLast,
            swayward_ipc::Action::FocusColumn { index } => Self::FocusColumn(index),
            swayward_ipc::Action::FocusWindowOrMonitorUp {} => Self::FocusWindowOrMonitorUp,
            swayward_ipc::Action::FocusWindowOrMonitorDown {} => Self::FocusWindowOrMonitorDown,
            swayward_ipc::Action::FocusColumnOrMonitorLeft {} => Self::FocusColumnOrMonitorLeft,
            swayward_ipc::Action::FocusColumnOrMonitorRight {} => Self::FocusColumnOrMonitorRight,
            swayward_ipc::Action::FocusWindowDown {} => Self::FocusWindowDown,
            swayward_ipc::Action::FocusWindowUp {} => Self::FocusWindowUp,
            swayward_ipc::Action::FocusWindowDownOrColumnLeft {} => {
                Self::FocusWindowDownOrColumnLeft
            }
            swayward_ipc::Action::FocusWindowDownOrColumnRight {} => {
                Self::FocusWindowDownOrColumnRight
            }
            swayward_ipc::Action::FocusWindowUpOrColumnLeft {} => Self::FocusWindowUpOrColumnLeft,
            swayward_ipc::Action::FocusWindowUpOrColumnRight {} => Self::FocusWindowUpOrColumnRight,
            swayward_ipc::Action::FocusWindowOrWorkspaceDown {} => Self::FocusWindowOrWorkspaceDown,
            swayward_ipc::Action::FocusWindowOrWorkspaceUp {} => Self::FocusWindowOrWorkspaceUp,
            swayward_ipc::Action::FocusWindowTop {} => Self::FocusWindowTop,
            swayward_ipc::Action::FocusWindowBottom {} => Self::FocusWindowBottom,
            swayward_ipc::Action::FocusWindowDownOrTop {} => Self::FocusWindowDownOrTop,
            swayward_ipc::Action::FocusWindowUpOrBottom {} => Self::FocusWindowUpOrBottom,
            swayward_ipc::Action::MoveColumnLeft {} => Self::MoveColumnLeft,
            swayward_ipc::Action::MoveColumnRight {} => Self::MoveColumnRight,
            swayward_ipc::Action::MoveColumnToFirst {} => Self::MoveColumnToFirst,
            swayward_ipc::Action::MoveColumnToLast {} => Self::MoveColumnToLast,
            swayward_ipc::Action::MoveColumnToIndex { index } => Self::MoveColumnToIndex(index),
            swayward_ipc::Action::MoveColumnLeftOrToMonitorLeft {} => {
                Self::MoveColumnLeftOrToMonitorLeft
            }
            swayward_ipc::Action::MoveColumnRightOrToMonitorRight {} => {
                Self::MoveColumnRightOrToMonitorRight
            }
            swayward_ipc::Action::MoveWindowDown {} => Self::MoveWindowDown,
            swayward_ipc::Action::MoveWindowUp {} => Self::MoveWindowUp,
            swayward_ipc::Action::MoveWindowDownOrToWorkspaceDown {} => {
                Self::MoveWindowDownOrToWorkspaceDown
            }
            swayward_ipc::Action::MoveWindowUpOrToWorkspaceUp {} => {
                Self::MoveWindowUpOrToWorkspaceUp
            }
            swayward_ipc::Action::ConsumeOrExpelWindowLeft { id: None } => {
                Self::ConsumeOrExpelWindowLeft
            }
            swayward_ipc::Action::ConsumeOrExpelWindowLeft { id: Some(id) } => {
                Self::ConsumeOrExpelWindowLeftById(id)
            }
            swayward_ipc::Action::ConsumeOrExpelWindowRight { id: None } => {
                Self::ConsumeOrExpelWindowRight
            }
            swayward_ipc::Action::ConsumeOrExpelWindowRight { id: Some(id) } => {
                Self::ConsumeOrExpelWindowRightById(id)
            }
            swayward_ipc::Action::ConsumeWindowIntoColumn {} => Self::ConsumeWindowIntoColumn,
            swayward_ipc::Action::ExpelWindowFromColumn {} => Self::ExpelWindowFromColumn,
            swayward_ipc::Action::SwapWindowRight {} => Self::SwapWindowRight,
            swayward_ipc::Action::SwapWindowLeft {} => Self::SwapWindowLeft,
            swayward_ipc::Action::ToggleColumnTabbedDisplay {} => Self::ToggleColumnTabbedDisplay,
            swayward_ipc::Action::SetColumnDisplay { display } => Self::SetColumnDisplay(display),
            swayward_ipc::Action::CenterColumn {} => Self::CenterColumn,
            swayward_ipc::Action::CenterWindow { id: None } => Self::CenterWindow,
            swayward_ipc::Action::CenterWindow { id: Some(id) } => Self::CenterWindowById(id),
            swayward_ipc::Action::CenterVisibleColumns {} => Self::CenterVisibleColumns,
            swayward_ipc::Action::FocusWorkspaceDown {} => Self::FocusWorkspaceDown,
            swayward_ipc::Action::FocusWorkspaceUp {} => Self::FocusWorkspaceUp,
            swayward_ipc::Action::FocusWorkspace { reference } => {
                Self::FocusWorkspace(WorkspaceReference::from(reference))
            }
            swayward_ipc::Action::FocusWorkspacePrevious {} => Self::FocusWorkspacePrevious,
            swayward_ipc::Action::MoveWindowToWorkspaceDown { focus } => {
                Self::MoveWindowToWorkspaceDown(focus)
            }
            swayward_ipc::Action::MoveWindowToWorkspaceUp { focus } => {
                Self::MoveWindowToWorkspaceUp(focus)
            }
            swayward_ipc::Action::MoveWindowToWorkspace {
                window_id: None,
                reference,
                focus,
            } => Self::MoveWindowToWorkspace(WorkspaceReference::from(reference), focus),
            swayward_ipc::Action::MoveWindowToWorkspace {
                window_id: Some(window_id),
                reference,
                focus,
            } => Self::MoveWindowToWorkspaceById {
                window_id,
                reference: WorkspaceReference::from(reference),
                focus,
            },
            swayward_ipc::Action::MoveColumnToWorkspaceDown { focus } => {
                Self::MoveColumnToWorkspaceDown(focus)
            }
            swayward_ipc::Action::MoveColumnToWorkspaceUp { focus } => {
                Self::MoveColumnToWorkspaceUp(focus)
            }
            swayward_ipc::Action::MoveColumnToWorkspace { reference, focus } => {
                Self::MoveColumnToWorkspace(WorkspaceReference::from(reference), focus)
            }
            swayward_ipc::Action::MoveWorkspaceDown {} => Self::MoveWorkspaceDown,
            swayward_ipc::Action::MoveWorkspaceUp {} => Self::MoveWorkspaceUp,
            swayward_ipc::Action::SetWorkspaceName {
                name,
                workspace: None,
            } => Self::SetWorkspaceName(name),
            swayward_ipc::Action::SetWorkspaceName {
                name,
                workspace: Some(reference),
            } => Self::SetWorkspaceNameByRef {
                name,
                reference: WorkspaceReference::from(reference),
            },
            swayward_ipc::Action::UnsetWorkspaceName { reference: None } => {
                Self::UnsetWorkspaceName
            }
            swayward_ipc::Action::UnsetWorkspaceName {
                reference: Some(reference),
            } => Self::UnsetWorkSpaceNameByRef(WorkspaceReference::from(reference)),
            swayward_ipc::Action::FocusMonitorLeft {} => Self::FocusMonitorLeft,
            swayward_ipc::Action::FocusMonitorRight {} => Self::FocusMonitorRight,
            swayward_ipc::Action::FocusMonitorDown {} => Self::FocusMonitorDown,
            swayward_ipc::Action::FocusMonitorUp {} => Self::FocusMonitorUp,
            swayward_ipc::Action::FocusMonitorPrevious {} => Self::FocusMonitorPrevious,
            swayward_ipc::Action::FocusMonitorNext {} => Self::FocusMonitorNext,
            swayward_ipc::Action::FocusMonitor { output } => Self::FocusMonitor(output),
            swayward_ipc::Action::MoveWindowToMonitorLeft {} => Self::MoveWindowToMonitorLeft,
            swayward_ipc::Action::MoveWindowToMonitorRight {} => Self::MoveWindowToMonitorRight,
            swayward_ipc::Action::MoveWindowToMonitorDown {} => Self::MoveWindowToMonitorDown,
            swayward_ipc::Action::MoveWindowToMonitorUp {} => Self::MoveWindowToMonitorUp,
            swayward_ipc::Action::MoveWindowToMonitorPrevious {} => {
                Self::MoveWindowToMonitorPrevious
            }
            swayward_ipc::Action::MoveWindowToMonitorNext {} => Self::MoveWindowToMonitorNext,
            swayward_ipc::Action::MoveWindowToMonitor { id: None, output } => {
                Self::MoveWindowToMonitor(output)
            }
            swayward_ipc::Action::MoveWindowToMonitor {
                id: Some(id),
                output,
            } => Self::MoveWindowToMonitorById { id, output },
            swayward_ipc::Action::MoveColumnToMonitorLeft {} => Self::MoveColumnToMonitorLeft,
            swayward_ipc::Action::MoveColumnToMonitorRight {} => Self::MoveColumnToMonitorRight,
            swayward_ipc::Action::MoveColumnToMonitorDown {} => Self::MoveColumnToMonitorDown,
            swayward_ipc::Action::MoveColumnToMonitorUp {} => Self::MoveColumnToMonitorUp,
            swayward_ipc::Action::MoveColumnToMonitorPrevious {} => {
                Self::MoveColumnToMonitorPrevious
            }
            swayward_ipc::Action::MoveColumnToMonitorNext {} => Self::MoveColumnToMonitorNext,
            swayward_ipc::Action::MoveColumnToMonitor { output } => {
                Self::MoveColumnToMonitor(output)
            }
            swayward_ipc::Action::SetWindowWidth { id: None, change } => {
                Self::SetWindowWidth(change)
            }
            swayward_ipc::Action::SetWindowWidth {
                id: Some(id),
                change,
            } => Self::SetWindowWidthById { id, change },
            swayward_ipc::Action::SetWindowHeight { id: None, change } => {
                Self::SetWindowHeight(change)
            }
            swayward_ipc::Action::SetWindowHeight {
                id: Some(id),
                change,
            } => Self::SetWindowHeightById { id, change },
            swayward_ipc::Action::ResetWindowHeight { id: None } => Self::ResetWindowHeight,
            swayward_ipc::Action::ResetWindowHeight { id: Some(id) } => {
                Self::ResetWindowHeightById(id)
            }
            swayward_ipc::Action::SwitchPresetColumnWidth {} => Self::SwitchPresetColumnWidth,
            swayward_ipc::Action::SwitchPresetColumnWidthBack {} => {
                Self::SwitchPresetColumnWidthBack
            }
            swayward_ipc::Action::SwitchPresetWindowWidth { id: None } => {
                Self::SwitchPresetWindowWidth
            }
            swayward_ipc::Action::SwitchPresetWindowWidthBack { id: None } => {
                Self::SwitchPresetWindowWidthBack
            }
            swayward_ipc::Action::SwitchPresetWindowWidth { id: Some(id) } => {
                Self::SwitchPresetWindowWidthById(id)
            }
            swayward_ipc::Action::SwitchPresetWindowWidthBack { id: Some(id) } => {
                Self::SwitchPresetWindowWidthBackById(id)
            }
            swayward_ipc::Action::SwitchPresetWindowHeight { id: None } => {
                Self::SwitchPresetWindowHeight
            }
            swayward_ipc::Action::SwitchPresetWindowHeightBack { id: None } => {
                Self::SwitchPresetWindowHeightBack
            }
            swayward_ipc::Action::SwitchPresetWindowHeight { id: Some(id) } => {
                Self::SwitchPresetWindowHeightById(id)
            }
            swayward_ipc::Action::SwitchPresetWindowHeightBack { id: Some(id) } => {
                Self::SwitchPresetWindowHeightBackById(id)
            }
            swayward_ipc::Action::MaximizeColumn {} => Self::MaximizeColumn,
            swayward_ipc::Action::MaximizeWindowToEdges { id: None } => Self::MaximizeWindowToEdges,
            swayward_ipc::Action::MaximizeWindowToEdges { id: Some(id) } => {
                Self::MaximizeWindowToEdgesById(id)
            }
            swayward_ipc::Action::SetColumnWidth { change } => Self::SetColumnWidth(change),
            swayward_ipc::Action::ExpandColumnToAvailableWidth {} => {
                Self::ExpandColumnToAvailableWidth
            }
            swayward_ipc::Action::SwitchLayout { layout } => Self::SwitchLayout(layout),
            swayward_ipc::Action::ShowHotkeyOverlay {} => Self::ShowHotkeyOverlay,
            swayward_ipc::Action::MoveWorkspaceToMonitorLeft {} => Self::MoveWorkspaceToMonitorLeft,
            swayward_ipc::Action::MoveWorkspaceToMonitorRight {} => {
                Self::MoveWorkspaceToMonitorRight
            }
            swayward_ipc::Action::MoveWorkspaceToMonitorDown {} => Self::MoveWorkspaceToMonitorDown,
            swayward_ipc::Action::MoveWorkspaceToMonitorUp {} => Self::MoveWorkspaceToMonitorUp,
            swayward_ipc::Action::MoveWorkspaceToMonitorPrevious {} => {
                Self::MoveWorkspaceToMonitorPrevious
            }
            swayward_ipc::Action::MoveWorkspaceToIndex {
                index,
                reference: Some(reference),
            } => Self::MoveWorkspaceToIndexByRef {
                new_idx: index,
                reference: WorkspaceReference::from(reference),
            },
            swayward_ipc::Action::MoveWorkspaceToIndex {
                index,
                reference: None,
            } => Self::MoveWorkspaceToIndex(index),
            swayward_ipc::Action::MoveWorkspaceToMonitor {
                output,
                reference: Some(reference),
            } => Self::MoveWorkspaceToMonitorByRef {
                output_name: output,
                reference: WorkspaceReference::from(reference),
            },
            swayward_ipc::Action::MoveWorkspaceToMonitor {
                output,
                reference: None,
            } => Self::MoveWorkspaceToMonitor(output),
            swayward_ipc::Action::MoveWorkspaceToMonitorNext {} => Self::MoveWorkspaceToMonitorNext,
            swayward_ipc::Action::ToggleDebugTint {} => Self::ToggleDebugTint,
            swayward_ipc::Action::DebugToggleOpaqueRegions {} => Self::DebugToggleOpaqueRegions,
            swayward_ipc::Action::DebugToggleDamage {} => Self::DebugToggleDamage,
            swayward_ipc::Action::ToggleWindowFloating { id: None } => Self::ToggleWindowFloating,
            swayward_ipc::Action::ToggleWindowFloating { id: Some(id) } => {
                Self::ToggleWindowFloatingById(id)
            }
            swayward_ipc::Action::MoveWindowToFloating { id: None } => Self::MoveWindowToFloating,
            swayward_ipc::Action::MoveWindowToFloating { id: Some(id) } => {
                Self::MoveWindowToFloatingById(id)
            }
            swayward_ipc::Action::MoveWindowToTiling { id: None } => Self::MoveWindowToTiling,
            swayward_ipc::Action::MoveWindowToTiling { id: Some(id) } => {
                Self::MoveWindowToTilingById(id)
            }
            swayward_ipc::Action::FocusFloating {} => Self::FocusFloating,
            swayward_ipc::Action::FocusTiling {} => Self::FocusTiling,
            swayward_ipc::Action::SwitchFocusBetweenFloatingAndTiling {} => {
                Self::SwitchFocusBetweenFloatingAndTiling
            }
            swayward_ipc::Action::MoveFloatingWindow { id, x, y } => {
                Self::MoveFloatingWindowById { id, x, y }
            }
            swayward_ipc::Action::ToggleWindowRuleOpacity { id: None } => {
                Self::ToggleWindowRuleOpacity
            }
            swayward_ipc::Action::ToggleWindowRuleOpacity { id: Some(id) } => {
                Self::ToggleWindowRuleOpacityById(id)
            }
            swayward_ipc::Action::SetDynamicCastWindow { id: None } => Self::SetDynamicCastWindow,
            swayward_ipc::Action::SetDynamicCastWindow { id: Some(id) } => {
                Self::SetDynamicCastWindowById(id)
            }
            swayward_ipc::Action::SetDynamicCastMonitor { output } => {
                Self::SetDynamicCastMonitor(output)
            }
            swayward_ipc::Action::ClearDynamicCastTarget {} => Self::ClearDynamicCastTarget,
            swayward_ipc::Action::StopCast { session_id } => Self::StopCast(session_id),
            swayward_ipc::Action::ToggleOverview {} => Self::ToggleOverview,
            swayward_ipc::Action::OpenOverview {} => Self::OpenOverview,
            swayward_ipc::Action::CloseOverview {} => Self::CloseOverview,
            swayward_ipc::Action::ToggleWindowUrgent { id } => Self::ToggleWindowUrgent(id),
            swayward_ipc::Action::SetWindowUrgent { id } => Self::SetWindowUrgent(id),
            swayward_ipc::Action::UnsetWindowUrgent { id } => Self::UnsetWindowUrgent(id),
            swayward_ipc::Action::LoadConfigFile { path } => Self::LoadConfigFile(path),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WorkspaceReference {
    Id(u64),
    Index(u8),
    Name(String),
}

impl From<WorkspaceReferenceArg> for WorkspaceReference {
    fn from(reference: WorkspaceReferenceArg) -> WorkspaceReference {
        match reference {
            WorkspaceReferenceArg::Id(id) => Self::Id(id),
            WorkspaceReferenceArg::Index(i) => Self::Index(i),
            WorkspaceReferenceArg::Name(n) => Self::Name(n),
        }
    }
}

impl<S: knuffel::traits::ErrorSpan> knuffel::DecodeScalar<S> for WorkspaceReference {
    fn type_check(
        type_name: &Option<knuffel::span::Spanned<knuffel::ast::TypeName, S>>,
        ctx: &mut knuffel::decode::Context<S>,
    ) {
        if let Some(type_name) = &type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }
    }

    fn raw_decode(
        val: &knuffel::span::Spanned<knuffel::ast::Literal, S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<WorkspaceReference, DecodeError<S>> {
        match &**val {
            knuffel::ast::Literal::String(ref s) => Ok(WorkspaceReference::Name(s.clone().into())),
            knuffel::ast::Literal::Int(ref value) => match value.try_into() {
                Ok(v) => Ok(WorkspaceReference::Index(v)),
                Err(e) => {
                    ctx.emit_error(DecodeError::conversion(val, e));
                    Ok(WorkspaceReference::Index(0))
                }
            },
            _ => {
                ctx.emit_error(DecodeError::unsupported(
                    val,
                    "Unsupported value, only numbers and strings are recognized",
                ));
                Ok(WorkspaceReference::Index(0))
            }
        }
    }
}

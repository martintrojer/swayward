use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use bitflags::bitflags;
use knuffel::errors::DecodeError;
use miette::miette;
use smithay::input::keyboard::keysyms::KEY_NoSymbol;
use smithay::input::keyboard::xkb::{keysym_from_name, KEYSYM_CASE_INSENSITIVE, KEYSYM_NO_FLAGS};
use smithay::input::keyboard::Keysym;
use swayward_ipc::{
    ColumnDisplay, LayoutSwitchTarget, PositionChange, SizeChange, WorkspaceReferenceArg,
};

use crate::recent_windows::{MruDirection, MruFilter, MruScope};
use crate::utils::{expect_only_children, MergeWith};

#[derive(Debug, Default, PartialEq)]
pub struct Binds(pub Vec<Bind>);

#[derive(Debug, PartialEq)]
pub struct BindingMode {
    pub name: String,
    pub binds: Binds,
}

impl<S> knuffel::Decode<S> for BindingMode
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        let name = match &node.arguments[..] {
            [argument] => knuffel::traits::DecodeScalar::decode(argument, ctx)?,
            _ => {
                return Err(DecodeError::unexpected(
                    node,
                    "mode",
                    "expected mode \"<name>\" { ... }",
                ));
            }
        };
        for property in &node.properties {
            ctx.emit_error(DecodeError::unexpected(
                property.0,
                "property",
                "no properties expected for mode",
            ));
        }
        Ok(Self {
            name,
            binds: Binds::decode_children(node, ctx),
        })
    }
}

#[derive(Clone, PartialEq)]
pub struct Bind {
    pub key: Key,
    pub action: Action,
    pub mouse_regions: MouseRegions,
    /// Sway-compatible input identifier, or `"*"` to match every device.
    pub input_device: String,
    /// Zero-based XKB layout group. `None` matches every active group.
    pub group: Option<u8>,
    pub release: bool,
    pub repeat: bool,
    pub cooldown: Option<Duration>,
    pub allow_when_locked: bool,
    pub allow_inhibiting: bool,
    pub hotkey_overlay_title: Option<Option<String>>,
}

impl std::fmt::Debug for Bind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bind")
            .field("key", &self.key)
            .field("action", &self.action)
            .field("repeat", &self.repeat)
            .field("cooldown", &self.cooldown)
            .field("allow_when_locked", &self.allow_when_locked)
            .field("allow_inhibiting", &self.allow_inhibiting)
            .field("hotkey_overlay_title", &self.hotkey_overlay_title)
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct Key {
    pub trigger: Trigger,
    pub modifiers: Modifiers,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum Trigger {
    Keysym(Keysym),
    Keycode(u32),
    MouseLeft,
    MouseRight,
    MouseMiddle,
    MouseBack,
    MouseForward,
    WheelScrollDown,
    WheelScrollUp,
    WheelScrollLeft,
    WheelScrollRight,
    TouchpadScrollDown,
    TouchpadScrollUp,
    TouchpadScrollLeft,
    TouchpadScrollRight,
    TabletStylusButton1,
    TabletStylusButton2,
    TabletStylusButton3,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct MouseRegions: u8 {
        const TITLEBAR = 1;
        const BORDER = 1 << 1;
        const CONTENTS = 1 << 2;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Modifiers : u16 {
        const CTRL = 1;
        const SHIFT = 1 << 1;
        const ALT = 1 << 2;
        const SUPER = 1 << 3;
        const NUM = 1 << 4;
        const CAPS = 1 << 5;
        const ISO_LEVEL3_SHIFT = 1 << 6;
        const ISO_LEVEL5_SHIFT = 1 << 7;
        const COMPOSITOR = 1 << 8;
    }
}

#[derive(knuffel::Decode, Debug, Default, Clone, PartialEq)]
pub struct SwitchBinds {
    #[knuffel(child)]
    pub lid_open: Option<SwitchAction>,
    #[knuffel(child)]
    pub lid_close: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_on: Option<SwitchAction>,
    #[knuffel(child)]
    pub tablet_mode_off: Option<SwitchAction>,
}

impl MergeWith<SwitchBinds> for SwitchBinds {
    fn merge_with(&mut self, part: &SwitchBinds) {
        merge_clone_opt!(
            (self, part),
            lid_open,
            lid_close,
            tablet_mode_on,
            tablet_mode_off,
        );
    }
}

#[derive(knuffel::Decode, Debug, Clone, PartialEq)]
pub struct SwitchAction {
    #[knuffel(child, unwrap(arguments))]
    pub spawn: Vec<String>,
}

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

impl Binds {
    fn decode_children<S: knuffel::traits::ErrorSpan>(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Self {
        type BindIdentity = (Key, String, Option<u8>, bool, bool, bool);
        let mut seen_keys: HashMap<BindIdentity, &knuffel::ast::SpannedNode<S>> = HashMap::new();
        let mut binds = Vec::new();

        for child in node.children() {
            match <Bind as knuffel::Decode<S>>::decode_node(child, ctx) {
                Err(e) => ctx.emit_error(e),
                Ok(bind) => match seen_keys.entry((
                    bind.key,
                    bind.input_device.clone(),
                    bind.group,
                    bind.release,
                    bind.allow_when_locked,
                    bind.allow_inhibiting,
                )) {
                    Entry::Occupied(entry) => {
                        // Even though it's technically incorrect, we use
                        // `DecodeError::Missing` here because it labels the bind with
                        // "node starts here", which is the least bad option
                        ctx.emit_error(DecodeError::missing(
                            entry.get(),
                            "keybind first defined here",
                        ));
                        ctx.emit_error(DecodeError::unexpected(
                            &child.node_name,
                            "keybind",
                            "duplicate keybind later defined here",
                        ));
                    }
                    Entry::Vacant(entry) => {
                        entry.insert(child);
                        binds.push(bind);
                    }
                },
            }
        }
        Self(binds)
    }
}

impl<S> knuffel::Decode<S> for Binds
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        expect_only_children(node, ctx);
        Ok(Self::decode_children(node, ctx))
    }
}

impl<S> knuffel::Decode<S> for Bind
where
    S: knuffel::traits::ErrorSpan,
{
    fn decode_node(
        node: &knuffel::ast::SpannedNode<S>,
        ctx: &mut knuffel::decode::Context<S>,
    ) -> Result<Self, DecodeError<S>> {
        if let Some(type_name) = &node.type_name {
            ctx.emit_error(DecodeError::unexpected(
                type_name,
                "type name",
                "no type name expected for this node",
            ));
        }

        for val in node.arguments.iter() {
            ctx.emit_error(DecodeError::unexpected(
                &val.literal,
                "argument",
                "no arguments expected for this node",
            ));
        }

        let mut key_name = node.node_name.to_string();
        let mut group = None;
        let mut key_parts = key_name.split('+').collect::<Vec<_>>();
        key_parts.retain(|part| {
            let value = if *part == "Mode_switch" {
                Some("2")
            } else {
                part.strip_prefix("Group")
            };
            let Some(value) = value else {
                return true;
            };
            match value.parse::<u8>() {
                Ok(value @ 1..=4) if group.is_none() => group = Some(value - 1),
                _ => ctx.emit_error(DecodeError::unexpected(
                    &node.node_name,
                    "keybind",
                    "exactly one XKB group from Group1 to Group4 is allowed",
                )),
            }
            false
        });
        key_name = key_parts.join("+");
        let key = key_name
            .parse::<Key>()
            .map_err(|e| DecodeError::conversion(&node.node_name, e.wrap_err("invalid keybind")))?;

        let mut mouse_regions = MouseRegions::empty();
        let mut input_device = "*".to_owned();
        let mut release = false;
        let mut repeat = true;
        let mut cooldown = None;
        let mut allow_when_locked = false;
        let mut allow_when_locked_node = None;
        let mut allow_inhibiting = true;
        let mut hotkey_overlay_title = None;
        for (name, val) in &node.properties {
            match &***name {
                "mouse-regions" => {
                    let regions: String = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                    for region in regions.split('+') {
                        mouse_regions |= match region {
                            "titlebar" => MouseRegions::TITLEBAR,
                            "border" => MouseRegions::BORDER,
                            "contents" => MouseRegions::CONTENTS,
                            _ => {
                                ctx.emit_error(DecodeError::unexpected(
                                    name,
                                    "property",
                                    "mouse-regions must contain titlebar, border, or contents",
                                ));
                                MouseRegions::empty()
                            }
                        };
                    }
                }
                "input-device" => {
                    input_device = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "release" => {
                    release = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "repeat" => {
                    repeat = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "cooldown-ms" => {
                    cooldown = Some(Duration::from_millis(
                        knuffel::traits::DecodeScalar::decode(val, ctx)?,
                    ));
                }
                "allow-when-locked" => {
                    allow_when_locked = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                    allow_when_locked_node = Some(name);
                }
                "allow-inhibiting" => {
                    allow_inhibiting = knuffel::traits::DecodeScalar::decode(val, ctx)?;
                }
                "hotkey-overlay-title" => {
                    hotkey_overlay_title = Some(knuffel::traits::DecodeScalar::decode(val, ctx)?);
                }
                name_str => {
                    ctx.emit_error(DecodeError::unexpected(
                        name,
                        "property",
                        format!("unexpected property `{}`", name_str.escape_default()),
                    ));
                }
            }
        }

        if release {
            repeat = false;
        }

        let mut children = node.children();

        // If the action is invalid but the key is fine, we still want to return something.
        // That way, the parent can handle the existence of duplicate keybinds,
        // even if their contents are not valid.
        let dummy = Self {
            key,
            action: Action::Spawn(vec![]),
            mouse_regions,
            input_device: input_device.clone(),
            group,
            release,
            repeat: true,
            cooldown: None,
            allow_when_locked: false,
            allow_inhibiting: true,
            hotkey_overlay_title: None,
        };

        if let Some(child) = children.next() {
            for unwanted_child in children {
                ctx.emit_error(DecodeError::unexpected(
                    unwanted_child,
                    "node",
                    "only one action is allowed per keybind",
                ));
            }
            if child.node_name.as_ref() == "command" {
                let command = match &child.arguments[..] {
                    [argument] => match &*argument.literal {
                        knuffel::ast::Literal::String(command) => command.to_string(),
                        _ => {
                            ctx.emit_error(DecodeError::unexpected(
                                &argument.literal,
                                "argument",
                                "command must be a quoted string",
                            ));
                            return Ok(dummy);
                        }
                    },
                    _ => {
                        ctx.emit_error(DecodeError::unexpected(
                            child,
                            "node",
                            "expected command \"<sway command>\"",
                        ));
                        return Ok(dummy);
                    }
                };
                if child.children.is_some() || !child.properties.is_empty() {
                    ctx.emit_error(DecodeError::unexpected(
                        child,
                        "node",
                        "command accepts one quoted string and no children or properties",
                    ));
                    return Ok(dummy);
                }
                return Ok(Self {
                    key,
                    action: Action::SwayCommand(command),
                    mouse_regions,
                    input_device,
                    group,
                    release,
                    repeat,
                    cooldown,
                    allow_when_locked,
                    allow_inhibiting,
                    hotkey_overlay_title,
                });
            }
            match Action::decode_node(child, ctx) {
                Ok(action) => {
                    if !matches!(action, Action::Spawn(_) | Action::SpawnSh(_)) {
                        if let Some(node) = allow_when_locked_node {
                            ctx.emit_error(DecodeError::unexpected(
                                node,
                                "property",
                                "allow-when-locked can only be set on spawn binds",
                            ));
                        }
                    }

                    // The toggle-inhibit action must always be uninhibitable.
                    // Otherwise, it would be impossible to trigger it.
                    if matches!(action, Action::ToggleKeyboardShortcutsInhibit) {
                        allow_inhibiting = false;
                    }

                    Ok(Self {
                        key,
                        action,
                        mouse_regions,
                        input_device,
                        group,
                        release,
                        repeat,
                        cooldown,
                        allow_when_locked,
                        allow_inhibiting,
                        hotkey_overlay_title,
                    })
                }
                Err(e) => {
                    ctx.emit_error(e);
                    Ok(dummy)
                }
            }
        } else {
            ctx.emit_error(DecodeError::missing(
                node,
                "expected an action for this keybind",
            ));
            Ok(dummy)
        }
    }
}

impl FromStr for Key {
    type Err = miette::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Modifiers::empty();

        let mut split = s.split('+');
        let key = split.next_back().unwrap();

        for part in split {
            let part = part.trim();
            if part.eq_ignore_ascii_case("mod") {
                modifiers |= Modifiers::COMPOSITOR
            } else if part.eq_ignore_ascii_case("ctrl") || part.eq_ignore_ascii_case("control") {
                modifiers |= Modifiers::CTRL;
            } else if part.eq_ignore_ascii_case("shift") {
                modifiers |= Modifiers::SHIFT;
            } else if part.eq_ignore_ascii_case("lock") {
                modifiers |= Modifiers::CAPS;
            } else if part.eq_ignore_ascii_case("alt") || part.eq_ignore_ascii_case("mod1") {
                modifiers |= Modifiers::ALT;
            } else if part.eq_ignore_ascii_case("super")
                || part.eq_ignore_ascii_case("logo")
                || part.eq_ignore_ascii_case("win")
                || part.eq_ignore_ascii_case("mod4")
            {
                modifiers |= Modifiers::SUPER;
            } else if part.eq_ignore_ascii_case("num") || part.eq_ignore_ascii_case("mod2") {
                modifiers |= Modifiers::NUM;
            } else if part.eq_ignore_ascii_case("iso_level3_shift")
                || part.eq_ignore_ascii_case("mod5")
            {
                modifiers |= Modifiers::ISO_LEVEL3_SHIFT;
            } else if part.eq_ignore_ascii_case("iso_level5_shift")
                || part.eq_ignore_ascii_case("mod3")
            {
                modifiers |= Modifiers::ISO_LEVEL5_SHIFT;
            } else {
                return Err(miette!("invalid modifier: {part}"));
            }
        }

        let trigger = if let Some(keycode) = key.strip_prefix("code:") {
            Trigger::Keycode(
                keycode
                    .parse()
                    .map_err(|_| miette!("invalid keycode: {keycode}"))?,
            )
        } else if key.eq_ignore_ascii_case("MouseLeft") {
            Trigger::MouseLeft
        } else if key.eq_ignore_ascii_case("MouseRight") {
            Trigger::MouseRight
        } else if key.eq_ignore_ascii_case("MouseMiddle") {
            Trigger::MouseMiddle
        } else if key.eq_ignore_ascii_case("MouseBack") {
            Trigger::MouseBack
        } else if key.eq_ignore_ascii_case("MouseForward") {
            Trigger::MouseForward
        } else if key.eq_ignore_ascii_case("WheelScrollDown") {
            Trigger::WheelScrollDown
        } else if key.eq_ignore_ascii_case("WheelScrollUp") {
            Trigger::WheelScrollUp
        } else if key.eq_ignore_ascii_case("WheelScrollLeft") {
            Trigger::WheelScrollLeft
        } else if key.eq_ignore_ascii_case("WheelScrollRight") {
            Trigger::WheelScrollRight
        } else if key.eq_ignore_ascii_case("TouchpadScrollDown") {
            Trigger::TouchpadScrollDown
        } else if key.eq_ignore_ascii_case("TouchpadScrollUp") {
            Trigger::TouchpadScrollUp
        } else if key.eq_ignore_ascii_case("TouchpadScrollLeft") {
            Trigger::TouchpadScrollLeft
        } else if key.eq_ignore_ascii_case("TouchpadScrollRight") {
            Trigger::TouchpadScrollRight
        } else if key.eq_ignore_ascii_case("TabletStylusButton1") {
            Trigger::TabletStylusButton1
        } else if key.eq_ignore_ascii_case("TabletStylusButton2") {
            Trigger::TabletStylusButton2
        } else if key.eq_ignore_ascii_case("TabletStylusButton3") {
            Trigger::TabletStylusButton3
        } else {
            let mut keysym = keysym_from_name(key, KEYSYM_CASE_INSENSITIVE);
            // The keyboard event handling code can receive either
            // XF86ScreenSaver or XF86Screensaver, because there is no
            // case mapping defined between these keysyms. If we just
            // use the case-insensitive version of keysym_from_name it
            // is not possible to bind the uppercase version, because the
            // case-insensitive match prefers the lowercase version when
            // there is a choice.
            //
            // Therefore, when we match this key with the initial
            // case-insensitive match we try a further case-sensitive match
            // (so that either key can be bound). If that fails, we change
            // to the uppercase version because:
            //
            // - A comment in xkb_keysym_from_name (in libxkbcommon) tells us that the uppercase
            //   version is the "best" of the two. [0]
            // - The xkbcommon crate only has a constant for ScreenSaver. [1]
            //
            // [0]: https://github.com/xkbcommon/libxkbcommon/blob/45a118d5325b051343b4b174f60c1434196fa7d4/src/keysym.c#L276
            // [1]: https://docs.rs/xkbcommon/latest/xkbcommon/xkb/keysyms/index.html#:~:text=KEY%5FXF86ScreenSaver
            //
            // See https://github.com/niri-wm/niri/issues/1969
            if keysym == Keysym::XF86_Screensaver {
                keysym = keysym_from_name(key, KEYSYM_NO_FLAGS);
                if keysym.raw() == KEY_NoSymbol {
                    keysym = Keysym::XF86_ScreenSaver;
                }
            }
            if keysym.raw() == KEY_NoSymbol {
                return Err(miette!("invalid key: {key}"));
            }
            Trigger::Keysym(keysym)
        };

        Ok(Key { trigger, modifiers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_xf86_screensaver() {
        assert_eq!(
            "XF86ScreenSaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_ScreenSaver),
                modifiers: Modifiers::empty(),
            },
        );
        assert_eq!(
            "XF86Screensaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_Screensaver),
                modifiers: Modifiers::empty(),
            }
        );
        assert_eq!(
            "xf86screensaver".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::XF86_ScreenSaver),
                modifiers: Modifiers::empty(),
            }
        );
    }

    #[test]
    fn parse_numlock_and_sway_modifier_aliases() {
        for (name, modifier) in [
            ("Shift", Modifiers::SHIFT),
            ("Lock", Modifiers::CAPS),
            ("Control", Modifiers::CTRL),
            ("Ctrl", Modifiers::CTRL),
            ("Alt", Modifiers::ALT),
            ("Mod1", Modifiers::ALT),
            ("Num", Modifiers::NUM),
            ("Mod2", Modifiers::NUM),
            ("Mod3", Modifiers::ISO_LEVEL5_SHIFT),
            ("Super", Modifiers::SUPER),
            ("Logo", Modifiers::SUPER),
            ("Mod4", Modifiers::SUPER),
            ("Mod5", Modifiers::ISO_LEVEL3_SHIFT),
        ] {
            assert_eq!(
                format!("{name}+A").parse::<Key>().unwrap().modifiers,
                modifier
            );
        }
        assert!("Mod6+A".parse::<Key>().is_err());
    }

    #[test]
    fn parse_iso_level_shifts() {
        assert_eq!(
            "ISO_Level3_Shift+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL3_SHIFT
            },
        );

        assert_eq!(
            "ISO_Level5_Shift+A".parse::<Key>().unwrap(),
            Key {
                trigger: Trigger::Keysym(Keysym::a),
                modifiers: Modifiers::ISO_LEVEL5_SHIFT
            },
        );
    }
}

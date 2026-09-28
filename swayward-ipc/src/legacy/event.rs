use super::*;

/// A compositor event.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "json-schema", derive(schemars::JsonSchema))]
pub enum Event {
    /// The workspace configuration has changed.
    WorkspacesChanged {
        /// The new workspace configuration.
        ///
        /// This configuration completely replaces the previous configuration. I.e. if any
        /// workspaces are missing from here, then they were deleted.
        workspaces: Vec<Workspace>,
    },
    /// A workspace became empty and was removed.
    WorkspaceEmptied {
        /// The removed workspace's last IPC tree representation.
        current: Box<crate::Node>,
    },
    /// The configuration was reloaded.
    WorkspaceReloaded,
    /// A workspace was created.
    WorkspaceInitialized {
        /// The new workspace's IPC tree representation.
        current: Box<crate::Node>,
    },
    /// A workspace was renamed.
    WorkspaceRenamed {
        /// The renamed workspace's IPC tree representation.
        current: Box<crate::Node>,
    },
    /// Focus moved between workspaces.
    WorkspaceFocusChanged {
        /// The previously focused workspace.
        old: Option<Box<crate::Node>>,
        /// The newly focused workspace.
        current: Box<crate::Node>,
    },
    /// A workspace moved to another output.
    WorkspaceMoved {
        /// The moved workspace's IPC tree representation.
        current: Box<crate::Node>,
    },
    /// The workspace urgency changed.
    WorkspaceUrgencyChanged {
        /// Internal id of the changed workspace.
        id: u64,
        /// The changed workspace's IPC tree representation.
        current: Box<crate::Node>,
    },
    /// A workspace was activated on an output.
    ///
    /// This doesn't always mean the workspace became focused, just that it's now the active
    /// workspace on its output. All other workspaces on the same output become inactive.
    WorkspaceActivated {
        /// Id of the newly active workspace.
        id: u64,
        /// Whether this workspace also became focused.
        ///
        /// If `true`, this is now the single focused workspace. All other workspaces are no longer
        /// focused, but they may remain active on their respective outputs.
        focused: bool,
    },
    /// An active window changed on a workspace.
    WorkspaceActiveWindowChanged {
        /// Id of the workspace on which the active window changed.
        workspace_id: u64,
        /// Id of the new active window, if any.
        active_window_id: Option<u64>,
    },
    /// The window configuration has changed.
    WindowsChanged {
        /// The new window configuration.
        ///
        /// This configuration completely replaces the previous configuration. I.e. if any windows
        /// are missing from here, then they were closed.
        windows: Vec<Window>,
    },
    /// A new toplevel window was opened, or an existing toplevel window changed.
    WindowOpenedOrChanged {
        /// The new or updated window.
        ///
        /// If the window is focused, all other windows are no longer focused.
        window: Window,
    },
    /// A sway-compatible window event with its serialized tree node.
    SwayWindowChanged {
        /// Sway's window change name.
        change: String,
        /// The affected container.
        container: serde_json::Value,
    },
    /// A window moved to another workspace.
    WindowMoved {
        /// Id of the moved window.
        id: i64,
    },
    /// A toplevel window was closed.
    WindowClosed {
        /// Id of the removed window.
        id: u64,
    },
    /// Window focus changed.
    ///
    /// All other windows are no longer focused.
    WindowFocusChanged {
        /// Id of the newly focused window, or `None` if no window is now focused.
        id: Option<u64>,
    },
    /// Window focus timestamp changed.
    ///
    /// This event is separate from [`Event::WindowFocusChanged`] because the focus timestamp only
    /// updates after some debounce time so that quick window switching doesn't mark intermediate
    /// windows as recently focused.
    WindowFocusTimestampChanged {
        /// Id of the window.
        id: u64,
        /// The new focus timestamp.
        focus_timestamp: Option<Timestamp>,
    },
    /// Window urgency changed.
    WindowUrgencyChanged {
        /// Id of the window.
        id: u64,
        /// The new urgency state of the window.
        urgent: bool,
    },
    /// The layout of one or more windows has changed.
    WindowLayoutsChanged {
        /// Pairs consisting of a window id and new layout information for the window.
        changes: Vec<(u64, WindowLayout)>,
    },
    /// The configured keyboard layouts have changed.
    KeyboardLayoutsChanged {
        /// The new keyboard layout configuration.
        keyboard_layouts: KeyboardLayouts,
    },
    /// The keyboard layout switched.
    KeyboardLayoutSwitched {
        /// Index of the newly active layout.
        idx: u32,
    },
    /// A sway-compatible input event with its GET_INPUTS device payload.
    SwayInputChanged {
        /// Sway's input change name.
        change: String,
        /// The affected input device.
        input: serde_json::Value,
    },
    /// The output layout changed.
    OutputChanged,
    /// The compositor is shutting down.
    Shutdown {
        /// Sway shutdown reason.
        reason: String,
    },
    /// A sway IPC synchronization tick.
    Tick {
        /// Client-provided tick payload.
        payload: String,
        /// Whether this is the initial subscription tick.
        first: bool,
    },
    /// A sway-compatible binding ran.
    SwayBinding {
        /// Command attached to the binding.
        command: String,
        /// Active modifier names.
        event_state_mask: Vec<String>,
        /// Configured input codes.
        input_codes: Vec<u32>,
        /// First configured input code, or zero for symbolic bindings.
        input_code: u32,
        /// Configured input symbols.
        symbols: Vec<String>,
        /// First configured input symbol.
        symbol: Option<String>,
        /// Sway input type name.
        input_type: String,
    },
    /// The sway-compatible binding mode changed.
    BindingModeChanged {
        /// Name of the newly active mode.
        mode: String,
        /// Whether clients should render the mode as Pango markup.
        pango_markup: bool,
    },
    /// The overview was opened or closed.
    OverviewOpenedOrClosed {
        /// The new state of the overview.
        is_open: bool,
    },
    /// The configuration was reloaded.
    ///
    /// You will always receive this event when connecting to the event stream, indicating the last
    /// config load attempt.
    ConfigLoaded {
        /// Whether the loading failed.
        ///
        /// For example, the config file couldn't be parsed.
        failed: bool,
    },
    /// A screenshot was captured.
    ScreenshotCaptured {
        /// The file path where the screenshot was saved, if it was written to disk.
        ///
        /// If `None`, the screenshot was either only copied to the clipboard, or the path couldn't
        /// be converted to a `String` (e.g. contained invalid UTF-8 bytes).
        path: Option<String>,
    },
    /// The screencasts have changed.
    CastsChanged {
        /// The new screencast information.
        ///
        /// This configuration completely replaces the previous configuration. I.e. if any casts
        /// are missing from here, then they were stopped.
        casts: Vec<Cast>,
    },
    /// A screencast started, or an existing cast changed.
    CastStartedOrChanged {
        /// The cast that started or changed.
        cast: Cast,
    },
    /// A screencast stopped.
    CastStopped {
        /// Stream ID of the stopped screencast.
        stream_id: u64,
    },
}

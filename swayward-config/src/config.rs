use crate::*;

/// Fully resolved compositor configuration.
///
/// Use [`Config::load`] or [`ConfigPath::load`] instead of constructing this type when includes
/// and source diagnostics matter.
#[derive(Debug, PartialEq)]
pub struct Config {
    /// Input-device settings.
    pub input: Input,
    /// Output settings indexed by output match rule.
    pub outputs: Outputs,
    /// Commands and arguments launched directly at compositor startup.
    pub spawn_at_startup: Vec<SpawnAtStartup>,
    /// Shell commands launched at compositor startup.
    pub spawn_sh_at_startup: Vec<SpawnShAtStartup>,
    /// Window layout and decoration settings.
    pub layout: Layout,
    /// Whether the compositor asks clients to avoid client-side decorations.
    pub prefer_no_csd: bool,
    /// How new popups interact with a fullscreen window.
    pub popup_during_fullscreen: PopupDuringFullscreen,
    /// How activation requests affect focus.
    pub focus_on_window_activation: FocusOnWindowActivation,
    /// Time in milliseconds before an urgent window loses urgency automatically.
    pub urgent_timeout_ms: u32,
    /// Cursor appearance and visibility settings.
    pub cursor: Cursor,
    /// Expanded screenshot path template.
    pub screenshot_path: ScreenshotPath,
    /// Clipboard persistence settings.
    pub clipboard: Clipboard,
    /// Hotkey-overlay settings.
    pub hotkey_overlay: HotkeyOverlay,
    /// Configuration error notification settings.
    pub config_notification: ConfigNotification,
    /// Animation settings.
    pub animations: Animations,
    /// Blur settings.
    pub blur: Blur,
    /// Gesture settings.
    pub gestures: Gestures,
    /// Overview settings.
    pub overview: Overview,
    /// Environment variables set for child processes.
    pub environment: Environment,
    /// Xwayland-satellite startup settings.
    pub xwayland_satellite: XwaylandSatellite,
    /// Ordered window rules.
    pub window_rules: Vec<WindowRule>,
    /// Ordered layer-shell rules.
    pub layer_rules: Vec<LayerRule>,
    /// Bindings in the default mode.
    pub binds: Binds,
    /// Named binding modes.
    pub binding_modes: Vec<BindingMode>,
    /// Lid and tablet-mode switch bindings.
    pub switch_events: SwitchBinds,
    /// Debug and diagnostic settings.
    pub debug: Debug,
    /// Statically configured workspaces.
    pub workspaces: Vec<Workspace>,
    /// Recent-window switcher settings.
    pub recent_windows: RecentWindows,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            input: Default::default(),
            outputs: Default::default(),
            spawn_at_startup: Default::default(),
            spawn_sh_at_startup: Default::default(),
            layout: Default::default(),
            prefer_no_csd: Default::default(),
            popup_during_fullscreen: Default::default(),
            focus_on_window_activation: Default::default(),
            urgent_timeout_ms: 500,
            cursor: Default::default(),
            screenshot_path: Default::default(),
            clipboard: Default::default(),
            hotkey_overlay: Default::default(),
            config_notification: Default::default(),
            animations: Default::default(),
            blur: Default::default(),
            gestures: Default::default(),
            overview: Default::default(),
            environment: Default::default(),
            xwayland_satellite: Default::default(),
            window_rules: Default::default(),
            layer_rules: Default::default(),
            binds: Default::default(),
            binding_modes: Default::default(),
            switch_events: Default::default(),
            debug: Default::default(),
            workspaces: Default::default(),
            recent_windows: Default::default(),
        }
    }
}

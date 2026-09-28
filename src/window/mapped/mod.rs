use std::cell::{Cell, Ref, RefCell};
use std::time::Duration;

use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::space::SpaceElement as _;
use smithay::desktop::{PopupKind, PopupManager, Window, WindowSurfaceType};
use smithay::output::{self, Output};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource as _;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size, Transform};
use smithay::wayland::compositor::{remove_pre_commit_hook, with_states, HookId, SurfaceData};
use smithay::wayland::seat::WaylandFocus;
use smithay::wayland::shell::xdg::{
    SurfaceCachedState, ToplevelCachedState, ToplevelConfigure, ToplevelSurface,
    XdgToplevelSurfaceData,
};
use swayward_config::{Color, Config, CornerRadius, GradientInterpolation, WindowRule};
use wayland_backend::server::Credentials;

use super::{ResolvedWindowRules, WindowRef};
use crate::handlers::KdeDecorationsModeState;
use crate::layout::{
    ConfigureIntent, InteractiveResizeData, LayoutElement, LayoutElementRenderElement,
    LayoutElementRenderSnapshot, SizingMode,
};
use crate::render_helpers::background_effect::BackgroundEffectElement;
use crate::render_helpers::border::BorderRenderElement;
use crate::render_helpers::offscreen::OffscreenData;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::snapshot::RenderSnapshot;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::surface::{
    push_elements_from_surface_tree, render_snapshot_from_surface_tree,
};
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::{background_effect, BakedBuffer, RenderCtx, RenderTarget};
use crate::swayward::{ClientState, SecurityContextMetadata};
use crate::swayward_render_elements;
use crate::utils::id::IdCounter;
use crate::utils::transaction::Transaction;
use crate::utils::{
    get_credentials_for_surface, get_monotonic_time, send_scale_transform, update_tiled_state,
    with_toplevel_last_uncommitted_configure, with_toplevel_role, with_toplevel_role_and_current,
    ResizeEdge,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutsInhibitPolicy {
    #[default]
    Default,
    Enable,
    Disable,
}

#[derive(Debug)]
pub struct Mapped {
    pub window: Window,

    /// Unique ID of this `Mapped`.
    id: MappedId,

    /// Credentials of the process that created the Wayland connection.
    credentials: Option<Credentials>,

    /// Pre-commit hook that we have on all mapped toplevel surfaces.
    pre_commit_hook: HookId,

    /// Up-to-date rules.
    rules: ResolvedWindowRules,

    /// Whether the window rules need to be recomputed.
    ///
    /// This is not used in all cases; for example, app ID and title changes recompute the rules
    /// immediately, rather than setting this flag.
    need_to_recompute_rules: bool,

    /// Whether this window needs a configure this loop cycle.
    ///
    /// Certain Wayland requests require a configure in response, like un/fullscreen.
    needs_configure: bool,

    /// Whether this window needs a frame callback.
    ///
    /// We set this after sending a configure to give invisible windows a chance to respond to
    /// resizes immediately, without waiting for a 1 second throttled callback.
    needs_frame_callback: bool,

    /// Data of the offscreen element rendered in place of this window.
    ///
    /// If `None`, then the window is not offscreened.
    offscreen_data: RefCell<Option<OffscreenData>>,

    /// When this window became urgent.
    urgent_since: Option<Duration>,

    /// Marks displayed beside this window's title.
    titlebar_marks: Vec<String>,

    /// Whether this window has the keyboard focus.
    is_focused: bool,

    /// Whether this window is the active window in its column.
    is_active_in_column: bool,

    /// Whether this window is floating.
    is_floating: bool,

    /// Client geometry captured when the window first mapped.
    natural_size: Size<i32, Logical>,

    /// Whether the client created an xdg-decoration object for this toplevel.
    has_xdg_decoration: bool,

    /// Whether this window is a target of a window cast.
    is_window_cast_target: bool,

    /// Policy for future keyboard-shortcuts inhibitor requests from this window.
    shortcuts_inhibit_policy: ShortcutsInhibitPolicy,

    /// Whether this window should ignore opacity set through window rules.
    ignore_opacity_window_rule: bool,

    /// Opacity set through sway's runtime command.
    command_opacity: f32,

    /// Buffer to draw instead of the window when it should be blocked out.
    block_out_buffer: RefCell<SolidColorBuffer>,

    /// The blur config, passed for background effect rendering.
    blur_config: swayward_config::Blur,

    /// Whether the next configure should be animated, if the configured state changed.
    animate_next_configure: bool,

    /// Serials of commits that should be animated.
    animate_serials: Vec<Serial>,

    /// Snapshot right before an animated commit, without popups.
    animation_snapshot: Option<LayoutElementRenderSnapshot>,

    /// State for the logic to request a size once (for floating windows).
    request_size_once: Option<RequestSizeOnce>,

    /// Transaction that the next configure should take part in, if any.
    transaction_for_next_configure: Option<Transaction>,

    /// Pending transactions that have not been added as blockers for this window yet.
    pending_transactions: Vec<(Serial, Transaction)>,

    /// State of an ongoing interactive resize.
    interactive_resize: Option<InteractiveResize>,

    /// Last time interactive resize was started.
    ///
    /// Used for double-resize-click tracking.
    last_interactive_resize_start: Cell<Option<(Duration, ResizeEdge)>>,

    /// Whether this window is in windowed (fake) fullscreen.
    ///
    /// In this mode, the underlying window is told that it's fullscreen, while keeping it as
    /// a regular, non-fullscreen tile.
    is_windowed_fullscreen: bool,

    /// Whether this window is pending to go to windowed (fake) fullscreen.
    ///
    /// Several places in the layout code assume that is_fullscreen() can flip only on a commit.
    /// Which is something that we do want to flip when changing is_windowed_fullscreen. Flipping
    /// it right away would mean remembering to call layout.update_window() after any operation
    /// that may change is_windowed_fullscreen, which is quite tricky and error-prone, especially
    /// for deeply nested operations.
    ///
    /// It's also not clear what's the best way to go about it. Ideally we'd wait for configure ack
    /// and commit before "committing" to is_windowed_fullscreen, however, since it's not real
    /// Wayland state, we may end up with no Wayland state change to configure at all.
    ///
    /// For example: when the window is in real fullscreen, but its non-fullscreen size matches
    /// its fullscreen size. Then turning on is_windowed_fullscreen will both keep the
    /// fullscreen state, and keep the size (since it matches), resulting in no configure.
    ///
    /// So we work around this by emulating a configure-ack/commit cycle through
    /// is_pending_windowed_fullscreen and uncommitted_windowed_fullscreen. We ensure we send
    /// actual configures in all cases through needs_configure. This can result in unnecessary
    /// configures (like in the example above), but in most cases there will be a configure
    /// anyway to change the Fullscreen state and/or the size. What this gives us is being able
    /// to synchronize our windowed fullscreen state to the real window updates to avoid any
    /// flickering.
    is_pending_windowed_fullscreen: bool,

    /// Pending windowed fullscreen updates.
    ///
    /// These have been "sent" to the window in form of configures, but the window hadn't committed
    /// in response yet.
    uncommitted_windowed_fullscreen: Vec<(Serial, bool)>,

    /// Whether this window is maximized.
    ///
    /// We have to track this ourselves in addition to the Maximized toplevel state in order to
    /// support windowed fullscreen, since in windowed fullscreen the toplevel state is always
    /// Fullscreen. So we need this variable to be able to report accurate sizing mode and pending
    /// sizing mode.
    is_maximized: bool,

    /// Whether this window is pending to be maximized.
    ///
    /// We have to track this ourselves due to windowed fullscreen.
    is_pending_maximized: bool,

    /// Pending maximized updates.
    ///
    /// These have been "sent" to the window in form of configures, but the window hadn't committed
    /// in response yet.
    uncommitted_maximized: Vec<(Serial, bool)>,

    /// Sway title format applied to the client metadata.
    title_format: Option<String>,

    security_context: Option<SecurityContextMetadata>,

    /// Most recent monotonic time when the window had the focus.
    focus_timestamp: Option<Duration>,
}

swayward_render_elements! {
    WindowCastRenderElements<R> => {
        Layout = LayoutElementRenderElement<R>,
        // Blocked-out window with rounded corners.
        Border = BorderRenderElement,
    }
}

static MAPPED_ID_COUNTER: IdCounter = IdCounter::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MappedId(u64);

impl MappedId {
    pub fn next() -> MappedId {
        MappedId(MAPPED_ID_COUNTER.next())
    }

    pub fn get(self) -> u64 {
        self.0
    }

    /// Converts the ID to a string that can be used as an identifier in
    /// ext_foreign_toplevel_handle_v1::identifier
    ///
    /// > An identifier is a string that contains up to 32 printable ASCII bytes.
    /// > An identifier must not be an empty string.
    ///
    /// Since the ID is exposed to IPC, it's useful for this conversion to be stable and reversible.
    /// That way, clients can associate a foreign toplevel handle with an IPC window ID.
    ///
    /// We use the decimal representation of the ID, which is up to 20 characters long for u64::MAX.
    /// This is within the 32-character limit, and is nice because it matches up with how `swayward
    /// msg` prints the IDs to the console.
    ///
    /// This namespace can be extended in the future, with any non-numeric prefix to disambiguate.
    pub fn to_protocol_identifier(self) -> String {
        format!("{}", self.0)
    }
}

/// Interactive resize state.
#[derive(Debug)]
enum InteractiveResize {
    /// The resize is ongoing.
    Ongoing(InteractiveResizeData),
    /// The resize has stopped and we're waiting to send the last configure.
    WaitingForLastConfigure(InteractiveResizeData),
    /// We had sent the last resize configure and are waiting for the corresponding commit.
    WaitingForLastCommit {
        data: InteractiveResizeData,
        serial: Serial,
    },
}

impl InteractiveResize {
    fn data(&self) -> InteractiveResizeData {
        match self {
            InteractiveResize::Ongoing(data) => *data,
            InteractiveResize::WaitingForLastConfigure(data) => *data,
            InteractiveResize::WaitingForLastCommit { data, .. } => *data,
        }
    }
}

include!("title.rs");
include!("state.rs");
include!("layout_element.rs");

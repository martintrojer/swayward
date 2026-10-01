use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, mem, thread};

use _server_decoration::server::org_kde_kwin_server_decoration_manager::Mode as KdeDecorationsMode;
use anyhow::{bail, ensure, Context};
use calloop::futures::Scheduler;
use smithay::backend::allocator::Fourcc;
use smithay::backend::input::{InputTime, Keycode, Switch, SwitchState};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::utils::{
    select_dmabuf_feedback, CropRenderElement, Relocate, RelocateRenderElement,
    RescaleRenderElement,
};
use smithay::backend::renderer::element::{
    default_primary_scanout_output_compare, Element, Id, Kind, PrimaryScanoutOutput, RenderElement,
    RenderElementStates,
};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::sync::SyncPoint;
use smithay::backend::renderer::Color32F;
use smithay::desktop::utils::{
    bbox_from_surface_tree, output_update, send_dmabuf_feedback_surface_tree,
    send_frames_surface_tree, surface_presentation_feedback_flags_from_states,
    surface_primary_scanout_output, take_presentation_feedback_surface_tree,
    under_from_surface_tree, update_surface_primary_scanout_output, with_surfaces_surface_tree,
    OutputPresentationFeedback,
};
use smithay::desktop::{
    find_popup_root_surface, layer_map_for_output, LayerMap, LayerSurface, PopupGrab, PopupManager,
    PopupUngrabStrategy, Space, Window, WindowSurfaceType,
};
use smithay::input::keyboard::{Layout as KeyboardLayout, XkbConfig};
use smithay::input::pointer::{
    CursorIcon, CursorImageStatus, CursorImageSurfaceData, Focus,
    GrabStartData as PointerGrabStartData, MotionEvent,
};
use smithay::input::tablet::TabletSeatTrait;
use smithay::input::{Seat, SeatState};
use smithay::output::{self, Output, OutputModeSource, PhysicalProperties, Subpixel, WeakOutput};
use smithay::reexports::calloop::generic::Generic;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::{
    Interest, LoopHandle, LoopSignal, Mode, PostAction, RegistrationToken,
};
use smithay::reexports::wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::WmCapabilities;
use smithay::reexports::wayland_protocols_misc::server_decoration as _server_decoration;
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::server::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;
use smithay::reexports::wayland_server::backend::{
    ClientData, ClientId, DisconnectReason, GlobalId,
};
use smithay::reexports::wayland_server::protocol::wl_shm;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource};
use smithay::utils::{
    ClockSource, IsAlive as _, Logical, Monotonic, Physical, Point, Rectangle, Scale, Size,
    Transform, SERIAL_COUNTER,
};
use smithay::wayland::background_effect::BackgroundEffectState;
use smithay::wayland::compositor::{
    with_states, with_surface_tree_downward, CompositorClientState, CompositorHandler,
    CompositorState, HookId, SurfaceData, TraversalAction,
};
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::DmabufState;
use smithay::wayland::fractional_scale::FractionalScaleManagerState;
use smithay::wayland::idle_inhibit::IdleInhibitManagerState;
use smithay::wayland::idle_notify::IdleNotifierState;
use smithay::wayland::input_method::InputMethodManagerState;
use smithay::wayland::keyboard_shortcuts_inhibit::{
    KeyboardShortcutsInhibitState, KeyboardShortcutsInhibitor,
};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::pointer_constraints::{with_pointer_constraint, PointerConstraintsState};
use smithay::wayland::pointer_gestures::PointerGesturesState;
use smithay::wayland::presentation::PresentationState;
use smithay::wayland::relative_pointer::RelativePointerManagerState;
use smithay::wayland::security_context::SecurityContextState;
use smithay::wayland::selection::data_device::{set_data_device_selection, DataDeviceState};
use smithay::wayland::selection::ext_data_control::DataControlState as ExtDataControlState;
use smithay::wayland::selection::primary_selection::PrimarySelectionState;
use smithay::wayland::selection::wlr_data_control::DataControlState as WlrDataControlState;
use smithay::wayland::session_lock::{LockSurface, SessionLockManagerState, SessionLocker};
use smithay::wayland::shell::kde::decoration::KdeDecorationState;
use smithay::wayland::shell::wlr_layer::{self, Layer, WlrLayerShellState};
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::XdgShellState;
use smithay::wayland::shm::ShmState;
mod capture;
mod capture_requests;
mod config_reload;
mod focus;
mod outputs;
mod render;

#[cfg(test)]
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::socket::ListeningSocketSource;
use smithay::wayland::tablet_manager::TabletManagerState;
use smithay::wayland::text_input::TextInputManagerState;
use smithay::wayland::viewporter::ViewporterState;
use smithay::wayland::virtual_keyboard::VirtualKeyboardManagerState;
use smithay::wayland::xdg_activation::XdgActivationState;
use smithay::wayland::xdg_foreign::XdgForeignState;
use smithay::wayland::xdg_toplevel_tag::XdgToplevelTagManager;
use swayward_config::debug::PreviewRender;
use swayward_config::output::MaxBpc;
use swayward_config::{
    Bind, Config, Modifiers, OutputName, PositiveFloatOrInt, TrackLayout, WarpMouseToFocusMode,
    WorkspaceReference, Xkb,
};
use wayland_server::protocol::wl_output::WlOutput;

#[cfg(feature = "dbus")]
use crate::a11y::A11y;
use crate::animation::Clock;
use crate::backend::headless::HeadlessStartupOutput;
use crate::backend::tty::SurfaceDmabufFeedback;
use crate::backend::{Backend, Headless, RenderResult, Tty, Winit};
use crate::cursor::{CursorManager, CursorTextureCache, RenderCursor, XCursor};
#[cfg(feature = "dbus")]
use crate::dbus::freedesktop_locale1::Locale1ToNiri;
#[cfg(feature = "dbus")]
use crate::dbus::freedesktop_login1::Login1ToNiri;
#[cfg(feature = "dbus")]
use crate::dbus::gnome_shell_introspect::{self, IntrospectToNiri, NiriToIntrospect};
#[cfg(feature = "dbus")]
use crate::dbus::gnome_shell_screenshot::{NiriToScreenshot, ScreenshotToNiri};
use crate::frame_clock::FrameClock;
use crate::handlers::{configure_lock_surface, XDG_ACTIVATION_TOKEN_TIMEOUT};
use crate::input::pick_color_grab::PickColorGrab;
use crate::input::scroll_swipe_gesture::ScrollSwipeGesture;
use crate::input::scroll_tracker::ScrollTracker;
use crate::input::{
    apply_libinput_settings, mods_with_finger_scroll_binds, mods_with_mouse_binds,
    mods_with_tablet_stylus_binds, mods_with_wheel_binds, TabletData,
};
use crate::ipc::server::IpcServer;
use crate::layer::mapped::LayerSurfaceRenderElement;
use crate::layer::MappedLayer;
use crate::layout::tile::TileRenderElement;
use crate::layout::workspace::{Workspace, WorkspaceId};
use crate::layout::{
    HitType, Layout, LayoutElement as _, LayoutElementRenderElement, MonitorRenderElement,
};
use crate::protocols::ext_workspace::{self, ExtWorkspaceManagerState};
use crate::protocols::foreign_toplevel::{self, ForeignToplevelManagerState};
use crate::protocols::gamma_control::GammaControlManagerState;
use crate::protocols::mutter_x11_interop::MutterX11InteropManagerState;
use crate::protocols::output_management::OutputManagementManagerState;
use crate::protocols::screencopy::{Screencopy, ScreencopyBuffer, ScreencopyManagerState};
use crate::protocols::virtual_pointer::VirtualPointerManagerState;
use crate::render_helpers::blur::BlurOptions;
use crate::render_helpers::debug::push_opaque_regions;
use crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::surface::push_elements_from_surface_tree;
use crate::render_helpers::texture::TextureBuffer;
use crate::render_helpers::xray::{Xray, XrayPos};
use crate::render_helpers::{
    encompassing_geo, render_to_dmabuf, render_to_encompassing_texture, render_to_shm,
    render_to_texture, render_to_vec, shaders, RenderCtx, RenderTarget,
};
#[cfg(feature = "xdp-gnome-screencast")]
use crate::screencasting::Screencasting;
use crate::swayward_render_elements;
use crate::ui::config_error_notification::ConfigErrorNotification;
use crate::ui::exit_confirm_dialog::{ExitConfirmDialog, ExitConfirmDialogRenderElement};
use crate::ui::hotkey_overlay::HotkeyOverlay;
use crate::ui::mru::{MruCloseRequest, WindowMruUi, WindowMruUiRenderElement};
use crate::ui::screen_transition::{self, ScreenTransition};
use crate::ui::screenshot_ui::{OutputScreenshot, ScreenshotUi, ScreenshotUiRenderElement};
use crate::utils::scale::{closest_representable_scale, guess_monitor_scale};
use crate::utils::spawning::{CHILD_DISPLAY, CHILD_ENV};
use crate::utils::vblank_throttle::VBlankThrottle;
use crate::utils::watcher::Watcher;
use crate::utils::xwayland::satellite::Satellite;
use crate::utils::{
    center, center_f64, expand_home, get_monotonic_time, ipc_transform_to_smithay, is_mapped,
    logical_output, make_screenshot_path, output_matches_name, output_size, panel_orientation,
    send_scale_transform, write_png_rgba8, xwayland,
};
use crate::window::mapped::MappedId;
use crate::window::{InitialConfigureState, Mapped, ResolvedWindowRules, Unmapped, WindowRef};

const CLEAR_COLOR_LOCKED: [f32; 4] = [0.3, 0.1, 0.1, 1.];

// We'll try to send frame callbacks at least once a second. We'll make a timer that fires once a
// second, so with the worst timing the maximum interval between two frame callbacks for a surface
// should be ~1.995 seconds.
const FRAME_CALLBACK_THROTTLE: Option<Duration> = Some(Duration::from_millis(995));

pub enum RuntimeWindowRule {
    Assign(
        crate::criteria::Criteria,
        swayward_ipc::command::AssignmentTarget,
    ),
    NoFocus(String, crate::criteria::Criteria),
}

struct LayerRenderRequest<'a> {
    ns: Option<usize>,
    layer_map: &'a LayerMap,
    layer: Layer,
    xray_pos: XrayPos,
    for_backdrop: bool,
}

type ScreencopyRenderResult = anyhow::Result<Option<SyncPoint>>;

pub struct Swayward {
    pub config: Rc<RefCell<Config>>,

    /// Output config from the config file.
    ///
    /// This does not include transient output config changes done via IPC. It is only used when
    /// reloading the config from disk to determine if the output configuration should be reloaded
    /// (and transient changes dropped).
    pub config_file_output_config: swayward_config::Outputs,

    pub config_file_watcher: Option<Watcher>,

    pub event_loop: LoopHandle<'static, State>,
    pub scheduler: Scheduler<()>,
    pub stop_signal: LoopSignal,
    pub shutdown_requested: bool,
    #[cfg(test)]
    lock_deadline: Duration,
    pub display_handle: DisplayHandle,

    /// Whether swayward was run with `--session`
    pub is_session_instance: bool,

    /// Name of the Wayland socket.
    ///
    /// This is `None` when creating `Swayward` without a Wayland socket.
    pub socket_name: Option<OsString>,

    pub start_time: Instant,

    /// Whether the at-startup=true window rules are active.
    pub is_at_startup: bool,

    /// Clock for driving animations.
    pub clock: Clock,

    // Each workspace corresponds to a Space. Each workspace generally has one Output mapped to it,
    // however it may have none (when there are no outputs connected) or multiple (when mirroring).
    pub layout: Layout<Mapped>,

    pub marks: HashMap<String, MappedId>,
    pub marks_by_window: HashMap<MappedId, Vec<String>>,
    pub marks_by_container: HashMap<crate::layout::tiling_tree::NodeId, Vec<String>>,
    pub runtime_window_rules: Vec<RuntimeWindowRule>,
    pub for_window: Vec<(String, String, crate::criteria::Criteria)>,
    /// Runtime `for_window` criteria added since the last successful reload.
    pub runtime_for_window: HashSet<(String, String)>,
    /// Runtime `for_window` criteria already executed for each mapped window.
    ///
    /// Sway stores criterion pointers on the view and clears them on unmap
    /// (`sway/tree/view.c`, `view_execute_criteria` and `view_unmap`). Strings
    /// give the split swayward parser the same stable identity.
    pub executed_for_window: HashSet<(MappedId, String, String)>,
    pub binding_mode: String,
    /// Runtime-only sway switch bindings. KDL `switch-events` remains its
    /// narrower spawn-only model; this overlay carries arbitrary sway commands
    /// and mode/lock identity exactly for `bindswitch`.
    pub runtime_switch_bindings: Vec<RuntimeSwitchBinding>,
    /// Runtime sway variables set by `set $name value`.
    ///
    /// Sway keeps these in `config->symbols` and substitutes them when a
    /// command is dispatched, not only when the config is read
    /// (`sway/sway/commands.c:283-285`), so a variable set over IPC affects
    /// every command sent afterwards. Sorted longest name first, as sway sorts
    /// on insert, so `$mod2` wins over `$mod` (`sway/sway/commands/set.c:13-15`).
    pub sway_variables: Vec<(String, String)>,
    pub seat_name: String,

    // This space does not actually contain any windows, but all outputs are mapped into it
    // according to their global position.
    pub global_space: Space<Window>,

    /// Mapped outputs, sorted by their name and position.
    pub sorted_outputs: Vec<Output>,

    // Windows which don't have a buffer attached yet.
    pub unmapped_windows: HashMap<WlSurface, Unmapped>,

    /// Layer surfaces which don't have a buffer attached yet.
    pub unmapped_layer_surfaces: HashSet<WlSurface>,

    /// Extra data for mapped layer surfaces.
    pub mapped_layer_surfaces: HashMap<LayerSurface, MappedLayer>,

    // Cached root surface for every surface, so that we can access it in destroyed() where the
    // normal get_parent() is cleared out.
    pub root_surface: HashMap<WlSurface, WlSurface>,

    // Dmabuf readiness pre-commit hook for a surface.
    pub dmabuf_pre_commit_hook: HashMap<WlSurface, HookId>,

    /// Clients to notify about their blockers being cleared.
    pub blocker_cleared_tx: Sender<Client>,
    pub blocker_cleared_rx: Receiver<Client>,

    pub output_state: HashMap<Output, OutputState>,
    /// Runtime power state keyed by connector name.
    pub output_power: HashMap<String, bool>,

    // When false, we're idling with monitors powered off.
    pub monitors_active: bool,

    /// Whether the laptop lid is closed.
    ///
    /// Libinput guarantees that the lid switch starts in open state, and if it was closed during
    /// startup, libinput will immediately send a closed event.
    pub is_lid_closed: bool,

    pub devices: HashSet<input::Device>,
    pub ipc_input_devices: HashMap<String, crate::input::IpcInputDevice>,
    pub tablets: HashMap<input::Device, TabletData>,
    pub touch: HashSet<input::Device>,

    // Smithay state.
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_toplevel_tag_manager: XdgToplevelTagManager,
    pub xdg_decoration_state: XdgDecorationState,
    pub kde_decoration_state: KdeDecorationState,
    pub layer_shell_state: WlrLayerShellState,
    pub session_lock_state: SessionLockManagerState,
    pub foreign_toplevel_state: ForeignToplevelManagerState,
    pub ext_workspace_state: ExtWorkspaceManagerState,
    pub screencopy_state: ScreencopyManagerState,
    pub output_management_state: OutputManagementManagerState,
    pub viewporter_state: ViewporterState,
    pub background_effect_state: BackgroundEffectState,
    pub xdg_foreign_state: XdgForeignState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub dmabuf_state: DmabufState,
    pub fractional_scale_manager_state: FractionalScaleManagerState,
    pub seat_state: SeatState<State>,
    pub tablet_state: TabletManagerState,
    pub text_input_state: TextInputManagerState,
    pub input_method_state: InputMethodManagerState,
    pub keyboard_shortcuts_inhibit_state: KeyboardShortcutsInhibitState,
    pub virtual_keyboard_state: VirtualKeyboardManagerState,
    pub virtual_pointer_state: VirtualPointerManagerState,
    pub pointer_gestures_state: PointerGesturesState,
    pub relative_pointer_state: RelativePointerManagerState,
    pub pointer_constraints_state: PointerConstraintsState,
    pub idle_notifier_state: IdleNotifierState<State>,
    pub idle_inhibit_manager_state: IdleInhibitManagerState,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub wlr_data_control_state: WlrDataControlState,
    pub ext_data_control_state: ExtDataControlState,
    pub popups: PopupManager,
    pub popup_grab: Option<PopupGrabState>,
    pub presentation_state: PresentationState,
    pub security_context_state: SecurityContextState,
    pub gamma_control_manager_state: GammaControlManagerState,
    pub activation_state: XdgActivationState,
    pub mutter_x11_interop_state: MutterX11InteropManagerState,

    // This will not work as is outside of tests, so it is gated with #[cfg(test)] for now. In
    // particular, shaders will need to learn about the single pixel buffer. Also, it must be
    // verified that a black single-pixel-buffer background lets the foreground surface to be
    // unredirected.
    //
    // https://github.com/niri-wm/niri/issues/619
    #[cfg(test)]
    pub single_pixel_buffer_state: SinglePixelBufferState,

    pub seat: Seat<State>,
    /// Scancodes of the keys to suppress.
    pub suppressed_keys: HashSet<Keycode>,
    pub held_release_bind: Option<Bind>,
    /// Button codes of the mouse buttons to suppress.
    pub suppressed_buttons: HashSet<u32>,
    pub held_release_buttons: HashMap<(String, u32), Bind>,
    pub bind_cooldown_timers: HashMap<swayward_config::BindIdentity, RegistrationToken>,
    pub bind_repeat_timer: Option<RegistrationToken>,
    pub keyboard_focus: KeyboardFocus,
    pub layer_shell_on_demand_focus: Option<LayerSurface>,
    pub idle_inhibiting_surfaces: HashSet<WlSurface>,
    pub is_fdo_idle_inhibited: Arc<AtomicBool>,
    pub keyboard_shortcuts_inhibiting_surfaces: HashMap<WlSurface, KeyboardShortcutsInhibitor>,

    /// Most recent XKB settings from org.freedesktop.locale1.
    pub xkb_from_locale1: Option<Xkb>,

    pub cursor_manager: CursorManager,
    pub cursor_texture_cache: CursorTextureCache,
    pub cursor_shape_manager_state: CursorShapeManagerState,
    pub dnd_icon: Option<DndIcon>,
    /// Contents under pointer.
    ///
    /// Periodically updated: on motion and other events and in the loop callback. If you require
    /// the real up-to-date contents somewhere, it's better to recompute on the spot.
    ///
    /// This is not pointer focus. I.e. during a click grab, the pointer focus remains on the
    /// client with the grab, but this field will keep updating to the latest contents as if no
    /// grab was active.
    ///
    /// This is primarily useful for emitting pointer motion events for surfaces that move
    /// underneath the cursor on their own (i.e. when the tiling layout moves). In this case, not
    /// taking grabs into account is expected, because we pass the information to pointer.motion()
    /// which passes it down through grabs, which decide what to do with it as they see fit.
    pub pointer_contents: PointContents,
    pub pointer_visibility: PointerVisibility,
    pub pointer_inactivity_timer: Option<RegistrationToken>,
    /// Whether the pointer inactivity timer got reset this event loop iteration.
    ///
    /// Used for limiting the reset to once per iteration, so that it's not spammed with high
    /// resolution mice.
    pub pointer_inactivity_timer_got_reset: bool,
    /// Whether the (idle notifier) activity was notified this event loop iteration.
    ///
    /// Used for limiting the notify to once per iteration, so that it's not spammed with high
    /// resolution mice.
    pub notified_activity_this_iteration: bool,
    pub pointer_inside_hot_corner: bool,
    /// The cursor currently shows a border-resize icon set by the compositor.
    pub border_resize_cursor: bool,
    pub pointer_constraint_position_hint: Option<Point<f64, Logical>>,
    pub tablet_cursor_location: Option<Point<f64, Logical>>,
    pub gesture_swipe_3f_cumulative: Option<(f64, f64)>,
    pub overview_scroll_swipe_gesture: ScrollSwipeGesture,
    pub vertical_wheel_tracker: ScrollTracker,
    pub horizontal_wheel_tracker: ScrollTracker,
    pub mods_with_mouse_binds: HashSet<Modifiers>,
    pub mods_with_wheel_binds: HashSet<Modifiers>,
    pub mods_with_tablet_stylus_binds: HashSet<Modifiers>,
    pub vertical_finger_scroll_tracker: ScrollTracker,
    pub horizontal_finger_scroll_tracker: ScrollTracker,
    pub mods_with_finger_scroll_binds: HashSet<Modifiers>,

    pub lock_state: LockState,

    // State that we last sent to the logind LockedHint.
    pub locked_hint: Option<bool>,

    pub screenshot_ui: ScreenshotUi,
    pub config_error_notification: ConfigErrorNotification,
    pub hotkey_overlay: HotkeyOverlay,
    pub exit_confirm_dialog: ExitConfirmDialog,

    pub window_mru_ui: WindowMruUi,
    pub pending_mru_commit: Option<PendingMruCommit>,
    pub urgency_timers: HashMap<MappedId, RegistrationToken>,

    pub pick_window: Option<async_channel::Sender<Option<MappedId>>>,
    pub pick_color: Option<async_channel::Sender<Option<swayward_ipc::PickedColor>>>,

    pub debug_draw_opaque_regions: bool,
    pub debug_draw_damage: bool,

    #[cfg(feature = "dbus")]
    pub dbus: Option<crate::dbus::DBusServers>,
    #[cfg(feature = "dbus")]
    pub a11y_manager: Option<crate::dbus::freedesktop_a11y::Manager>,
    #[cfg(feature = "dbus")]
    pub a11y: A11y,
    #[cfg(feature = "dbus")]
    pub inhibit_power_key_fd: Option<zbus::zvariant::OwnedFd>,

    pub ipc_server: Option<IpcServer>,
    pub ipc_outputs_changed: bool,

    pub satellite: Option<Satellite>,

    #[cfg(feature = "xdp-gnome-screencast")]
    pub casting: Screencasting,
}

smithay::delegate_dispatch2!(State);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerVisibility {
    /// The pointer is visible.
    Visible,
    /// The pointer is invisible, but retains its focus.
    ///
    /// This state is set temporarily after auto-hiding the pointer to keep tooltips open and grabs
    /// ongoing.
    Hidden,
    /// The pointer is invisible and cannot focus.
    ///
    /// Corresponds to a fully disabled pointer, for example after a touchscreen input, or after
    /// the pointer contents changed in a Hidden state.
    Disabled,
}

impl PointerVisibility {
    pub fn is_visible(&self) -> bool {
        matches!(self, Self::Visible)
    }
}

#[derive(Debug)]
pub struct DndIcon {
    pub surface: WlSurface,
    pub offset: Point<i32, Logical>,
}

pub struct OutputState {
    pub global: GlobalId,
    pub frame_clock: FrameClock,
    pub redraw_state: RedrawState,
    pub on_demand_vrr_enabled: bool,
    // After the last redraw, some ongoing animations still remain.
    pub unfinished_animations_remain: bool,
    /// Last sequence received in a vblank event.
    pub last_drm_sequence: Option<u32>,
    pub vblank_throttle: VBlankThrottle,
    /// Sequence for frame callback throttling.
    ///
    /// We want to send frame callbacks for each surface at most once per monitor refresh cycle.
    ///
    /// Even if a surface commit resulted in empty damage to the monitor, we want to delay the next
    /// frame callback until roughly when a VBlank would occur, had the monitor been damaged. This
    /// is necessary to prevent clients busy-looping with frame callbacks that result in empty
    /// damage.
    ///
    /// This counter wrapping-increments by 1 every time we move into the next refresh cycle, as
    /// far as frame callback throttling is concerned. Specifically, it happens:
    ///
    /// 1. Upon a successful DRM frame submission. Notably, we don't wait for the VBlank here,
    ///    because the client buffers are already "latched" at the point of submission. Even if a
    ///    client submits a new buffer right away, we will wait for a VBlank to draw it, which
    ///    means that busy looping is avoided.
    /// 2. If a frame resulted in empty damage, a timer is queued to fire roughly when a VBlank
    ///    would occur, based on the last presentation time and output refresh interval. Sequence
    ///    is incremented in that timer, before attempting a redraw or sending frame callbacks.
    pub frame_callback_sequence: u32,
    /// Solid color buffer for the backdrop that we use instead of clearing to avoid damage
    /// tracking issues and make screenshots easier.
    pub backdrop_buffer: SolidColorBuffer,
    pub xray: Xray,
    pub lock_render_state: LockRenderState,
    pub lock_surface: Option<LockSurface>,
    pub lock_color_buffer: SolidColorBuffer,
    screen_transition: Option<ScreenTransition>,
    /// Damage tracker used for the debug damage visualization.
    pub debug_damage_tracker: OutputDamageTracker,
}

#[derive(Debug, Default)]
pub enum RedrawState {
    /// The compositor is idle.
    #[default]
    Idle,
    /// A redraw is queued.
    Queued,
    /// We submitted a frame to the KMS and waiting for it to be presented.
    WaitingForVBlank { redraw_needed: bool },
    /// We did not submit anything to KMS and made a timer to fire at the estimated VBlank.
    WaitingForEstimatedVBlank(RegistrationToken),
    /// A redraw is queued on top of the above.
    WaitingForEstimatedVBlankAndQueued(RegistrationToken),
}

pub struct PopupGrabState {
    pub root: WlSurface,
    pub grab: PopupGrab<State>,
    pub has_keyboard_grab: bool,
}

// The surfaces here are always toplevel surfaces focused as far as niri's logic is concerned, even
// when popup grabs are active (which means the real keyboard focus is on a popup descending from
// that toplevel surface).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyboardFocus {
    // Layout is focused by default if there's nothing else to focus.
    Layout { surface: Option<WlSurface> },
    LayerShell { surface: WlSurface },
    LockScreen { surface: Option<WlSurface> },
    ScreenshotUi,
    ExitConfirmDialog,
    Overview,
    Mru,
}

#[derive(Default, Clone, PartialEq)]
pub struct PointContents {
    // Output under point.
    pub output: Option<Output>,
    // Surface under point and its location in the global coordinate space.
    //
    // Can be `None` even when `window` is set, for example when the pointer is over the niri
    // border around the window.
    pub surface: Option<(WlSurface, Point<f64, Logical>)>,
    // If surface belongs to a window, this is that window.
    pub window: Option<(Window, HitType)>,
    // If surface belongs to a layer surface, this is that layer surface.
    pub layer: Option<LayerSurface>,
    // Pointer is over a hot corner.
    pub hot_corner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSwitchBinding {
    pub mode: String,
    pub switch: Switch,
    /// `None` is sway's `toggle` trigger; otherwise match this exact state.
    pub state: Option<SwitchState>,
    pub locked: bool,
    pub command: String,
}

#[derive(Debug, Default)]
pub enum LockState {
    #[default]
    Unlocked,
    WaitingForSurfaces {
        confirmation: SessionLocker,
        deadline_token: RegistrationToken,
    },
    Locking(SessionLocker),
    Locked(ExtSessionLockV1),
}

#[derive(PartialEq, Eq)]
pub enum LockRenderState {
    /// The output displays a normal session frame.
    Unlocked,
    /// The output displays a locked frame.
    Locked,
}

// Not related to the one in Smithay.
//
// This state keeps track of when a surface last received a frame callback.
struct SurfaceFrameThrottlingState {
    /// Output and sequence that the frame callback was last sent at.
    last_sent_at: RefCell<Option<(Output, u32)>>,
}

pub enum CenterCoords {
    Separately,
    Both,
    // Force centering even if the cursor is already in the rectangle.
    BothAlways,
}

#[derive(Clone, PartialEq, Eq)]
pub enum CastTarget {
    // Dynamic cast before selecting anything.
    Nothing,
    Output {
        output: WeakOutput,
        /// Cached name of the output.
        name: String,
    },
    Window {
        id: u64,
    },
}

impl CastTarget {
    pub fn output(output: &Output) -> Self {
        Self::Output {
            output: output.downgrade(),
            name: output.name(),
        }
    }

    pub fn matches_output(&self, weak: &WeakOutput) -> bool {
        matches!(self, CastTarget::Output { output, .. } if output == weak)
    }
}

/// Pending update to a window's focus timestamp.
#[derive(Debug)]
pub struct PendingMruCommit {
    id: MappedId,
    token: RegistrationToken,
    stamp: Duration,
}

impl RedrawState {
    fn queue_redraw(self) -> Self {
        match self {
            RedrawState::Idle => RedrawState::Queued,
            RedrawState::WaitingForEstimatedVBlank(token) => {
                RedrawState::WaitingForEstimatedVBlankAndQueued(token)
            }

            // A redraw is already queued.
            value @ (RedrawState::Queued | RedrawState::WaitingForEstimatedVBlankAndQueued(_)) => {
                value
            }

            // We're waiting for VBlank, request a redraw afterwards.
            RedrawState::WaitingForVBlank { .. } => RedrawState::WaitingForVBlank {
                redraw_needed: true,
            },
        }
    }
}

impl Default for SurfaceFrameThrottlingState {
    fn default() -> Self {
        Self {
            last_sent_at: RefCell::new(None),
        }
    }
}

impl KeyboardFocus {
    pub fn surface(&self) -> Option<&WlSurface> {
        match self {
            KeyboardFocus::Layout { surface } => surface.as_ref(),
            KeyboardFocus::LayerShell { surface } => Some(surface),
            KeyboardFocus::LockScreen { surface } => surface.as_ref(),
            KeyboardFocus::ScreenshotUi => None,
            KeyboardFocus::ExitConfirmDialog => None,
            KeyboardFocus::Overview => None,
            KeyboardFocus::Mru => None,
        }
    }

    pub fn into_surface(self) -> Option<WlSurface> {
        match self {
            KeyboardFocus::Layout { surface } => surface,
            KeyboardFocus::LayerShell { surface } => Some(surface),
            KeyboardFocus::LockScreen { surface } => surface,
            KeyboardFocus::ScreenshotUi => None,
            KeyboardFocus::ExitConfirmDialog => None,
            KeyboardFocus::Overview => None,
            KeyboardFocus::Mru => None,
        }
    }

    pub fn is_layout(&self) -> bool {
        matches!(self, KeyboardFocus::Layout { .. })
    }

    pub fn is_overview(&self) -> bool {
        matches!(self, KeyboardFocus::Overview)
    }
}

#[cfg(test)]
pub(crate) static LIVE_STATE_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

pub enum IpcMode {
    Ambient,
    At(PathBuf),
    Off,
}

pub struct StartupOptions {
    pub headless: bool,
    pub create_wayland_socket: bool,
    pub ipc_mode: IpcMode,
    pub is_session_instance: bool,
}

pub struct State {
    pub backend: Backend,
    pub swayward: Swayward,
}

#[cfg(test)]
impl Drop for State {
    fn drop(&mut self) {
        LIVE_STATE_COUNT.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

impl State {
    pub fn new(
        config: Config,
        event_loop: LoopHandle<'static, State>,
        stop_signal: LoopSignal,
        display: Display<State>,
        options: StartupOptions,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let _span = tracy_client::span!("State::new");

        let config = Rc::new(RefCell::new(config));

        let has_display = env::var_os("WAYLAND_DISPLAY").is_some()
            || env::var_os("WAYLAND_SOCKET").is_some()
            || env::var_os("DISPLAY").is_some();

        let mut backend = if options.headless {
            let headless = Headless::new();
            Backend::Headless(headless)
        } else if has_display {
            let winit = Winit::new(config.clone(), event_loop.clone())?;
            Backend::Winit(winit)
        } else {
            let tty = Tty::new(config.clone(), event_loop.clone())
                .context("error initializing the TTY backend")?;
            Backend::Tty(tty)
        };

        let mut swayward = Swayward::new(
            config.clone(),
            event_loop,
            stop_signal,
            display,
            &backend,
            &options,
        )?;
        backend.init(&mut swayward);

        let mut state = Self { backend, swayward };
        #[cfg(test)]
        LIVE_STATE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Load the xkb_file config option if set by the user.
        state.load_xkb_file();
        // Initialize some IPC server state.
        state.ipc_keyboard_layouts_changed();
        // Focus the default monitor if set by the user.
        state.focus_default_monitor();

        Ok(state)
    }

    pub fn request_stop(&mut self, reason: &str) {
        if self.swayward.shutdown_requested {
            return;
        }
        self.swayward.shutdown_requested = true;
        let stop_signal = self.swayward.stop_signal.clone();
        if let Some(server) = &self.swayward.ipc_server {
            server.send_event(swayward_ipc::legacy::Event::Shutdown {
                reason: reason.into(),
            });
        }
        self.swayward
            .event_loop
            .insert_source(
                Timer::from_duration(Duration::from_millis(10)),
                move |_, _, _| {
                    stop_signal.stop();
                    TimeoutAction::Drop
                },
            )
            .unwrap();
    }

    pub fn refresh_and_flush_clients(&mut self) {
        let _span = tracy_client::span!("State::refresh_and_flush_clients");

        self.refresh();

        // Advance animations to the current time (not target render time) before rendering outputs
        // in order to clear completed animations and render elements. Even if we're not rendering,
        // it's good to advance every now and then so the workspace clean-up and animations don't
        // build up (the 1 second frame callback timer will call this line).
        self.swayward.advance_animations();

        self.swayward.redraw_queued_outputs(&mut self.backend);

        {
            let _span = tracy_client::span!("flush_clients");
            self.swayward.display_handle.flush_clients().unwrap();
        }

        #[cfg(feature = "dbus")]
        self.swayward.update_locked_hint();

        // Clear the time so it's fetched afresh next iteration.
        self.swayward.clock.clear();
        self.swayward.pointer_inactivity_timer_got_reset = false;
        self.swayward.notified_activity_this_iteration = false;
    }

    // We monitor both libinput and logind: libinput is always there (including without DBus), but
    // it misses some switch events (e.g. after unsuspend) on some systems.
    pub fn set_lid_closed(&mut self, is_closed: bool) {
        if self.swayward.is_lid_closed == is_closed {
            return;
        }

        debug!("laptop lid {}", if is_closed { "closed" } else { "opened" });
        self.swayward.is_lid_closed = is_closed;
        self.backend.on_output_config_changed(&mut self.swayward);
    }

    fn refresh(&mut self) {
        let _span = tracy_client::span!("State::refresh");

        // Handle commits for surfaces whose blockers cleared this cycle. This should happen before
        // layout.refresh() since this is where these surfaces handle commits.
        self.notify_blocker_cleared();

        // These should be called periodically, before flushing the clients.
        self.swayward.popups.cleanup();
        self.refresh_popup_grab();
        self.update_keyboard_focus();

        // Should be called before refresh_layout() because that one will refresh other window
        // states and then send a pending configure.
        self.swayward.refresh_window_states();

        // Needs to be called after updating the keyboard focus.
        self.swayward.refresh_layout();

        self.swayward
            .cursor_manager
            .check_cursor_image_surface_alive();
        self.swayward.refresh_pointer_outputs();
        self.swayward.global_space.refresh();
        self.swayward.refresh_idle_inhibit();
        self.refresh_pointer_contents();
        foreign_toplevel::refresh(self);
        ext_workspace::refresh(self);

        #[cfg(feature = "xdp-gnome-screencast")]
        self.swayward.refresh_mapped_cast_outputs();
        // Should happen before refresh_window_rules(), but after anything that can start or stop
        // screencasts.
        #[cfg(feature = "xdp-gnome-screencast")]
        self.swayward.refresh_mapped_cast_window_rules();
        // Clear expired screencopy casts. Ideally we'd have a deadline timer, but our 1 second
        // frame callback timer calls refresh regularly, so that's fine as is.
        self.swayward.screencopy_state.clear_expired_casts();

        self.swayward.refresh_window_rules();
        self.refresh_ipc_outputs();
        self.ipc_refresh_layout();
        self.ipc_refresh_keyboard_layout_index();

        // Needs to be called after updating the keyboard focus.
        #[cfg(feature = "dbus")]
        self.swayward.refresh_a11y();
    }

    fn notify_blocker_cleared(&mut self) {
        let dh = self.swayward.display_handle.clone();
        while let Ok(client) = self.swayward.blocker_cleared_rx.try_recv() {
            trace!("calling blocker_cleared");
            self.client_compositor_state(&client)
                .blocker_cleared(self, &dh);
        }
    }

    pub fn move_cursor(&mut self, location: Point<f64, Logical>) {
        let mut under = match self.swayward.pointer_visibility {
            PointerVisibility::Disabled => PointContents::default(),
            _ => self.swayward.contents_under(location),
        };

        // Disable the hidden pointer if the contents underneath have changed.
        if !self.swayward.pointer_visibility.is_visible() && self.swayward.pointer_contents != under
        {
            self.swayward.pointer_visibility = PointerVisibility::Disabled;

            // When setting PointerVisibility::Hidden together with pointer contents changing,
            // we can change straight to nothing to avoid one frame of hover. Notably, this can
            // be triggered through warp-mouse-to-focus combined with hide-when-typing.
            under = PointContents::default();
        }

        self.swayward.pointer_contents.clone_from(&under);

        let pointer = &self.swayward.seat.get_pointer().unwrap();
        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: InputTime::now(),
            },
        );
        pointer.frame(self);

        self.swayward.maybe_activate_pointer_constraint();

        // We do not show the pointer on programmatic or keyboard movement.

        // FIXME: granular
        self.swayward.queue_redraw_all();
    }

    /// Moves cursor within the specified rectangle, only adjusting coordinates if needed.
    fn move_cursor_to_rect(&mut self, rect: Rectangle<f64, Logical>, mode: CenterCoords) -> bool {
        let pointer = &self.swayward.seat.get_pointer().unwrap();
        let cur_loc = pointer.current_location();
        let x_in_bound = cur_loc.x >= rect.loc.x && cur_loc.x <= rect.loc.x + rect.size.w;
        let y_in_bound = cur_loc.y >= rect.loc.y && cur_loc.y <= rect.loc.y + rect.size.h;

        let p = match mode {
            CenterCoords::Separately => {
                if x_in_bound && y_in_bound {
                    return false;
                } else if y_in_bound {
                    // adjust x
                    Point::from((rect.loc.x + rect.size.w / 2.0, cur_loc.y))
                } else if x_in_bound {
                    // adjust y
                    Point::from((cur_loc.x, rect.loc.y + rect.size.h / 2.0))
                } else {
                    // adjust x and y
                    center_f64(rect)
                }
            }
            CenterCoords::Both => {
                if x_in_bound && y_in_bound {
                    return false;
                } else {
                    // adjust x and y
                    center_f64(rect)
                }
            }
            CenterCoords::BothAlways => center_f64(rect),
        };

        self.move_cursor(p);
        true
    }

    pub fn move_cursor_to_focused_tile(&mut self, mode: CenterCoords) -> bool {
        if !self.swayward.keyboard_focus.is_layout() {
            return false;
        }

        if self.swayward.tablet_cursor_location.is_some() {
            return false;
        }

        let Some(output) = self.swayward.layout.active_output() else {
            return false;
        };
        let monitor = self.swayward.layout.monitor_for_output(output).unwrap();

        let mut rv = false;
        let rect = monitor.active_window_visual_rectangle();

        if let Some(rect) = rect {
            let output_geo = self.swayward.global_space.output_geometry(output).unwrap();
            let mut rect = rect;
            rect.loc += output_geo.loc.to_f64();
            rv = self.move_cursor_to_rect(rect, mode);
        }

        rv
    }

    pub fn focus_default_monitor(&mut self) {
        // Our default target is the first output in sorted order.
        let Some(target) = self.swayward.sorted_outputs.first().cloned() else {
            // No outputs are connected.
            return;
        };

        if !self.focus_configured_monitor() {
            self.swayward.layout.focus_output(&target);
            self.move_cursor_to_output(&target);
        }
    }

    pub fn focus_configured_monitor(&mut self) -> bool {
        let target = {
            let config = self.swayward.config.borrow();
            config.outputs.0.iter().find_map(|config| {
                config
                    .focus_at_startup
                    .then(|| self.swayward.output_by_name_match(&config.name))
                    .flatten()
                    .cloned()
            })
        };
        let Some(target) = target else {
            return false;
        };

        self.swayward.layout.focus_output(&target);
        self.move_cursor_to_output(&target);
        true
    }

    /// Focus a specific window, taking care of a potential active output change and cursor
    /// warp.
    pub fn focus_window(&mut self, window: &Window) {
        let active_output = self.swayward.layout.active_output().cloned();

        self.swayward.layout.activate_window(window);

        let new_active = self.swayward.layout.active_output().cloned();
        if new_active != active_output {
            if !self.maybe_warp_cursor_to_focus_centered() {
                self.move_cursor_to_output(&new_active.unwrap());
            }
        } else {
            self.maybe_warp_cursor_to_focus();
        }

        // FIXME: granular
        self.swayward.queue_redraw_all();
    }

    pub fn confirm_mru(&mut self) {
        if let Some(window) = self.swayward.close_mru(MruCloseRequest::Confirm) {
            // focus_window() will warp the cursor to the window only when the keyboard focus is on
            // the layout. However, right now the keyboard focus is still on the MRU (that we had
            // just closed) since it's only updated at the end of the event loop cycle. Force-update
            // the keyboard focus here to make cursor warping work.
            self.update_keyboard_focus();

            self.focus_window(&window);
        }
    }

    /// Resolve sway's `mouse_warping` policy for the focus change about to
    /// happen.
    ///
    /// `Some(mode)` means the policy governs this warp and the inherited
    /// `warp-mouse-to-focus` centering option is not consulted; `None` means
    /// no sway policy is in force.
    ///
    /// `WARP_OUTPUT` warps only when the newly focused target is on an output
    /// that does not already contain the pointer, and `WARP_CONTAINER` warps
    /// on every qualifying focus change. Both skip the warp when the pointer
    /// already sits inside the target, because sway calls
    /// `cursor_warp_to_container` with `force` unset
    /// (`sway/sway/input/seat.c:1526-1547`, `sway/sway/input/cursor.c:1166-1184`).
    fn sway_warp_mode(&self) -> Option<CenterCoords> {
        match self.swayward.config.borrow().input.mouse_warping {
            swayward_config::input::MouseWarping::No => None,
            swayward_config::input::MouseWarping::Container => Some(CenterCoords::Both),
            swayward_config::input::MouseWarping::Output => {
                let output = self.swayward.layout.active_output()?;
                let geometry = self.swayward.global_space.output_geometry(output)?;
                let pointer = self.swayward.seat.get_pointer()?.current_location();
                if geometry.to_f64().contains(pointer) {
                    // The pointer is already on the focused output, so this
                    // mode leaves it alone.
                    None
                } else {
                    Some(CenterCoords::Both)
                }
            }
        }
    }

    pub fn maybe_warp_cursor_to_focus(&mut self) -> bool {
        if let Some(mode) = self.sway_warp_mode() {
            return self.move_cursor_to_focused_tile(mode);
        }
        let focused = match self.swayward.config.borrow().input.warp_mouse_to_focus {
            None => return false,
            Some(inner) => match inner.mode {
                None => CenterCoords::Separately,
                Some(WarpMouseToFocusMode::CenterXy) => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXyAlways) => CenterCoords::BothAlways,
            },
        };
        self.move_cursor_to_focused_tile(focused)
    }

    pub fn maybe_warp_cursor_to_focus_centered(&mut self) -> bool {
        if let Some(mode) = self.sway_warp_mode() {
            return self.move_cursor_to_focused_tile(mode);
        }
        let focused = match self.swayward.config.borrow().input.warp_mouse_to_focus {
            None => return false,
            Some(inner) => match inner.mode {
                None => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXy) => CenterCoords::Both,
                Some(WarpMouseToFocusMode::CenterXyAlways) => CenterCoords::BothAlways,
            },
        };
        self.move_cursor_to_focused_tile(focused)
    }

    pub fn refresh_pointer_contents(&mut self) {
        // Don't move the mouse pointer while the user is interacting with the tablet, as it causes
        // unwanted jumps for the client.
        if self.swayward.tablet_cursor_location.is_some() {
            return;
        }

        let _span = tracy_client::span!("Swayward::refresh_pointer_contents");

        let pointer = &self.swayward.seat.get_pointer().unwrap();
        let location = pointer.current_location();

        if !self.swayward.exit_confirm_dialog.is_open()
            && !self.swayward.is_locked()
            && !self.swayward.screenshot_ui.is_open()
        {
            // Don't refresh cursor focus during transitions.
            if let Some((output, _)) = self.swayward.output_under(location) {
                let monitor = self.swayward.layout.monitor_for_output(output).unwrap();
                if monitor.are_transitions_ongoing() {
                    return;
                }
            }
        }

        if !self.update_pointer_contents() {
            return;
        }

        pointer.frame(self);

        // Pointer motion from a surface to nothing triggers a cursor change to default, which
        // means we may need to redraw.

        // FIXME: granular
        self.swayward.queue_redraw_all();
    }

    pub fn update_pointer_contents(&mut self) -> bool {
        let _span = tracy_client::span!("Swayward::update_pointer_contents");

        let pointer = &self.swayward.seat.get_pointer().unwrap();
        let location = pointer.current_location();
        let mut under = match self.swayward.pointer_visibility {
            PointerVisibility::Disabled => PointContents::default(),
            _ => self.swayward.contents_under(location),
        };

        // We're not changing the global cursor location here, so if the contents did not change,
        // then nothing changed.
        if self.swayward.pointer_contents == under {
            return false;
        }

        // Disable the hidden pointer if the contents underneath have changed.
        if !self.swayward.pointer_visibility.is_visible() {
            self.swayward.pointer_visibility = PointerVisibility::Disabled;

            // When setting PointerVisibility::Hidden together with pointer contents changing,
            // we can change straight to nothing to avoid one frame of hover. Notably, this can
            // be triggered through warp-mouse-to-focus combined with hide-when-typing.
            under = PointContents::default();
            if self.swayward.pointer_contents == under {
                return false;
            }
        }

        self.swayward.pointer_contents.clone_from(&under);

        pointer.motion(
            self,
            under.surface,
            &MotionEvent {
                location,
                serial: SERIAL_COUNTER.next_serial(),
                time: InputTime::now(),
            },
        );

        self.swayward.maybe_activate_pointer_constraint();

        true
    }

    pub fn move_cursor_to_output(&mut self, output: &Output) {
        let geo = self.swayward.global_space.output_geometry(output).unwrap();
        self.move_cursor(center(geo).to_f64());
    }

    pub fn refresh_popup_grab(&mut self) {
        if let Some(grab) = &mut self.swayward.popup_grab {
            if grab.grab.has_ended() {
                self.swayward.popup_grab = None;
            }
        }
    }

    /// The seat keyboard's modifier state, or no modifiers when startup could
    /// not add a keyboard because no keymap compiled.
    pub fn modifier_state(&self) -> smithay::input::keyboard::ModifiersState {
        self.swayward
            .seat
            .get_keyboard()
            .map(|keyboard| keyboard.modifier_state())
            .unwrap_or_default()
    }

    pub fn update_keyboard_focus(&mut self) {
        let Some(keyboard) = self.swayward.seat.get_keyboard() else {
            return;
        };

        // Clean up on-demand layer surface focus if necessary.
        if let Some(surface) = &self.swayward.layer_shell_on_demand_focus {
            // Still alive and has on-demand interactivity.
            let mut good = surface.alive()
                && surface.cached_state().keyboard_interactivity
                    == wlr_layer::KeyboardInteractivity::OnDemand;

            if let Some(mapped) = self.swayward.mapped_layer_surfaces.get(surface) {
                // Check if it moved to the overview backdrop.
                if mapped.place_within_backdrop() {
                    good = false;
                }
            } else {
                // The layer surface is alive but it got unmapped.
                good = false;
            }

            if !good {
                self.swayward.layer_shell_on_demand_focus = None;
            }
        }

        // Compute the current focus.
        let focus = if self.swayward.exit_confirm_dialog.is_open() {
            KeyboardFocus::ExitConfirmDialog
        } else if self.swayward.is_locked() {
            KeyboardFocus::LockScreen {
                surface: self.swayward.lock_surface_focus(),
            }
        } else if self.swayward.screenshot_ui.is_open() {
            KeyboardFocus::ScreenshotUi
        } else if self.swayward.window_mru_ui.is_open() {
            KeyboardFocus::Mru
        } else if let Some(output) = self.swayward.layout.active_output() {
            let mon = self.swayward.layout.monitor_for_output(output).unwrap();
            let layers = layer_map_for_output(output);

            // Explicitly check for layer-shell popup grabs here, our keyboard focus will stay on
            // the root layer surface while it has grabs.
            let layer_grab = self.swayward.popup_grab.as_ref().and_then(|g| {
                layers
                    .layer_for_surface(&g.root, WindowSurfaceType::TOPLEVEL)
                    .and_then(|l| l.can_receive_keyboard_focus().then(|| (&g.root, l.layer())))
            });
            let grab_on_layer = |layer: Layer| {
                layer_grab
                    .and_then(move |(s, l)| if l == layer { Some(s.clone()) } else { None })
                    .map(|surface| KeyboardFocus::LayerShell { surface })
            };

            let layout_focus = || {
                self.swayward
                    .layout
                    .focus()
                    .map(|win| win.toplevel().wl_surface().clone())
                    .map(|surface| KeyboardFocus::Layout {
                        surface: Some(surface),
                    })
            };

            let excl_focus_on_layer = |layer| {
                layers.layers_on(layer).find_map(|surface| {
                    if surface.cached_state().keyboard_interactivity
                        != wlr_layer::KeyboardInteractivity::Exclusive
                    {
                        return None;
                    }

                    let mapped = self.swayward.mapped_layer_surfaces.get(surface)?;
                    if mapped.place_within_backdrop() {
                        return None;
                    }

                    let surface = surface.wl_surface().clone();
                    Some(KeyboardFocus::LayerShell { surface })
                })
            };

            let on_d_focus_on_layer = |layer| {
                layers.layers_on(layer).find_map(|surface| {
                    let is_on_demand_surface =
                        Some(surface) == self.swayward.layer_shell_on_demand_focus.as_ref();
                    is_on_demand_surface
                        .then(|| surface.wl_surface().clone())
                        .map(|surface| KeyboardFocus::LayerShell { surface })
                })
            };

            // Prefer exclusive focus on a layer, then check on-demand focus.
            let focus_on_layer =
                |layer| excl_focus_on_layer(layer).or_else(|| on_d_focus_on_layer(layer));

            let is_overview_open = self.swayward.layout.is_overview_open();

            let mut surface = grab_on_layer(Layer::Overlay);
            // FIXME: we shouldn't prioritize the top layer grabs over regular overlay input or a
            // fullscreen layout window. This will need tracking in grab() to avoid handing it out
            // in the first place. Or a better way to structure this code.
            surface = surface.or_else(|| grab_on_layer(Layer::Top));

            if !is_overview_open {
                surface = surface.or_else(|| grab_on_layer(Layer::Bottom));
                surface = surface.or_else(|| grab_on_layer(Layer::Background));
            }

            surface = surface.or_else(|| focus_on_layer(Layer::Overlay));

            if mon.render_above_top_layer() {
                surface = surface.or_else(layout_focus);
                surface = surface.or_else(|| focus_on_layer(Layer::Top));
                surface = surface.or_else(|| focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| focus_on_layer(Layer::Background));
            } else {
                surface = surface.or_else(|| focus_on_layer(Layer::Top));

                if is_overview_open {
                    surface = Some(surface.unwrap_or(KeyboardFocus::Overview));
                }

                surface = surface.or_else(|| on_d_focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| on_d_focus_on_layer(Layer::Background));
                surface = surface.or_else(layout_focus);

                // Bottom and background layers can only receive exclusive focus when there are no
                // layout windows.
                surface = surface.or_else(|| excl_focus_on_layer(Layer::Bottom));
                surface = surface.or_else(|| excl_focus_on_layer(Layer::Background));
            }

            surface.unwrap_or(KeyboardFocus::Layout { surface: None })
        } else {
            KeyboardFocus::Layout { surface: None }
        };

        if self.swayward.keyboard_focus != focus {
            trace!(
                "keyboard focus changed from {:?} to {:?}",
                self.swayward.keyboard_focus,
                focus
            );

            let workspace_for_surface = |surface: &WlSurface| {
                let mut workspace = None;
                self.swayward
                    .layout
                    .with_windows(|mapped, _, workspace_id, _| {
                        if mapped.is_wl_surface(surface) {
                            workspace = workspace_id;
                        }
                    });
                workspace
            };
            let last_workspace = self
                .swayward
                .keyboard_focus
                .surface()
                .and_then(workspace_for_surface);
            let new_workspace = focus.surface().and_then(workspace_for_surface);

            // Tell the windows their new focus state for window rule purposes.
            if let KeyboardFocus::Layout {
                surface: Some(surface),
            } = &self.swayward.keyboard_focus
            {
                if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface)
                {
                    mapped.set_is_focused(false);
                }
            }
            if let KeyboardFocus::Layout {
                surface: Some(surface),
            } = &focus
            {
                self.swayward.focus_clears_urgency(
                    surface,
                    last_workspace.is_some() && last_workspace != new_workspace,
                );
                if let Some((mapped, _)) = self.swayward.layout.find_window_and_output_mut(surface)
                {
                    mapped.set_is_focused(true);

                    // Structural focus fallback follows sway's seat-wide stack immediately. The
                    // recent-windows UI still uses the debounce below before committing its order.
                    let stamp = get_monotonic_time();
                    mapped.set_focus_timestamp(stamp);

                    let debounce = self.swayward.config.borrow().recent_windows.debounce_ms;
                    let debounce = Duration::from_millis(u64::from(debounce));

                    if !debounce.is_zero() {
                        let timer = Timer::from_duration(debounce);

                        let focus_token = self
                            .swayward
                            .event_loop
                            .insert_source(timer, move |_, _, state| {
                                state.swayward.mru_apply_keyboard_commit();
                                TimeoutAction::Drop
                            })
                            .unwrap();
                        if let Some(PendingMruCommit { token, .. }) =
                            self.swayward.pending_mru_commit.replace(PendingMruCommit {
                                id: mapped.id(),
                                token: focus_token,
                                stamp,
                            })
                        {
                            self.swayward.event_loop.remove(token);
                        }
                    }
                }
            }

            if let Some(grab) = self.swayward.popup_grab.as_mut() {
                if grab.has_keyboard_grab && Some(&grab.root) != focus.surface() {
                    trace!(
                        "grab root {:?} is not the new focus {:?}, ungrabbing",
                        grab.root,
                        focus
                    );

                    grab.grab.ungrab(PopupUngrabStrategy::All);
                    keyboard.unset_grab(self);
                    self.swayward.seat.get_pointer().unwrap().unset_grab(
                        self,
                        SERIAL_COUNTER.next_serial(),
                        InputTime::now(),
                    );
                    self.swayward.popup_grab = None;
                }
            }

            if self.swayward.config.borrow().input.keyboard.track_layout == TrackLayout::Window {
                let current_layout = keyboard.with_xkb_state(self, |context| {
                    let xkb = context.xkb().lock().unwrap();
                    xkb.active_layout()
                });

                let mut new_layout = current_layout;
                // Store the currently active layout for the surface.
                if let Some(current_focus) = self.swayward.keyboard_focus.surface() {
                    with_states(current_focus, |data| {
                        let cell = data
                            .data_map
                            .get_or_insert::<Cell<KeyboardLayout>, _>(Cell::default);
                        cell.set(current_layout);
                    });
                }

                if let Some(focus) = focus.surface() {
                    new_layout = with_states(focus, |data| {
                        let cell = data.data_map.get_or_insert::<Cell<KeyboardLayout>, _>(|| {
                            // The default layout is effectively the first layout in the
                            // keymap, so use it for new windows.
                            Cell::new(KeyboardLayout::default())
                        });
                        cell.get()
                    });
                }
                if new_layout != current_layout && focus.surface().is_some() {
                    keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
                    keyboard.with_xkb_state(self, |mut context| {
                        context.set_layout(new_layout);
                    });
                }
            }

            self.swayward.keyboard_focus.clone_from(&focus);
            keyboard.set_focus(self, focus.into_surface(), SERIAL_COUNTER.next_serial());

            // FIXME: can be more granular.
            self.swayward.queue_redraw_all();
        }
    }
}

impl Swayward {
    pub fn new(
        config: Rc<RefCell<Config>>,
        event_loop: LoopHandle<'static, State>,
        stop_signal: LoopSignal,
        display: Display<State>,
        backend: &Backend,
        options: &StartupOptions,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let _span = tracy_client::span!("Swayward::new");

        let (executor, scheduler) =
            calloop::futures::executor().context("error creating the async executor")?;
        event_loop
            .insert_source(executor, |_, _, _| ())
            .map_err(|error| anyhow::anyhow!(error.error))
            .context("error registering the async executor")?;

        let display_handle = display.handle();
        let config_ = config.borrow();
        let config_file_output_config = config_.outputs.clone();

        let mut animation_clock = Clock::default();

        let rate = 1.0 / config_.animations.slowdown.max(0.001);
        animation_clock.set_rate(rate);
        animation_clock.set_complete_instantly(config_.animations.off);

        let layout = Layout::new(animation_clock.clone(), &config_);

        let (blocker_cleared_tx, blocker_cleared_rx) = mpsc::channel();

        fn client_is_unrestricted(client: &Client) -> bool {
            !client.get_data::<ClientState>().unwrap().restricted
        }

        let compositor_state = CompositorState::new_v6::<State>(&display_handle);
        let xdg_shell_state = XdgShellState::new_with_capabilities::<State>(
            &display_handle,
            [WmCapabilities::Fullscreen, WmCapabilities::Maximize],
        );
        let xdg_toplevel_tag_manager = XdgToplevelTagManager::new::<State>(&display_handle);
        let xdg_decoration_state = XdgDecorationState::new::<State>(&display_handle);
        let kde_decoration_state = KdeDecorationState::new_with_filter::<State, _>(
            &display_handle,
            // If we want CSD we will hide the global.
            KdeDecorationsMode::Server,
            |client| {
                client
                    .get_data::<ClientState>()
                    .unwrap()
                    .can_view_decoration_globals
            },
        );
        let layer_shell_state = WlrLayerShellState::new_with_filter::<State, _>(
            &display_handle,
            client_is_unrestricted,
        );
        let session_lock_state =
            SessionLockManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let shm_state = ShmState::new::<State>(
            &display_handle,
            vec![wl_shm::Format::Xbgr8888, wl_shm::Format::Abgr8888],
        );
        let output_manager_state =
            OutputManagerState::new_with_xdg_output::<State>(&display_handle);
        let dmabuf_state = DmabufState::new();
        let fractional_scale_manager_state =
            FractionalScaleManagerState::new::<State>(&display_handle);
        let mut seat_state = SeatState::new();
        let tablet_state = TabletManagerState::new::<State>(&display_handle);
        let pointer_gestures_state = PointerGesturesState::new::<State>(&display_handle);
        let relative_pointer_state = RelativePointerManagerState::new::<State>(&display_handle);
        let pointer_constraints_state = PointerConstraintsState::new::<State>(&display_handle);
        let idle_notifier_state = IdleNotifierState::new(&display_handle, event_loop.clone());
        let idle_inhibit_manager_state = IdleInhibitManagerState::new::<State>(&display_handle);
        let data_device_state = DataDeviceState::new::<State>(&display_handle);
        let primary_selection_state =
            PrimarySelectionState::new_with_filter::<State, _>(&display_handle, |client| {
                !client
                    .get_data::<ClientState>()
                    .unwrap()
                    .primary_selection_disabled
            });
        let wlr_data_control_state = WlrDataControlState::new::<State, _>(
            &display_handle,
            Some(&primary_selection_state),
            client_is_unrestricted,
        );
        let ext_data_control_state = ExtDataControlState::new::<State, _>(
            &display_handle,
            Some(&primary_selection_state),
            client_is_unrestricted,
        );
        let presentation_state =
            PresentationState::new::<State>(&display_handle, Monotonic::ID as u32);
        let security_context_state =
            SecurityContextState::new::<State, _>(&display_handle, client_is_unrestricted);

        let text_input_state = TextInputManagerState::new::<State>(&display_handle);
        let input_method_state =
            InputMethodManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let keyboard_shortcuts_inhibit_state =
            KeyboardShortcutsInhibitState::new::<State>(&display_handle);
        let virtual_keyboard_state =
            VirtualKeyboardManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let virtual_pointer_state =
            VirtualPointerManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let foreign_toplevel_state =
            ForeignToplevelManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let ext_workspace_state =
            ExtWorkspaceManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let mut output_management_state =
            OutputManagementManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        output_management_state.on_config_changed(config_.outputs.clone());
        let screencopy_state =
            ScreencopyManagerState::new::<State, _>(&display_handle, client_is_unrestricted);
        let viewporter_state = ViewporterState::new::<State>(&display_handle);
        let background_effect_state = BackgroundEffectState::new::<State>(&display_handle);
        let xdg_foreign_state = XdgForeignState::new::<State>(&display_handle);

        let supports_gamma = backend.supports_gamma();
        let gamma_control_manager_state =
            GammaControlManagerState::new::<State, _>(&display_handle, move |client| {
                supports_gamma && !client.get_data::<ClientState>().unwrap().restricted
            });
        let activation_state = XdgActivationState::new::<State>(&display_handle);
        event_loop
            .insert_source(
                Timer::from_duration(XDG_ACTIVATION_TOKEN_TIMEOUT),
                |_, _, state| {
                    state
                        .swayward
                        .activation_state
                        .retain_tokens(|_, token_data| {
                            token_data.timestamp.elapsed() < XDG_ACTIVATION_TOKEN_TIMEOUT
                        });
                    TimeoutAction::ToDuration(XDG_ACTIVATION_TOKEN_TIMEOUT)
                },
            )
            .map_err(|error| anyhow::anyhow!(error.error))
            .context("error registering the activation token timer")?;

        let mutter_x11_interop_state =
            MutterX11InteropManagerState::new::<State, _>(&display_handle, move |_| true);

        #[cfg(test)]
        let single_pixel_buffer_state = SinglePixelBufferState::new::<State>(&display_handle);

        let seat_name = backend.seat_name();
        let mut seat: Seat<State> = seat_state.new_wl_seat(&display_handle, seat_name.clone());
        let keyboard = match seat.add_keyboard(
            config_.input.keyboard.xkb.to_xkb_config(),
            config_.input.keyboard.repeat_delay.into(),
            config_.input.keyboard.repeat_rate.into(),
        ) {
            Err(err) => {
                if let smithay::input::keyboard::Error::BadKeymap = err {
                    warn!("error loading the configured xkb keymap, trying default");
                } else {
                    warn!("error adding keyboard: {err:?}");
                }
                match seat.add_keyboard(
                    Default::default(),
                    config_.input.keyboard.repeat_delay.into(),
                    config_.input.keyboard.repeat_rate.into(),
                ) {
                    Ok(keyboard) => Some(keyboard),
                    Err(err) => {
                        error!("error adding keyboard with the default keymap: {err:?}");
                        None
                    }
                }
            }
            Ok(keyboard) => Some(keyboard),
        };
        if config_.input.keyboard.numlock {
            if let Some(keyboard) = keyboard {
                let mut modifier_state = keyboard.modifier_state();
                modifier_state.num_lock = true;
                keyboard.set_modifier_state(modifier_state);
            }
        }
        seat.add_pointer();

        let cursor_shape_manager_state = CursorShapeManagerState::new::<State>(&display_handle);
        let cursor_manager =
            CursorManager::new(&config_.cursor.xcursor_theme, config_.cursor.xcursor_size);

        let mod_key = backend.mod_key(&config.borrow());
        let mods_with_mouse_binds = mods_with_mouse_binds(mod_key, &config_.binds);
        let mods_with_wheel_binds = mods_with_wheel_binds(mod_key, &config_.binds);
        let mods_with_finger_scroll_binds = mods_with_finger_scroll_binds(mod_key, &config_.binds);
        let mods_with_tablet_stylus_binds = mods_with_tablet_stylus_binds(mod_key, &config_.binds);

        let screenshot_ui = ScreenshotUi::new(animation_clock.clone(), config.clone());
        let window_mru_ui = WindowMruUi::new(config.clone());
        let config_error_notification =
            ConfigErrorNotification::new(animation_clock.clone(), config.clone());

        let mut hotkey_overlay = HotkeyOverlay::new(config.clone(), mod_key);
        if !config_.hotkey_overlay.skip_at_startup {
            hotkey_overlay.show();
        }

        let exit_confirm_dialog = ExitConfirmDialog::new(animation_clock.clone(), config.clone());

        #[cfg(feature = "dbus")]
        let a11y = A11y::new(event_loop.clone());

        event_loop
            .insert_source(
                Timer::from_duration(Duration::from_secs(1)),
                |_, _, state| {
                    state.swayward.send_frame_callbacks_on_fallback_timer();
                    TimeoutAction::ToDuration(Duration::from_secs(1))
                },
            )
            .unwrap();

        let socket_name = if options.create_wayland_socket {
            let socket_source =
                ListeningSocketSource::new_auto().context("unable to open Wayland socket")?;
            let socket_name = socket_source.socket_name().to_os_string();
            event_loop
                .insert_source(socket_source, move |client, _, state| {
                    state.swayward.insert_client(NewClient {
                        client,
                        restricted: false,
                        credentials_unknown: false,
                        security_context: None,
                    });
                })
                .context("unable to register Wayland socket")?;
            Some(socket_name)
        } else {
            None
        };

        let ipc_server = match &options.ipc_mode {
            IpcMode::Ambient if socket_name.is_some() => {
                Some(IpcServer::start(&event_loop, socket_name.as_deref())?)
            }
            IpcMode::At(path) => Some(IpcServer::start_at(&event_loop, Some(path.clone()))?),
            IpcMode::Ambient | IpcMode::Off => None,
        };

        #[cfg(feature = "xdp-gnome-screencast")]
        let screencasting = Screencasting::new(&event_loop);

        let display_source = Generic::new(display, Interest::READ, Mode::Level);
        event_loop
            .insert_source(display_source, |_, display, state| {
                // SAFETY: we don't drop the display.
                unsafe {
                    display.get_mut().dispatch_clients(state).unwrap();
                }
                Ok(PostAction::Continue)
            })
            .map_err(|error| anyhow::anyhow!(error.error))
            .context("error registering the Wayland display")?;

        event_loop
            .insert_source(
                Timer::from_duration(Duration::from_secs(60)),
                |_, _, state| {
                    let _span = tracy_client::span!("startup timeout");
                    state.swayward.is_at_startup = false;
                    state.swayward.recompute_window_rules();
                    state.swayward.recompute_layer_rules();
                    TimeoutAction::Drop
                },
            )
            .map_err(|error| anyhow::anyhow!(error.error))
            .context("error registering the startup timer")?;

        drop(config_);
        let mut swayward = Self {
            config,
            config_file_output_config,
            config_file_watcher: None,

            event_loop,
            scheduler,
            stop_signal,
            shutdown_requested: false,
            #[cfg(test)]
            lock_deadline: Duration::from_millis(1000),
            socket_name,
            display_handle,
            is_session_instance: options.is_session_instance,
            start_time: Instant::now(),
            is_at_startup: true,
            clock: animation_clock,

            layout,
            marks: HashMap::new(),
            marks_by_window: HashMap::new(),
            marks_by_container: HashMap::new(),
            runtime_window_rules: Vec::new(),
            for_window: Vec::new(),
            runtime_for_window: HashSet::new(),
            executed_for_window: HashSet::new(),
            binding_mode: "default".into(),
            runtime_switch_bindings: Vec::new(),
            sway_variables: Vec::new(),
            seat_name,
            global_space: Space::default(),
            sorted_outputs: Vec::default(),
            output_state: HashMap::new(),
            output_power: HashMap::new(),
            unmapped_windows: HashMap::new(),
            unmapped_layer_surfaces: HashSet::new(),
            mapped_layer_surfaces: HashMap::new(),
            root_surface: HashMap::new(),
            dmabuf_pre_commit_hook: HashMap::new(),
            blocker_cleared_tx,
            blocker_cleared_rx,
            monitors_active: true,
            is_lid_closed: false,

            devices: HashSet::new(),
            ipc_input_devices: HashMap::new(),
            tablets: HashMap::new(),
            touch: HashSet::new(),

            compositor_state,
            xdg_shell_state,
            xdg_toplevel_tag_manager,
            xdg_decoration_state,
            kde_decoration_state,
            layer_shell_state,
            session_lock_state,
            foreign_toplevel_state,
            ext_workspace_state,
            output_management_state,
            screencopy_state,
            viewporter_state,
            background_effect_state,
            xdg_foreign_state,
            text_input_state,
            input_method_state,
            keyboard_shortcuts_inhibit_state,
            virtual_keyboard_state,
            virtual_pointer_state,
            shm_state,
            output_manager_state,
            dmabuf_state,
            fractional_scale_manager_state,
            seat_state,
            tablet_state,
            pointer_gestures_state,
            relative_pointer_state,
            pointer_constraints_state,
            idle_notifier_state,
            idle_inhibit_manager_state,
            data_device_state,
            primary_selection_state,
            wlr_data_control_state,
            ext_data_control_state,
            popups: PopupManager::default(),
            popup_grab: None,
            suppressed_keys: HashSet::new(),
            held_release_bind: None,
            suppressed_buttons: HashSet::new(),
            held_release_buttons: HashMap::new(),
            bind_cooldown_timers: HashMap::new(),
            bind_repeat_timer: Option::default(),
            presentation_state,
            security_context_state,
            gamma_control_manager_state,
            activation_state,
            mutter_x11_interop_state,
            #[cfg(test)]
            single_pixel_buffer_state,

            seat,
            keyboard_focus: KeyboardFocus::Layout { surface: None },
            layer_shell_on_demand_focus: None,
            idle_inhibiting_surfaces: HashSet::new(),
            is_fdo_idle_inhibited: Arc::new(AtomicBool::new(false)),
            keyboard_shortcuts_inhibiting_surfaces: HashMap::new(),
            xkb_from_locale1: None,
            cursor_manager,
            cursor_texture_cache: Default::default(),
            cursor_shape_manager_state,
            dnd_icon: None,
            pointer_contents: PointContents::default(),
            pointer_visibility: PointerVisibility::Visible,
            pointer_inactivity_timer: None,
            pointer_inactivity_timer_got_reset: false,
            notified_activity_this_iteration: false,
            pointer_inside_hot_corner: false,
            border_resize_cursor: false,
            pointer_constraint_position_hint: None,
            tablet_cursor_location: None,
            gesture_swipe_3f_cumulative: None,
            overview_scroll_swipe_gesture: ScrollSwipeGesture::new(),
            vertical_wheel_tracker: ScrollTracker::new(120),
            horizontal_wheel_tracker: ScrollTracker::new(120),
            mods_with_mouse_binds,
            mods_with_wheel_binds,
            mods_with_tablet_stylus_binds,

            // 10 is copied from Clutter: DISCRETE_SCROLL_STEP.
            vertical_finger_scroll_tracker: ScrollTracker::new(10),
            horizontal_finger_scroll_tracker: ScrollTracker::new(10),
            mods_with_finger_scroll_binds,

            lock_state: LockState::Unlocked,
            locked_hint: None,

            screenshot_ui,
            config_error_notification,
            hotkey_overlay,
            exit_confirm_dialog,

            window_mru_ui,
            pending_mru_commit: None,
            urgency_timers: HashMap::new(),

            pick_window: None,
            pick_color: None,

            debug_draw_opaque_regions: false,
            debug_draw_damage: false,

            #[cfg(feature = "dbus")]
            dbus: None,
            #[cfg(feature = "dbus")]
            a11y_manager: None,
            #[cfg(feature = "dbus")]
            a11y,
            #[cfg(feature = "dbus")]
            inhibit_power_key_fd: None,

            ipc_server,
            ipc_outputs_changed: false,

            satellite: None,

            #[cfg(feature = "xdp-gnome-screencast")]
            casting: screencasting,
        };

        swayward.reset_pointer_inactivity_timer();

        Ok(swayward)
    }

    pub fn set_mark(&mut self, window: MappedId, mark: &str, add: bool, toggle: bool) {
        let had_mark = self.marks.get(mark) == Some(&window);
        if !add {
            if let Some(existing) = self.marks_by_window.remove(&window) {
                for mark in existing {
                    self.marks.remove(&mark);
                }
            }
        }
        if let Some(previous) = self.marks.remove(mark) {
            if let Some(marks) = self.marks_by_window.get_mut(&previous) {
                marks.retain(|existing| existing != mark);
            }
        }
        if !toggle || !had_mark {
            self.marks.insert(mark.to_owned(), window);
            self.marks_by_window
                .entry(window)
                .or_default()
                .push(mark.to_owned());
        }
    }

    /// Move container marks to the node ids a tree transfer assigned.
    ///
    /// Node ids are unique across every tree, so marks are keyed by node
    /// alone and follow a container wherever it lives, including a hidden
    /// scratchpad group. All entries are lifted before any is re-inserted, so
    /// a swap whose two halves exchange ids cannot clobber either half.
    pub fn remap_container_marks(
        &mut self,
        remapped: impl IntoIterator<
            Item = (
                crate::layout::tiling_tree::NodeId,
                crate::layout::tiling_tree::NodeId,
            ),
        >,
    ) {
        let moved = remapped
            .into_iter()
            .filter_map(|(old, new)| Some((new, self.marks_by_container.remove(&old)?)))
            .collect::<Vec<_>>();
        for (new, marks) in moved {
            self.marks_by_container
                .entry(new)
                .or_default()
                .extend(marks);
        }
    }

    pub fn unmark(&mut self, window: Option<MappedId>, mark: Option<&str>) {
        match (window, mark) {
            (Some(window), Some(mark)) if self.marks.get(mark) == Some(&window) => {
                self.marks.remove(mark);
                if let Some(marks) = self.marks_by_window.get_mut(&window) {
                    marks.retain(|existing| existing != mark);
                }
            }
            (Some(window), None) => {
                for mark in self.marks_by_window.remove(&window).unwrap_or_default() {
                    self.marks.remove(&mark);
                }
            }
            (None, Some(mark)) => {
                if let Some(window) = self.marks.remove(mark) {
                    if let Some(marks) = self.marks_by_window.get_mut(&window) {
                        marks.retain(|existing| existing != mark);
                    }
                }
            }
            (None, None) => {
                self.marks.clear();
                self.marks_by_window.clear();
            }
            _ => {}
        }
    }

    pub fn insert_client(&mut self, client: NewClient) {
        let NewClient {
            client,
            restricted,
            credentials_unknown,
            security_context,
        } = client;

        let config = self.config.borrow();
        let data = Arc::new(ClientState {
            compositor_state: Default::default(),
            can_view_decoration_globals: config.prefer_no_csd,
            primary_selection_disabled: config.clipboard.disable_primary,
            restricted,
            credentials_unknown,
            security_context,
        });

        if let Err(err) = self.display_handle.insert_client(client, data) {
            warn!("error inserting client: {err}");
        }
    }

    #[cfg(feature = "dbus")]
    pub fn inhibit_power_key(&mut self) -> anyhow::Result<()> {
        use smithay::reexports::rustix::io::{fcntl_setfd, FdFlags};

        let conn = zbus::blocking::Connection::system()?;

        let message = conn.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &(
                "handle-power-key",
                "swayward",
                "Power key handling",
                "block",
            ),
        )?;

        let fd: zbus::zvariant::OwnedFd = message.body().deserialize()?;

        // Don't leak the fd to child processes.
        if let Err(err) = fcntl_setfd(&fd, FdFlags::CLOEXEC) {
            warn!("error setting CLOEXEC on inhibit fd: {err:?}");
        };

        self.inhibit_power_key_fd = Some(fd);

        Ok(())
    }

    /// Repositions all outputs, optionally adding a new output.
    fn ensure_lock_redraw(&mut self, output: &Output) {
        let output = output.downgrade();
        self.event_loop
            .insert_source(
                Timer::from_duration(Duration::from_millis(100)),
                move |_, _, state| {
                    if !matches!(state.swayward.lock_state, LockState::Locking(_)) {
                        return TimeoutAction::Drop;
                    }
                    let Some(output) = output.upgrade() else {
                        return TimeoutAction::Drop;
                    };
                    let Some(output_state) = state.swayward.output_state.get_mut(&output) else {
                        return TimeoutAction::Drop;
                    };
                    if output_state.lock_render_state == LockRenderState::Locked {
                        return TimeoutAction::Drop;
                    }

                    output_state.redraw_state =
                        mem::take(&mut output_state.redraw_state).queue_redraw();
                    TimeoutAction::ToDuration(Duration::from_millis(100))
                },
            )
            .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn set_test_lock_deadline(&mut self, deadline: Duration) {
        self.lock_deadline = deadline;
    }

    pub fn is_locked(&self) -> bool {
        match self.lock_state {
            LockState::Unlocked | LockState::WaitingForSurfaces { .. } => false,
            LockState::Locking(_) | LockState::Locked(_) => true,
        }
    }

    pub fn lock(&mut self, confirmation: SessionLocker) {
        // Check if another client is in the process of locking.
        if matches!(
            self.lock_state,
            LockState::WaitingForSurfaces { .. } | LockState::Locking(_)
        ) {
            info!("refusing lock as another client is currently locking");
            return;
        }

        // Check if we're already locked with an active client.
        if let LockState::Locked(lock) = &self.lock_state {
            if lock.is_alive() {
                info!("refusing lock as already locked with an active client");
                return;
            }

            // If the client had died, continue with the new lock.
            info!("locking session (replacing existing dead lock)");

            // Since the session was already locked, we know that the outputs are blanked, and
            // can lock right away.
            let lock = confirmation.ext_session_lock().clone();
            confirmation.lock();
            self.lock_state = LockState::Locked(lock);

            return;
        }

        info!("locking session");

        if self.output_state.is_empty() {
            // There are no outputs, lock the session right away.
            self.screenshot_ui.close();
            self.cursor_manager
                .set_cursor_image(CursorImageStatus::default_named());

            let lock = confirmation.ext_session_lock().clone();
            confirmation.lock();
            self.lock_state = LockState::Locked(lock);
        } else {
            // There are outputs which we need to redraw before locking. But before we do that,
            // let's wait for the lock surfaces.
            //
            // Give them a second; swaylock can take its time to paint a big enough image.
            #[cfg(not(test))]
            let lock_deadline = Duration::from_millis(1000);
            #[cfg(test)]
            let lock_deadline = self.lock_deadline;
            let timer = Timer::from_duration(lock_deadline);
            let deadline_token = self
                .event_loop
                .insert_source(timer, |_, _, state| {
                    trace!("lock deadline expired, continuing");
                    state.swayward.continue_to_locking();
                    TimeoutAction::Drop
                })
                .unwrap();

            self.lock_state = LockState::WaitingForSurfaces {
                confirmation,
                deadline_token,
            };
        }
    }

    pub fn maybe_continue_to_locking(&mut self) {
        if !matches!(self.lock_state, LockState::WaitingForSurfaces { .. }) {
            // Not waiting.
            return;
        }

        // Check if there are any outputs whose lock surfaces had not had a commit yet.
        for state in self.output_state.values() {
            let Some(surface) = &state.lock_surface else {
                // Surface not created yet.
                return;
            };

            if !is_mapped(surface.wl_surface()) {
                return;
            }
        }

        // All good.
        trace!("lock surfaces are ready, continuing");
        self.continue_to_locking();
    }

    fn continue_to_locking(&mut self) {
        match mem::take(&mut self.lock_state) {
            LockState::WaitingForSurfaces {
                confirmation,
                deadline_token,
            } => {
                self.event_loop.remove(deadline_token);

                self.screenshot_ui.close();
                self.cursor_manager
                    .set_cursor_image(CursorImageStatus::default_named());
                self.cancel_mru();

                if self.output_state.is_empty() {
                    // There are no outputs, lock the session right away.
                    let lock = confirmation.ext_session_lock().clone();
                    confirmation.lock();
                    self.lock_state = LockState::Locked(lock);
                } else {
                    // There are outputs which we need to redraw before locking.
                    self.lock_state = LockState::Locking(confirmation);
                    self.queue_redraw_all();
                    for output in self.output_state.keys().cloned().collect::<Vec<_>>() {
                        self.ensure_lock_redraw(&output);
                    }
                }
            }
            other => {
                error!("continue_to_locking() called with wrong lock state: {other:?}",);
                self.lock_state = other;
            }
        }
    }

    pub fn unlock(&mut self) {
        info!("unlocking session");

        let prev = mem::take(&mut self.lock_state);
        if let LockState::WaitingForSurfaces { deadline_token, .. } = prev {
            self.event_loop.remove(deadline_token);
        }

        for output_state in self.output_state.values_mut() {
            output_state.lock_surface = None;
        }
        self.queue_redraw_all();
    }

    #[cfg(feature = "dbus")]
    fn update_locked_hint(&mut self) {
        use std::sync::LazyLock;

        if !self.is_session_instance {
            return;
        }

        static XDG_SESSION_ID: LazyLock<Option<String>> = LazyLock::new(|| {
            let id = std::env::var("XDG_SESSION_ID").ok();
            if id.is_none() {
                warn!(
                    "env var 'XDG_SESSION_ID' is unset or invalid; logind LockedHint won't be set"
                );
            }
            id
        });

        let Some(session_id) = &*XDG_SESSION_ID else {
            return;
        };

        fn call(session_id: &str, locked: bool) -> anyhow::Result<()> {
            let conn = zbus::blocking::Connection::system()
                .context("error connecting to the system bus")?;

            let message = conn
                .call_method(
                    Some("org.freedesktop.login1"),
                    "/org/freedesktop/login1",
                    Some("org.freedesktop.login1.Manager"),
                    "GetSession",
                    &(session_id),
                )
                .context("failed to call GetSession")?;

            let message_body = message.body();
            let session_path: zbus::zvariant::ObjectPath = message_body
                .deserialize()
                .context("failed to deserialize GetSession reply")?;

            conn.call_method(
                Some("org.freedesktop.login1"),
                session_path,
                Some("org.freedesktop.login1.Session"),
                "SetLockedHint",
                &(locked),
            )
            .context("failed to call SetLockedHint")?;

            Ok(())
        }

        // Consider only the fully locked state here. When using the locked hint with sleep
        // inhibitor tools, we want to allow sleep only after the screens are fully cleared with
        // the lock screen, which corresponds to the Locked state.
        let locked = matches!(self.lock_state, LockState::Locked(_));

        if self.locked_hint.is_some_and(|h| h == locked) {
            return;
        }

        self.locked_hint = Some(locked);

        let res = thread::Builder::new()
            .name("Logind LockedHint Updater".to_owned())
            .spawn(move || {
                let _span = tracy_client::span!("LockedHint");

                if let Err(err) = call(session_id, locked) {
                    warn!("failed to set logind LockedHint: {err:?}");
                }
            });

        if let Err(err) = res {
            warn!("error spawning a thread to set logind LockedHint: {err:?}");
        }
    }

    pub fn new_lock_surface(&mut self, surface: LockSurface, output: &Output) {
        let lock = match &self.lock_state {
            LockState::Unlocked => {
                error!("tried to add a lock surface on an unlocked session");
                return;
            }
            LockState::WaitingForSurfaces { confirmation, .. } => confirmation.ext_session_lock(),
            LockState::Locking(confirmation) => confirmation.ext_session_lock(),
            LockState::Locked(lock) => lock,
        };

        if lock.client() != surface.wl_surface().client() {
            debug!("ignoring lock surface from an unrelated client");
            return;
        }

        if lock != surface.ext_session_lock() {
            debug!("ignoring lock surface from an unrelated lock instance");
            return;
        }

        let Some(output_state) = self.output_state.get_mut(output) else {
            error!("missing output state");
            return;
        };

        output_state.lock_surface = Some(surface);
    }

    /// Activates the pointer constraint if necessary according to the current pointer contents.
    ///
    /// Make sure the pointer location and contents are up to date before calling this.
    pub fn maybe_activate_pointer_constraint(&self) {
        let Some((surface, surface_loc)) = &self.pointer_contents.surface else {
            return;
        };

        let pointer = self.seat.get_pointer().unwrap();
        if Some(surface) != pointer.current_focus().as_ref() {
            return;
        }

        with_pointer_constraint(surface, &pointer, |constraint| {
            let Some(constraint) = constraint else { return };

            if constraint.is_active() {
                return;
            }

            // Constraint does not apply if not within region.
            if let Some(region) = constraint.region() {
                let pointer_pos = pointer.current_location();
                let pos_within_surface = pointer_pos - *surface_loc;
                if !region.contains(pos_within_surface.to_i32_round()) {
                    return;
                }
            }

            constraint.activate();
        });
    }

    pub fn focus_layer_surface_if_on_demand(&mut self, surface: Option<LayerSurface>) {
        if let Some(surface) = surface {
            if surface.cached_state().keyboard_interactivity
                == wlr_layer::KeyboardInteractivity::OnDemand
            {
                if self.layer_shell_on_demand_focus.as_ref() != Some(&surface) {
                    self.layer_shell_on_demand_focus = Some(surface);

                    // FIXME: granular.
                    self.queue_redraw_all();
                }

                return;
            }
        }

        // Something else got clicked, clear on-demand layer-shell focus.
        if self.layer_shell_on_demand_focus.is_some() {
            self.layer_shell_on_demand_focus = None;

            // FIXME: granular.
            self.queue_redraw_all();
        }
    }

    /// Tries to find and return the root shell surface for a given surface.
    ///
    /// I.e. for popups, this function will try to find the parent toplevel or layer surface. For
    /// regular subsurfaces, it will find the root surface.
    pub fn find_root_shell_surface(&self, surface: &WlSurface) -> WlSurface {
        let Some(root) = self.root_surface.get(surface) else {
            return surface.clone();
        };

        if let Some(popup) = self.popups.find_popup(root) {
            return find_popup_root_surface(&popup).unwrap_or_else(|_| root.clone());
        }

        root.clone()
    }

    #[cfg(feature = "dbus")]
    pub fn on_ipc_outputs_changed(&self) {
        let _span = tracy_client::span!("Swayward::on_ipc_outputs_changed");

        let Some(dbus) = &self.dbus else { return };
        let Some(conn_display_config) = dbus.conn_display_config.clone() else {
            return;
        };

        let res = thread::Builder::new()
            .name("DisplayConfig MonitorsChanged Emitter".to_owned())
            .spawn(move || {
                use crate::dbus::mutter_display_config::DisplayConfig;
                let _span = tracy_client::span!("MonitorsChanged");
                let iface = match conn_display_config
                    .object_server()
                    .interface::<_, DisplayConfig>("/org/gnome/Mutter/DisplayConfig")
                {
                    Ok(iface) => iface,
                    Err(err) => {
                        warn!("error getting DisplayConfig interface: {err:?}");
                        return;
                    }
                };

                async_io::block_on(async move {
                    if let Err(err) = DisplayConfig::monitors_changed(iface.signal_emitter()).await
                    {
                        warn!("error emitting MonitorsChanged: {err:?}");
                    }
                });
            });

        if let Err(err) = res {
            warn!("error spawning a thread to send MonitorsChanged: {err:?}");
        }
    }

    pub fn do_screen_transition(&mut self, renderer: &mut GlesRenderer, delay_ms: Option<u16>) {
        let _span = tracy_client::span!("Swayward::do_screen_transition");

        self.update_render_elements(None);

        let textures: Vec<_> = self
            .output_state
            .keys()
            .cloned()
            .filter_map(|output| {
                let size = output.current_mode().unwrap().size;
                let transform = output.current_transform();

                let scale = Scale::from(output.current_scale().fractional_scale());
                let targets = [
                    RenderTarget::Output,
                    RenderTarget::Screencast,
                    RenderTarget::ScreenCapture,
                ];
                let textures = targets.map(|target| {
                    let ctx = RenderCtx {
                        renderer,
                        target,
                        xray: None,
                    };
                    let elements = self.render_to_vec(ctx, &output, false);
                    let elements = elements.iter().rev();

                    let res = render_to_texture(
                        renderer,
                        size,
                        scale,
                        transform,
                        Fourcc::Abgr8888,
                        elements,
                    );

                    if let Err(err) = &res {
                        warn!("error rendering output {}: {err:?}", output.name());
                    }

                    res
                });

                if textures.iter().any(|res| res.is_err()) {
                    return None;
                }

                let textures = textures.map(|res| {
                    let texture = res.unwrap().0;
                    TextureBuffer::from_texture(
                        renderer,
                        texture,
                        scale,
                        transform,
                        Vec::new(), // We want windows below to get frame callbacks.
                    )
                });

                Some((output, textures))
            })
            .collect();

        let delay = delay_ms.map_or(screen_transition::DELAY, |d| {
            Duration::from_millis(u64::from(d))
        });

        for (output, from_texture) in textures {
            let state = self.output_state.get_mut(&output).unwrap();
            state.screen_transition = Some(ScreenTransition::new(
                from_texture,
                delay,
                self.clock.clone(),
            ));
        }

        // We don't actually need to queue a redraw because the point is to freeze the screen for a
        // bit, and even if the delay was zero, we're drawing the same contents anyway.
    }

    pub fn recompute_window_rules(&mut self) {
        let _span = tracy_client::span!("Swayward::recompute_window_rules");

        let changed = {
            let window_rules = &self.config.borrow().window_rules;

            for unmapped in self.unmapped_windows.values_mut() {
                let new_rules = ResolvedWindowRules::compute(
                    window_rules,
                    WindowRef::Unmapped(unmapped),
                    self.is_at_startup,
                );
                if let InitialConfigureState::Configured { rules, .. } = &mut unmapped.state {
                    *rules = new_rules;
                }
            }

            let mut windows = vec![];
            self.layout.with_windows_mut(|mapped, _| {
                if mapped.recompute_window_rules(window_rules, self.is_at_startup) {
                    windows.push(mapped.window.clone());
                }
            });
            let changed = !windows.is_empty();
            for win in windows {
                self.layout.update_window(&win, None);
            }
            changed
        };

        if changed {
            // FIXME: granular.
            self.queue_redraw_all();
        }
    }

    pub fn recompute_layer_rules(&mut self) {
        let _span = tracy_client::span!("Swayward::recompute_layer_rules");

        let mut changed = false;
        {
            let config = self.config.borrow();
            let rules = &config.layer_rules;

            for mapped in self.mapped_layer_surfaces.values_mut() {
                if mapped.recompute_layer_rules(rules, self.is_at_startup) {
                    changed = true;
                    mapped.update_config(&config);
                }
            }
        }

        if changed {
            // FIXME: granular.
            self.queue_redraw_all();
        }
    }

    fn focus_clears_urgency(&mut self, surface: &WlSurface, changed_workspace: bool) {
        let Some((mapped, _)) = self.layout.find_window_and_output_mut(surface) else {
            return;
        };
        if !mapped.is_urgent() || self.urgency_timers.contains_key(&mapped.id()) {
            return;
        }

        let id = mapped.id();
        let timeout_ms = self.config.borrow().urgent_timeout_ms;
        if !changed_workspace || timeout_ms == 0 {
            mapped.set_urgent(false);
            return;
        }

        let token = self
            .event_loop
            .insert_source(
                Timer::from_duration(Duration::from_millis(u64::from(timeout_ms))),
                move |_, _, state| {
                    state.swayward.urgency_timers.remove(&id);
                    state.swayward.clear_window_urgency(id);
                    TimeoutAction::Drop
                },
            )
            .unwrap();
        self.urgency_timers.insert(id, token);
    }

    fn clear_window_urgency(&mut self, id: MappedId) {
        self.layout.with_windows_mut(|mapped, _| {
            if mapped.id() == id {
                mapped.set_urgent(false);
            }
        });
        self.queue_redraw_all();
    }

    #[cfg(test)]
    pub fn fire_urgency_timer_for_test(&mut self, id: MappedId) -> bool {
        let Some(token) = self.urgency_timers.remove(&id) else {
            return false;
        };
        self.event_loop.remove(token);
        self.clear_window_urgency(id);
        true
    }

    pub fn cancel_urgency_timer(&mut self, id: MappedId) {
        if let Some(token) = self.urgency_timers.remove(&id) {
            self.event_loop.remove(token);
        }
    }

    pub fn set_window_urgent(&mut self, id: MappedId, urgent: bool) {
        if !urgent {
            self.cancel_urgency_timer(id);
        }
        self.layout.with_windows_mut(|mapped, _| {
            if mapped.id() == id {
                mapped.set_urgent(urgent);
            }
        });
    }

    pub fn reset_pointer_inactivity_timer(&mut self) {
        if self.pointer_inactivity_timer_got_reset {
            return;
        }

        let _span = tracy_client::span!("Swayward::reset_pointer_inactivity_timer");

        if let Some(token) = self.pointer_inactivity_timer.take() {
            self.event_loop.remove(token);
        }

        let Some(timeout_ms) = self.config.borrow().cursor.hide_after_inactive_ms else {
            return;
        };

        let duration = Duration::from_millis(timeout_ms as u64);
        let timer = Timer::from_duration(duration);
        let token = self
            .event_loop
            .insert_source(timer, move |_, _, state| {
                state.swayward.pointer_inactivity_timer = None;

                // If the pointer is already invisible, don't reset it back to Hidden causing one
                // frame of hover.
                if state.swayward.pointer_visibility.is_visible() {
                    state.swayward.pointer_visibility = PointerVisibility::Hidden;
                    state.swayward.queue_redraw_all();
                }

                TimeoutAction::Drop
            })
            .unwrap();
        self.pointer_inactivity_timer = Some(token);

        self.pointer_inactivity_timer_got_reset = true;
    }

    pub fn notify_activity(&mut self) {
        if self.notified_activity_this_iteration {
            return;
        }

        let _span = tracy_client::span!("Swayward::notify_activity");

        self.idle_notifier_state.notify_activity(&self.seat);

        self.notified_activity_this_iteration = true;
    }

    pub fn close_mru(&mut self, close_request: MruCloseRequest) -> Option<Window> {
        if !self.window_mru_ui.is_open() {
            return None;
        }
        self.queue_redraw_all();

        let id = self.window_mru_ui.close(close_request)?;
        self.find_window_by_id(id)
    }

    pub fn cancel_mru(&mut self) {
        self.close_mru(MruCloseRequest::Cancel);
    }

    /// Apply a pending MRU commit immediately.
    ///
    /// Called for example on keyboard events that reach the active window, which immediately adds
    /// it to the MRU.
    pub fn mru_apply_keyboard_commit(&mut self) {
        let Some(pending) = self.pending_mru_commit.take() else {
            return;
        };
        self.event_loop.remove(pending.token);

        if let Some(window) = self
            .layout
            .workspaces_mut()
            .flat_map(|ws| ws.windows_mut())
            .find(|w| w.id() == pending.id)
        {
            window.set_focus_timestamp(pending.stamp);
        }
    }

    pub fn queue_redraw_mru_output(&mut self) {
        if let Some(output) = self.window_mru_ui.output().cloned() {
            self.queue_redraw(&output);
        }
    }
}

pub struct NewClient {
    pub client: UnixStream,
    pub restricted: bool,
    pub credentials_unknown: bool,
    pub security_context: Option<SecurityContextMetadata>,
}

#[derive(Clone, Debug, Default)]
pub struct SecurityContextMetadata {
    pub sandbox_engine: Option<String>,
    pub app_id: Option<String>,
    pub instance_id: Option<String>,
}

pub struct ClientState {
    pub compositor_state: CompositorClientState,
    pub can_view_decoration_globals: bool,
    pub primary_selection_disabled: bool,
    /// Whether this client is denied from the restricted protocols such as security-context.
    pub restricted: bool,
    /// We cannot retrieve this client's socket credentials.
    pub credentials_unknown: bool,
    pub security_context: Option<SecurityContextMetadata>,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

fn scale_relocate_crop<E: Element>(
    elem: E,
    output_scale: Scale<f64>,
    zoom: f64,
    ws_geo: Rectangle<f64, Logical>,
) -> Option<CropRenderElement<RelocateRenderElement<RescaleRenderElement<E>>>> {
    let ws_geo = ws_geo.to_physical_precise_round(output_scale);
    let elem = RescaleRenderElement::from_element(elem, Point::from((0, 0)), zoom);
    let elem = RelocateRenderElement::from_element(elem, ws_geo.loc, Relocate::Relative);
    CropRenderElement::from_element(elem, output_scale, ws_geo)
}

swayward_render_elements! {
    PointerRenderElements<R> => {
        Wayland = WaylandSurfaceRenderElement<R>,
        NamedPointer = MemoryRenderBufferRenderElement<R>,
    }
}

swayward_render_elements! {
    WindowScreenshotRenderElement<R> => {
        Layout = LayoutElementRenderElement<R>,
        Pointer = RelocateRenderElement<PointerRenderElements<R>>,
    }
}

swayward_render_elements! {
    OutputRenderElements<R> => {
        Monitor = MonitorRenderElement<R>,
        RescaledTile = RescaleRenderElement<TileRenderElement<R>>,
        LayerSurface = LayerSurfaceRenderElement<R>,
        RelocatedLayerSurface = CropRenderElement<RelocateRenderElement<RescaleRenderElement<
            LayerSurfaceRenderElement<R>
        >>>,
        RelocatedColor = CropRenderElement<RelocateRenderElement<RescaleRenderElement<
            SolidColorRenderElement
        >>>,
        Pointer = PointerRenderElements<R>,
        Wayland = WaylandSurfaceRenderElement<R>,
        SolidColor = SolidColorRenderElement,
        ScreenshotUi = ScreenshotUiRenderElement,
        WindowMruUi = WindowMruUiRenderElement<R>,
        ExitConfirmDialog = ExitConfirmDialogRenderElement,
        Texture = PrimaryGpuTextureRenderElement,
        // Used for the CPU-rendered panels.
        RelocatedMemoryBuffer = RelocateRenderElement<MemoryRenderBufferRenderElement<R>>,
    }
}

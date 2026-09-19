//! Window layout logic.
//!
//! Niri implements scrollable tiling with dynamic workspaces. The scrollable tiling is mostly
//! orthogonal to any particular workspace system, though outputs living in separate coordinate
//! spaces suggest per-output workspaces.
//!
//! I chose a dynamic workspace system because I think it works very well. In particular, it works
//! naturally across outputs getting added and removed, since workspaces can move between outputs
//! as necessary.
//!
//! In the layout, one output (the first one to be added) is designated as *primary*. This is where
//! workspaces from disconnected outputs will move. Currently, the primary output has no other
//! distinction from other outputs.
//!
//! Where possible, niri tries to follow these principles with regards to outputs:
//!
//! 1. Disconnecting and reconnecting the same output must not change the layout.
//!    * This includes both secondary outputs and the primary output.
//! 2. Connecting an output must not change the layout for any workspaces that were never on that
//!    output.
//!
//! Therefore, we implement the following logic: every workspace keeps track of which output it
//! originated on—its *original output*. When an output disconnects, its workspaces are appended to
//! the (potentially new) primary output, but remember their original output. Then, if the original
//! output connects again, all workspaces originally from there move back to that output.
//!
//! In order to avoid surprising behavior, if the user creates or moves any new windows onto a
//! workspace, it forgets its original output, and its current output becomes its original output.
//! Imagine a scenario: the user works with a laptop and a monitor at home, then takes their laptop
//! with them, disconnecting the monitor, and keeps working as normal, using the second monitor's
//! workspace just like any other. Then they come back, reconnect the second monitor, and now we
//! don't want an unassuming workspace to end up on it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::mem;
use std::rc::Rc;
use std::time::Duration;

use monitor::{InsertHint, InsertPosition, InsertWorkspace, MonitorAddWindowTarget};
use scrolling::ColumnWidth;
use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::utils::RescaleRenderElement;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::output::{self, Output};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size, Transform};
use swayward_config::utils::MergeWith as _;
use swayward_config::{
    Config, CornerRadius, LayoutPart, PresetSize, Workspace as WorkspaceConfig, WorkspaceReference,
};
use swayward_ipc::{ColumnDisplay, PositionChange, SizeChange, WindowLayout};
use tile::{Tile, TileRenderElement};
use workspace::{WorkspaceAddWindowTarget, WorkspaceId};

use self::monitor::Monitor;
pub use self::monitor::MonitorRenderElement;
use self::workspace::{OutputId, Workspace};
use crate::animation::{Animation, Clock};
use crate::input::swipe_tracker::SwipeTracker;
use crate::layout::scrolling::ScrollDirection;
use crate::render_helpers::background_effect::BackgroundEffectElement;
use crate::render_helpers::offscreen::OffscreenData;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::snapshot::RenderSnapshot;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::texture::TextureBuffer;
use crate::render_helpers::xray::{Xray, XrayPos};
use crate::render_helpers::{BakedBuffer, RenderCtx};
use crate::rubber_band::RubberBand;
use crate::swayward_render_elements;
use crate::utils::transaction::{Transaction, TransactionBlocker};
use crate::utils::{
    ensure_min_max_size_maybe_zero, output_matches_name, output_size, round_logical_in_physical,
    round_logical_in_physical_max1, ResizeEdge,
};
use crate::window::ResolvedWindowRules;

pub mod closing_window;
pub mod floating;
pub mod focus_ring;
pub mod insert_hint_element;
pub mod monitor;
pub mod opening_window;
#[allow(dead_code)]
pub mod scrolling;
pub mod shadow;
pub mod tab_indicator;
pub mod tile;
pub mod tiling_tree;
mod titlebar;
pub mod workspace;

#[cfg(test)]
mod tests;

/// Size changes up to this many pixels don't animate.
pub const RESIZE_ANIMATION_THRESHOLD: f64 = 10.;

/// Pointer needs to move this far to pull a window from the layout.
const INTERACTIVE_MOVE_START_THRESHOLD: f64 = 256. * 256.;

/// Opacity of interactively moved tiles targeting the scrolling layout.
const INTERACTIVE_MOVE_ALPHA: f64 = 0.75;

/// Amount of touchpad movement to toggle the overview.
const OVERVIEW_GESTURE_MOVEMENT: f64 = 300.;

const OVERVIEW_GESTURE_RUBBER_BAND: RubberBand = RubberBand {
    stiffness: 0.5,
    limit: 0.05,
};

/// Size-relative units.
pub struct SizeFrac;

swayward_render_elements! {
    LayoutElementRenderElement<R> => {
        Wayland = WaylandSurfaceRenderElement<R>,
        SolidColor = SolidColorRenderElement,
        BackgroundEffect = BackgroundEffectElement,
    }
}

pub type LayoutElementRenderSnapshot =
    RenderSnapshot<BakedBuffer<TextureBuffer<GlesTexture>>, BakedBuffer<SolidColorBuffer>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizingMode {
    Normal,
    Maximized,
    Fullscreen,
}

pub trait LayoutElement {
    /// Type that can be used as a unique ID of this element.
    type Id: PartialEq + std::fmt::Debug + Clone;

    /// Unique ID of this element.
    fn id(&self) -> &Self::Id;

    /// Most recent time this element received keyboard focus.
    fn focus_timestamp(&self) -> Option<Duration> {
        None
    }

    /// Updates the config for the element.
    fn update_config(&mut self, blur_config: swayward_config::Blur) {
        let _ = blur_config;
    }

    /// Title displayed in server-side decorations.
    fn title(&self) -> String {
        String::new()
    }

    /// Visual size of the element.
    ///
    /// This is what the user would consider the size, i.e. excluding CSD shadows and whatnot.
    /// Corresponds to the Wayland window geometry size.
    fn size(&self) -> Size<i32, Logical>;

    /// Returns the location of the element's buffer relative to the element's visual geometry.
    ///
    /// I.e. if the element has CSD shadows, its buffer location will have negative coordinates.
    fn buf_loc(&self) -> Point<i32, Logical>;

    /// Checks whether a point is in the element's input region.
    ///
    /// The point is relative to the element's visual geometry.
    fn is_in_input_region(&self, point: Point<f64, Logical>) -> bool;

    /// Renders the element at the given visual location.
    ///
    /// The element should be rendered in such a way that its visual geometry ends up at the given
    /// location.
    fn render<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        scale: Scale<f64>,
        alpha: f32,
        xray_pos: XrayPos,
        push: &mut dyn FnMut(LayoutElementRenderElement<R>),
    ) {
        self.render_popups(ctx.r(), location, scale, alpha, xray_pos, push);
        self.render_normal(ctx.r(), location, scale, alpha, push);
    }

    /// Renders the non-popup parts of the element.
    fn render_normal<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        scale: Scale<f64>,
        alpha: f32,
        push: &mut dyn FnMut(LayoutElementRenderElement<R>),
    ) {
        let _ = (ctx, location, scale, alpha, push);
    }

    /// Renders the popups of the element.
    fn render_popups<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        location: Point<f64, Logical>,
        scale: Scale<f64>,
        alpha: f32,
        xray_pos: XrayPos,
        push: &mut dyn FnMut(LayoutElementRenderElement<R>),
    ) {
        let _ = (ctx, location, scale, alpha, xray_pos, push);
    }

    /// Renders the background effect behind the main surface of the element.
    #[allow(clippy::too_many_arguments)]
    fn render_background_effect(
        &self,
        _ctx: RenderCtx<GlesRenderer>,
        _geometry: Rectangle<f64, Logical>,
        _scale: f64,
        _clip_to_geometry: bool,
        _surface_anim_scale: Scale<f64>,
        _radius: CornerRadius,
        _xray_pos: XrayPos,
        _push: &mut dyn FnMut(BackgroundEffectElement),
    ) {
    }

    /// Requests the element to change its size.
    ///
    /// The size request is stored and will be continuously sent to the element on any further
    /// state changes.
    fn request_size(
        &mut self,
        size: Size<i32, Logical>,
        mode: SizingMode,
        animate: bool,
        transaction: Option<Transaction>,
    );

    /// Requests the element to change size once, clearing the request afterwards.
    fn request_size_once(&mut self, size: Size<i32, Logical>, animate: bool) {
        self.request_size(size, SizingMode::Normal, animate, None);
    }

    fn min_size(&self) -> Size<i32, Logical>;
    fn max_size(&self) -> Size<i32, Logical>;
    fn is_wl_surface(&self, wl_surface: &WlSurface) -> bool;
    fn has_ssd(&self) -> bool;
    fn set_preferred_scale_transform(&self, scale: output::Scale, transform: Transform);
    fn output_enter(&self, output: &Output);
    fn output_leave(&self, output: &Output);
    fn set_offscreen_data(&self, data: Option<OffscreenData>);
    fn set_activated(&mut self, active: bool);
    fn set_active_in_column(&mut self, active: bool);
    fn set_floating(&mut self, floating: bool);
    fn has_xdg_decoration(&self) -> bool {
        false
    }
    fn request_server_decoration(&mut self, server_side: bool) {
        let _ = server_side;
    }
    fn set_bounds(&self, bounds: Size<i32, Logical>);
    fn is_ignoring_opacity_window_rule(&self) -> bool;

    fn is_urgent(&self) -> bool;

    fn configure_intent(&self) -> ConfigureIntent;
    fn send_pending_configure(&mut self);

    /// The element's current sizing mode.
    ///
    /// This will *not* switch immediately after a [`LayoutElement::request_size()`] call.
    fn sizing_mode(&self) -> SizingMode;

    /// The sizing mode that we're requesting the element to assume.
    ///
    /// This *will* switch immediately after a [`LayoutElement::request_size()`] call.
    fn pending_sizing_mode(&self) -> SizingMode;

    /// Size previously requested through [`LayoutElement::request_size()`].
    fn requested_size(&self) -> Option<Size<i32, Logical>>;

    /// Client geometry captured when the window first mapped.
    fn natural_size(&self) -> Size<i32, Logical> {
        self.size()
    }

    /// Size to expose through IPC while a configure is pending.
    fn ipc_size(&self) -> Size<i32, Logical> {
        self.size()
    }

    /// Non-fullscreen size that we expect this window has or will shortly have.
    ///
    /// This can be different from [`requested_size()`](LayoutElement::requested_size()). For
    /// example, for floating windows this will generally return the current window size, rather
    /// than the last size that we requested, since we want floating windows to be able to change
    /// size freely. But not always: if we just requested a floating window to resize and it hasn't
    /// responded to it yet, this will return the newly requested size.
    ///
    /// This function should never return a 0 size component. `None` means there's no known
    /// expected size (for example, the window is fullscreen).
    ///
    /// The default impl is for testing only, it will not preserve the window's own size changes.
    fn expected_size(&self) -> Option<Size<i32, Logical>> {
        if self.sizing_mode().is_fullscreen() {
            return None;
        }

        let mut requested = self.requested_size().unwrap_or_default();
        let current = self.size();
        if requested.w == 0 {
            requested.w = current.w;
        }
        if requested.h == 0 {
            requested.h = current.h;
        }
        Some(requested)
    }

    fn is_windowed_fullscreen(&self) -> bool {
        false
    }
    fn is_pending_windowed_fullscreen(&self) -> bool {
        false
    }
    fn request_windowed_fullscreen(&mut self, value: bool) {
        let _ = value;
    }

    /// The effective geometry corner radius for this element.
    ///
    /// Returns zero when the element is in windowed fullscreen, since fullscreen windows have
    /// square corners.
    ///
    /// This method only handles windowed fullscreen and not maximized/real fullscreen. This is
    /// because windowed fullscreen is handled by the element itself, whereas other sizing modes
    /// are handled externally by the Tile, so the corner radius changes for those modes is also
    /// handled externally.
    fn geometry_corner_radius(&self) -> CornerRadius {
        let rules = self.rules();

        // When windows think they're fullscreen, they square their corners.
        //
        // However, if the user is clipping the window to geometry, they are likely going for
        // consistent corner radius, and want this radius to remain in windowed fullscreen.
        if self.is_windowed_fullscreen() && rules.clip_to_geometry != Some(true) {
            return CornerRadius::default();
        }

        rules.geometry_corner_radius.unwrap_or_default()
    }

    fn is_child_of(&self, parent: &Self) -> bool;

    fn rules(&self) -> &ResolvedWindowRules;

    /// Runs periodic clean-up tasks.
    fn refresh(&self);

    fn take_animation_snapshot(&mut self) -> Option<LayoutElementRenderSnapshot>;

    fn set_interactive_resize(&mut self, data: Option<InteractiveResizeData>);
    fn cancel_interactive_resize(&mut self);
    fn interactive_resize_data(&self) -> Option<InteractiveResizeData>;

    fn on_commit(&mut self, serial: Serial);
}

#[derive(Debug)]
pub struct Layout<W: LayoutElement> {
    /// Monitors and workspaes in the layout.
    monitor_set: MonitorSet<W>,
    /// Whether the layout should draw as active.
    ///
    /// This normally indicates that the layout has keyboard focus, but not always. E.g. when the
    /// screenshot UI is open, it keeps the layout drawing as active.
    is_active: bool,
    /// Map from monitor name to id of its last active workspace.
    ///
    /// This data is stored upon monitor removal and is used to restore the active workspace when
    /// the monitor is reconnected.
    ///
    /// The workspace id does not necessarily point to a valid workspace. If it doesn't, then it is
    /// simply ignored.
    last_active_workspace_id: HashMap<String, WorkspaceId>,
    /// Windows hidden on sway's synthetic scratchpad workspace.
    scratchpad: VecDeque<RemovedTile<W>>,
    /// All scratchpad windows, including the one currently shown.
    scratchpad_windows: Vec<W::Id>,
    /// Ongoing interactive move.
    interactive_move: Option<InteractiveMoveState<W>>,
    /// Ongoing drag-and-drop operation.
    dnd: Option<DndData<W>>,
    /// Clock for driving animations.
    clock: Clock,
    /// Time that we last updated render elements for.
    update_render_elements_time: Duration,
    /// Whether the overview is open.
    ///
    /// This is a boolean flag that controls things like where input goes to. The actual animation
    /// is controlled by overview_progress.
    overview_open: bool,
    /// The overview zoom progress.
    overview_progress: Option<OverviewProgress>,
    /// Configurable properties of the layout.
    options: Rc<Options>,
    /// Absolute workspace targets from default-mode binds, in declaration order.
    initial_workspace_names: Vec<String>,
    /// Workspace configuration used to resolve sway's ordered output assignments lazily.
    workspace_configs: Vec<WorkspaceConfig>,
}

#[derive(Debug)]
enum MonitorSet<W: LayoutElement> {
    /// At least one output is connected.
    Normal {
        /// Connected monitors.
        monitors: Vec<Monitor<W>>,
        /// Index of the primary monitor.
        primary_idx: usize,
        /// Index of the active monitor.
        active_monitor_idx: usize,
    },
    /// No outputs are connected, and these are the workspaces.
    NoOutputs {
        /// The workspaces.
        workspaces: Vec<Workspace<W>>,
    },
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Options {
    pub layout: swayward_config::Layout,
    pub animations: swayward_config::Animations,
    pub gestures: swayward_config::Gestures,
    pub overview: swayward_config::Overview,
    pub blur: swayward_config::Blur,
    // Debug flags.
    pub disable_resize_throttling: bool,
    pub disable_transactions: bool,
    pub deactivate_unfocused_windows: bool,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum InteractiveMoveState<W: LayoutElement> {
    /// Initial rubberbanding; the window remains in the layout.
    Starting {
        /// The window we're moving.
        window_id: W::Id,
        /// Current pointer delta from the starting location.
        pointer_delta: Point<f64, Logical>,
        /// Pointer location within the visual window geometry as ratio from geometry size.
        ///
        /// This helps the pointer remain inside the window as it resizes.
        pointer_ratio_within_window: (f64, f64),
    },
    /// Moving; the window is no longer in the layout.
    Moving(InteractiveMoveData<W>),
}

#[derive(Debug)]
struct InteractiveMoveData<W: LayoutElement> {
    /// The window being moved.
    pub(self) tile: Tile<W>,
    /// Output where the window is currently located/rendered.
    pub(self) output: Output,
    /// Current pointer position within output.
    pub(self) pointer_pos_within_output: Point<f64, Logical>,
    /// Window column width.
    pub(self) width: ColumnWidth,
    /// Whether the window column was full-width.
    pub(self) is_full_width: bool,
    /// Whether the window targets the floating layout.
    pub(self) is_floating: bool,
    /// Workspace that owned the tile before the move began.
    pub(self) source_workspace: WorkspaceId,
    /// Pointer location within the visual window geometry as ratio from geometry size.
    ///
    /// This helps the pointer remain inside the window as it resizes.
    pub(self) pointer_ratio_within_window: (f64, f64),
    /// Config overrides for the output where the window is currently located.
    ///
    /// Cached here to be accessible while an output is removed.
    pub(self) output_config: Option<swayward_config::LayoutPart>,
    /// Config overrides for the workspace where the window is currently located.
    ///
    /// To avoid sudden window changes when starting an interactive move, it will remember the
    /// config overrides for the workspace where the move originated from. As soon as the window
    /// moves over some different workspace though, this override will reset.
    pub(self) workspace_config: Option<(WorkspaceId, swayward_config::LayoutPart)>,
}

#[derive(Debug)]
pub struct DndData<W: LayoutElement> {
    /// Output where the pointer is currently located.
    output: Output,
    /// Current pointer position within output.
    pointer_pos_within_output: Point<f64, Logical>,
    /// Ongoing DnD hold to activate something.
    hold: Option<DndHold<W>>,
}

#[derive(Debug)]
struct DndHold<W: LayoutElement> {
    /// Time when we started holding on the target.
    start_time: Duration,
    target: DndHoldTarget<W::Id>,
}

#[derive(Debug, PartialEq, Eq)]
enum DndHoldTarget<WindowId> {
    Window(WindowId),
    Workspace(WorkspaceId),
}

#[derive(Debug, Clone, Copy)]
pub struct InteractiveResizeData {
    pub(self) edges: ResizeEdge,
}

#[derive(Debug, Clone, Copy)]
pub enum ConfigureIntent {
    /// A configure is not needed (no changes to server pending state).
    NotNeeded,
    /// A configure is throttled (due to resizing too fast for example).
    Throttled,
    /// Can send the configure if it isn't throttled externally (only size changed).
    CanSend,
    /// Should send the configure regardless of external throttling (something other than size
    /// changed).
    ShouldSend,
}

/// Tile that was just removed from the layout.
#[derive(Debug)]
pub struct RemovedTile<W: LayoutElement> {
    tile: Tile<W>,
    /// Width of the column the tile was in.
    width: ColumnWidth,
    /// Whether the column the tile was in was full-width.
    is_full_width: bool,
    /// Whether the tile was floating.
    is_floating: bool,
    /// Working area whose coordinates the stored floating position uses.
    floating_working_area: Option<Rectangle<f64, Logical>>,
}

/// Whether to activate a newly added window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ActivateWindow {
    /// Activate unconditionally.
    Yes,
    /// Activate based on heuristics.
    #[default]
    Smart,
    /// Do not activate.
    No,
}

/// Where to put a newly added window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AddWindowTarget<'a, W: LayoutElement> {
    /// No particular preference.
    #[default]
    Auto,
    /// On this output.
    Output(&'a Output),
    /// On this workspace.
    Workspace(WorkspaceId),
    /// Next to this existing window.
    NextTo(&'a W::Id),
}

/// Type of the window hit from `window_under()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitType {
    /// The hit is within a window's input region and can be used for sending events to it.
    Input {
        /// Position of the window's buffer.
        win_pos: Point<f64, Logical>,
    },
    /// The hit can activate a window, but it is not in the input region so cannot send events.
    ///
    /// For example, this could be clicking on a tile border outside the window.
    Activate {
        /// Whether the hit was on the tab indicator.
        is_tab_indicator: bool,
    },
}

#[derive(Debug)]
enum OverviewProgress {
    Animation(Animation),
    Gesture(OverviewGesture),
    Open,
}

#[derive(Debug)]
struct OverviewGesture {
    tracker: SwipeTracker,
    /// Start point.
    start: f64,
    /// Current progress.
    value: f64,
}

/// Layer of windows to render.
#[derive(Clone, Copy)]
pub enum RenderLayer {
    Normal,
    /// Windows currently moving between workspaces.
    MovingBetweenWorkspaces,
}

impl SizingMode {
    #[must_use]
    pub fn is_normal(&self) -> bool {
        matches!(self, Self::Normal)
    }

    #[must_use]
    pub fn is_fullscreen(&self) -> bool {
        matches!(self, Self::Fullscreen)
    }

    #[must_use]
    pub fn is_maximized(&self) -> bool {
        matches!(self, Self::Maximized)
    }
}

impl<W: LayoutElement> InteractiveMoveState<W> {
    fn moving(&self) -> Option<&InteractiveMoveData<W>> {
        match self {
            InteractiveMoveState::Moving(move_) => Some(move_),
            _ => None,
        }
    }

    fn moving_mut(&mut self) -> Option<&mut InteractiveMoveData<W>> {
        match self {
            InteractiveMoveState::Moving(move_) => Some(move_),
            _ => None,
        }
    }
}

impl<W: LayoutElement> InteractiveMoveData<W> {
    fn tile_render_location(&self, zoom: f64) -> Point<f64, Logical> {
        let scale = Scale::from(self.output.current_scale().fractional_scale());
        let window_size = self.tile.window_size();
        let pointer_offset_within_window = Point::from((
            window_size.w * self.pointer_ratio_within_window.0,
            window_size.h * self.pointer_ratio_within_window.1,
        ));
        let pos = self.pointer_pos_within_output
            - (pointer_offset_within_window + self.tile.window_loc() - self.tile.render_offset())
                .upscale(zoom);
        // Round to physical pixels.
        pos.to_physical_precise_round(scale).to_logical(scale)
    }
}

impl ActivateWindow {
    pub fn map_smart(self, f: impl FnOnce() -> bool) -> bool {
        match self {
            ActivateWindow::Yes => true,
            ActivateWindow::Smart => f(),
            ActivateWindow::No => false,
        }
    }
}

impl HitType {
    pub fn offset_win_pos(mut self, offset: Point<f64, Logical>) -> Self {
        match &mut self {
            HitType::Input { win_pos } => *win_pos += offset,
            HitType::Activate { .. } => (),
        }
        self
    }

    pub fn hit_tile<W: LayoutElement>(
        tile: &Tile<W>,
        tile_pos: Point<f64, Logical>,
        point: Point<f64, Logical>,
    ) -> Option<(&W, Self)> {
        let pos_within_tile = point - tile_pos;
        tile.hit(pos_within_tile)
            .map(|hit| (tile.window(), hit.offset_win_pos(tile_pos)))
    }

    pub fn to_activate(self) -> Self {
        match self {
            HitType::Input { .. } => HitType::Activate {
                is_tab_indicator: false,
            },
            HitType::Activate { .. } => self,
        }
    }
}

impl Options {
    fn from_config(config: &Config) -> Self {
        Self {
            layout: config.layout.clone(),
            animations: config.animations.clone(),
            gestures: config.gestures,
            overview: config.overview,
            blur: config.blur,
            disable_resize_throttling: config.debug.disable_resize_throttling,
            disable_transactions: config.debug.disable_transactions,
            deactivate_unfocused_windows: config.debug.deactivate_unfocused_windows,
        }
    }

    fn with_merged_layout(mut self, part: Option<&swayward_config::LayoutPart>) -> Self {
        if let Some(part) = part {
            self.layout.merge_with(part);
        }
        self
    }

    fn adjusted_for_scale(mut self, scale: f64) -> Self {
        self.layout.gaps = round_logical_in_physical_max1(scale, self.layout.gaps);
        self.layout.outer_gaps.left = round_logical_in_physical(scale, self.layout.outer_gaps.left);
        self.layout.outer_gaps.right =
            round_logical_in_physical(scale, self.layout.outer_gaps.right);
        self.layout.outer_gaps.top = round_logical_in_physical(scale, self.layout.outer_gaps.top);
        self.layout.outer_gaps.bottom =
            round_logical_in_physical(scale, self.layout.outer_gaps.bottom);
        self
    }
}

impl OverviewProgress {
    fn value(&self) -> f64 {
        match self {
            OverviewProgress::Animation(anim) => anim.value(),
            OverviewProgress::Gesture(gesture) => gesture.value,
            OverviewProgress::Open => 1.,
        }
    }

    fn is_animation(&self) -> bool {
        matches!(self, OverviewProgress::Animation(_))
    }
}

impl RenderLayer {
    /// Returns `true` if the render layer is [`Normal`].
    ///
    /// [`Normal`]: RenderLayer::Normal
    #[must_use]
    pub fn is_normal(&self) -> bool {
        matches!(self, Self::Normal)
    }
}

fn parse_workspace_num(name: &str) -> Option<i32> {
    let end = name
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(name.len());
    (end > 0).then(|| name[..end].parse().ok()).flatten()
}

/// Whether `name` is matched by the `workspace number <target>` form.
///
/// Mirrors `_workspace_by_number` (sway/sway/tree/workspace.c:493-502): the
/// digits of `target` must equal the leading digits of `name`, and `name` must
/// not carry a further digit. So "1" matches "1" and "1:first" but not "11".
pub(crate) fn workspace_name_matches_number(name: &str, target: &str) -> bool {
    let mut name_chars = name.chars();
    for digit in target.chars().take_while(char::is_ascii_digit) {
        if name_chars.next() != Some(digit) {
            return false;
        }
    }
    !name_chars.next().is_some_and(|c| c.is_ascii_digit())
}

pub(crate) fn sway_workspace_num(name: &str) -> i32 {
    parse_workspace_num(name).unwrap_or(-1)
}

fn workspace_number_matches(workspace_name: &str, number: &str) -> bool {
    workspace_name
        .strip_prefix(number)
        .is_some_and(|suffix| !suffix.starts_with(|character: char| character.is_ascii_digit()))
}

fn workspace_matches_target<W: LayoutElement>(
    workspace: &Workspace<W>,
    target: &crate::command::WorkspaceTarget,
) -> bool {
    match target {
        // Match the digit prefix of the name, as sway's _workspace_by_number
        // does (sway/sway/tree/workspace.c:493-502), so `number 1` finds
        // "1:first". Comparing a stored number missed it, and `move ... to
        // workspace number` then created a second workspace instead.
        crate::command::WorkspaceTarget::Number(value) => {
            workspace.has_sway_identity()
                && workspace
                    .sway_name()
                    .is_some_and(|name| workspace_name_matches_number(&name, value))
        }
        crate::command::WorkspaceTarget::Name(value) => workspace
            .sway_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(value)),
        _ => false,
    }
}

fn sway_workspace_identity(
    target: crate::command::WorkspaceTarget,
) -> Result<(Option<String>, Option<i32>), String> {
    match target {
        crate::command::WorkspaceTarget::Number(name) => {
            let number = parse_workspace_num(&name)
                .ok_or_else(|| format!("invalid workspace number '{name}'"))?;
            Ok(((name != number.to_string()).then_some(name), Some(number)))
        }
        crate::command::WorkspaceTarget::Name(name) => {
            let number = parse_workspace_num(&name);
            Ok((
                (number.is_none() || name != number.unwrap().to_string()).then_some(name),
                number,
            ))
        }
        _ => Err("relative workspace target cannot be created".into()),
    }
}

fn initial_workspace_name_from_action(action: &swayward_config::Action) -> Option<String> {
    let target = match action {
        swayward_config::Action::SwayCommand(command) => {
            let parsed = crate::command::parse(command).into_iter().next()?.ok()?;
            let crate::command::Command::Workspace { target, .. } = parsed.command else {
                return None;
            };
            target
        }
        swayward_config::Action::FocusWorkspace(reference) => {
            return match reference {
                swayward_config::WorkspaceReference::Name(name) => Some(name.clone()),
                swayward_config::WorkspaceReference::Index(index) => Some(index.to_string()),
                swayward_config::WorkspaceReference::Id(_) => None,
            };
        }
        _ => return None,
    };
    match target {
        crate::command::WorkspaceTarget::Name(name) if !name.eq_ignore_ascii_case("number") => {
            Some(name)
        }
        crate::command::WorkspaceTarget::Number(name) => Some(name),
        _ => None,
    }
}

fn initial_workspace_names(config: &Config) -> Vec<String> {
    config
        .binds
        .0
        .iter()
        .filter_map(|bind| initial_workspace_name_from_action(&bind.action))
        .collect()
}

impl<W: LayoutElement> Layout<W> {
    pub fn new(clock: Clock, config: &Config) -> Self {
        let initial_workspace_names = initial_workspace_names(config);
        Self::with_options_and_workspaces(
            clock,
            config,
            Options::from_config(config),
            initial_workspace_names,
        )
    }

    pub fn with_options(clock: Clock, options: Options) -> Self {
        Self {
            monitor_set: MonitorSet::NoOutputs { workspaces: vec![] },
            is_active: true,
            last_active_workspace_id: HashMap::new(),
            scratchpad: VecDeque::new(),
            scratchpad_windows: Vec::new(),
            interactive_move: None,
            dnd: None,
            clock,
            update_render_elements_time: Duration::ZERO,
            overview_open: false,
            overview_progress: None,
            options: Rc::new(options),
            initial_workspace_names: Vec::new(),
            workspace_configs: Vec::new(),
        }
    }

    fn with_options_and_workspaces(
        clock: Clock,
        config: &Config,
        options: Options,
        initial_workspace_names: Vec<String>,
    ) -> Self {
        let opts = Rc::new(options);

        let workspaces = config
            .workspaces
            .iter()
            .filter(|workspace| workspace.sway_output_assignment.is_none())
            .map(|workspace| {
                Workspace::new_with_config_no_outputs(
                    Some(workspace.clone()),
                    clock.clone(),
                    opts.clone(),
                )
            })
            .collect();

        Self {
            monitor_set: MonitorSet::NoOutputs { workspaces },
            is_active: true,
            last_active_workspace_id: HashMap::new(),
            scratchpad: VecDeque::new(),
            scratchpad_windows: Vec::new(),
            interactive_move: None,
            dnd: None,
            clock,
            update_render_elements_time: Duration::ZERO,
            overview_open: false,
            overview_progress: None,
            options: opts,
            initial_workspace_names,
            workspace_configs: config.workspaces.clone(),
        }
    }

    fn next_free_workspace_identity(&self) -> (Option<String>, Option<i32>) {
        self.next_free_workspace_identity_for_output(None)
    }

    fn next_free_workspace_identity_for_output(
        &self,
        output: Option<&Output>,
    ) -> (Option<String>, Option<i32>) {
        let mut used = self
            .workspaces()
            .filter_map(|(_, _, workspace)| workspace.sway_name())
            .filter_map(|name| parse_workspace_num(&name))
            .filter(|number| *number > 0)
            .collect::<HashSet<_>>();
        // Also skip a number that an assignment claims for a DIFFERENT output.
        // Sway's fallback loop rejects a candidate while workspace_by_number
        // finds it (sway/sway/tree/workspace.c:484-490), and such a name is
        // reserved for the output its assignment names, so handing it to this
        // output would take a name that is not free.
        if let Some(output) = output {
            for config in &self.workspace_configs {
                if Self::workspace_assignment(config).is_none() {
                    continue;
                }
                if self.workspace_assigned_to_output(&config.name.0, output) {
                    continue;
                }
                // Only a name whose assignment actually RESOLVES is reserved.
                // An assignment naming solely absent outputs claims nothing, so
                // its number stays free: sway's workspace_by_number test only
                // rejects a number some existing workspace holds, and such a
                // workspace is never created.
                let resolves = Self::workspace_assignment(config)
                    .into_iter()
                    .flatten()
                    .any(|name| {
                        self.monitors()
                            .any(|monitor| output_matches_name(monitor.output(), &name))
                    });
                if !resolves {
                    continue;
                }
                if let Some(number) = parse_workspace_num(&config.name.0) {
                    used.insert(number);
                }
            }
        }
        let number = (1..).find(|number| !used.contains(number)).unwrap();
        (None, Some(number))
    }

    /// The name a newly enabled output should give its first workspace.
    ///
    /// Mirrors `workspace_next_name` (sway/sway/tree/workspace.c:436-490).
    /// Names from bindings come first, then `workspace <name> output <output>`
    /// assignments. An assignment is skipped when a workspace of that name
    /// already exists. Within one assignment sway walks the listed outputs and
    /// `break`s at the FIRST one that resolves, claiming the name only if that
    /// output is this one; an output name that resolves to nothing does not
    /// break, so the search continues. An assignment naming only absent outputs
    /// therefore claims nothing, and its workspace falls back to a free number.
    /// Whether `name` may be used as `output`'s first workspace name.
    ///
    /// Mirrors `workspace_valid_on_output` (sway/sway/tree/workspace.c:334-354).
    /// A name with no assignment is valid on any output. Otherwise the first
    /// output in its assignment that RESOLVES decides: the name is valid only
    /// on that output. An assignment naming only absent outputs is valid
    /// nowhere.
    fn workspace_valid_on_output(&self, name: &str, output: &Output) -> bool {
        let Some(config) = self
            .workspace_configs
            .iter()
            .find(|config| config.name.0.eq_ignore_ascii_case(name))
        else {
            return true;
        };
        if Self::workspace_assignment(config).is_none() {
            return true;
        }
        self.workspace_assigned_to_output(name, output)
    }

    /// The ordered output list a workspace config assigns, under either
    /// spelling: `sway-output-assignment` carries a list, while a single
    /// `open-on-output` is what the translator emits for a sway
    /// `workspace <name> output <output>`.
    fn workspace_assignment(config: &WorkspaceConfig) -> Option<Vec<String>> {
        config.sway_output_assignment.clone().or_else(|| {
            config
                .open_on_output
                .as_ref()
                .map(|output| vec![output.clone()])
        })
    }

    /// Whether `name`'s assignment claims `output`.
    ///
    /// Sway breaks at the first output in the list that RESOLVES and claims the
    /// name only if that output is this one (sway/sway/tree/workspace.c:
    /// 465-475), so an assignment naming only absent outputs claims nothing.
    fn workspace_assigned_to_output(&self, name: &str, output: &Output) -> bool {
        let Some(config) = self
            .workspace_configs
            .iter()
            .find(|config| config.name.0.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        let Some(outputs) = Self::workspace_assignment(config) else {
            return false;
        };
        // `output` may not be in the monitor list yet, since add_output resolves
        // the name before inserting it, so resolve against the monitors PLUS
        // this output. Sway's output_by_name_or_id sees the output because
        // output_enable adds it first (sway/sway/tree/output.c:161-166).
        outputs
            .iter()
            .find(|name| {
                output_matches_name(output, name)
                    || self
                        .monitors()
                        .any(|monitor| output_matches_name(monitor.output(), name))
            })
            .is_some_and(|name| output_matches_name(output, name))
    }

    fn next_initial_workspace_name_for_output(&self, output: Option<&Output>) -> Option<String> {
        let existing_names = self
            .workspaces()
            .filter_map(|(_, _, workspace)| workspace.sway_name())
            .collect::<Vec<_>>();
        let unused = |name: &str| {
            !existing_names
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(name))
        };

        let output = output?;

        // Sway takes "assignments primarily, falling back to bindings and
        // numbers" (sway/sway/tree/workspace.c:440): both loops run and the
        // ASSIGNMENT loop overwrites whatever a binding chose, so an assignment
        // naming this output wins.
        if let Some(name) = self
            .workspace_configs
            .iter()
            .filter(|config| unused(&config.name.0))
            .find(|config| self.workspace_assigned_to_output(&config.name.0, output))
            .map(|config| config.name.0.clone())
        {
            return Some(name);
        }

        // Then a binding name, but only on an output it is VALID on:
        // workspace_valid_on_output (sway/sway/tree/workspace.c:334-354)
        // requires a name that HAS an assignment to match that assignment's
        // first resolvable output. A name without one is valid anywhere.
        self.initial_workspace_names
            .iter()
            .find(|name| unused(name) && self.workspace_valid_on_output(name, output))
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn initialize_workspaces_from_bindings(&mut self, config: &Config) {
        self.initial_workspace_names = initial_workspace_names(config);
        // Also take the workspace configs, so a `workspace <name> output
        // <output>` assignment is visible while startup names are resolved.
        // Without this the assignment list was empty here and every output fell
        // through to a binding name or a bare number.
        self.workspace_configs = config.workspaces.clone();
        for monitor in self.monitors_mut() {
            for workspace in &mut monitor.workspaces {
                if !workspace.is_persistent() && !workspace.has_windows() {
                    workspace.unname();
                }
            }
        }
        // Pair each unnamed workspace with its own output, so an assignment
        // like `workspace special output fake-0` is resolved against the output
        // the workspace actually sits on, the way sway's workspace_next_name
        // takes the output name. Carry on rather than breaking: an output with
        // no claimable name must not stop a later output from taking its own.
        let available = self
            .workspaces()
            .filter(|(_, _, workspace)| !workspace.has_sway_identity() && !workspace.has_windows())
            .map(|(monitor, _, workspace)| {
                (
                    workspace.id(),
                    monitor.map(|monitor| monitor.output().clone()),
                )
            })
            .collect::<Vec<_>>();
        for (id, output) in available {
            // Fall back to the next free number when no name claims this
            // output, as sway's workspace_next_name does
            // (sway/sway/tree/workspace.c:484-490). Skipping the output left it
            // with the index-derived identity the workspace model forbids.
            let (name, number) = match self.next_initial_workspace_name_for_output(output.as_ref())
            {
                Some(name) => {
                    sway_workspace_identity(crate::command::WorkspaceTarget::Name(name)).unwrap()
                }
                None => self.next_free_workspace_identity_for_output(output.as_ref()),
            };
            self.workspaces_mut()
                .find(|workspace| workspace.id() == id)
                .unwrap()
                .set_sway_identity(name, number);
        }
    }

    pub fn add_output(&mut self, output: Output, layout_config: Option<LayoutPart>) {
        // Consult this output's assignments, as sway does: workspace_next_name
        // takes the output name (sway/sway/tree/workspace.c:436) and is called
        // from output_enable (sway/sway/tree/output.c:161-166).
        let initial_workspace_name = self.next_initial_workspace_name_for_output(Some(&output));
        let initial_workspace_number = initial_workspace_name
            .is_none()
            .then(|| {
                self.next_free_workspace_identity_for_output(Some(&output))
                    .1
            })
            .flatten();
        self.monitor_set = match mem::take(&mut self.monitor_set) {
            MonitorSet::Normal {
                mut monitors,
                primary_idx,
                active_monitor_idx,
            } => {
                let primary = &mut monitors[primary_idx];

                let mut stopped_primary_ws_switch = false;

                // Only the primary gives workspaces away here. Repair it
                // afterwards if it did: restoring unconditionally resurrects
                // workspaces other outputs deliberately discarded.
                let mut reclaimed_any = false;
                let mut workspaces = vec![];
                for i in (0..primary.workspaces.len()).rev() {
                    if primary.workspaces[i].original_output.matches(&output) {
                        let ws = primary.workspaces.remove(i);
                        reclaimed_any = true;

                        // FIXME: this can be coded in a way that the workspace switch won't be
                        // affected if the removed workspace is invisible. But this is good enough
                        // for now.
                        if primary.workspace_switch.is_some() {
                            primary.workspace_switch = None;
                            stopped_primary_ws_switch = true;
                        }

                        // The user could've closed a window while remaining on this workspace, on
                        // another monitor. However, we will add an empty workspace in the end
                        // instead.
                        if ws.must_be_kept() {
                            workspaces.push(ws);
                        }

                        if i <= primary.active_workspace_idx
                            // Generally when moving the currently active workspace, we want to
                            // fall back to the workspace above, so as not to end up on the last
                            // empty workspace. However, with empty workspace above first, when
                            // moving the workspace at index 1 (first non-empty), we want to stay
                            // at index 1, so as once again not to end up on an empty workspace.
                            //
                            // This comes into play at compositor startup when having named
                            // workspaces set up across multiple monitors. Without this check, the
                            // first monitor to connect can end up with the first empty workspace
                            // focused instead of the first named workspace.
                            && !(false
                                && primary.active_workspace_idx == 1)
                        {
                            primary.active_workspace_idx =
                                primary.active_workspace_idx.saturating_sub(1);
                        }
                    }
                }

                // If we stopped a workspace switch, then we might need to clean up workspaces.
                if stopped_primary_ws_switch || (false && primary.workspaces.len() == 2) {
                    primary.clean_up_workspaces();
                }

                workspaces.reverse();

                let ws_id_to_activate = self.last_active_workspace_id.remove(&output.name());

                let mut monitor = Monitor::new(
                    output,
                    workspaces,
                    ws_id_to_activate,
                    initial_workspace_name.clone(),
                    initial_workspace_number,
                    self.clock.clone(),
                    self.options.clone(),
                    layout_config,
                );
                monitor.overview_open = self.overview_open;
                monitor.set_overview_progress(self.overview_progress.as_ref());
                // Monitor::new adopts workspaces reclaimed from the primary
                // monitor, which can include one holding windows, so the new
                // monitor need not end in an empty placeholder.
                monitor.reap_empty_workspaces();
                monitors.push(monitor);
                // Reclaiming workspaces mutates the monitor they came from too,
                // and sorting can leave it ending in an addressable workspace.
                // Only repair monitors that actually lost one: restoring
                // unconditionally resurrects workspaces other outputs
                // deliberately discarded.
                if reclaimed_any {
                    monitors[primary_idx].reap_empty_workspaces();
                }

                MonitorSet::Normal {
                    monitors,
                    primary_idx,
                    active_monitor_idx,
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                let ws_id_to_activate = self.last_active_workspace_id.remove(&output.name());

                let mut monitor = Monitor::new(
                    output,
                    workspaces,
                    ws_id_to_activate,
                    initial_workspace_name,
                    initial_workspace_number,
                    self.clock.clone(),
                    self.options.clone(),
                    layout_config,
                );
                monitor.overview_open = self.overview_open;
                monitor.set_overview_progress(self.overview_progress.as_ref());

                MonitorSet::Normal {
                    monitors: vec![monitor],
                    primary_idx: 0,
                    active_monitor_idx: 0,
                }
            }
        }
    }

    pub fn remove_output(&mut self, output: &Output) {
        self.monitor_set = match mem::take(&mut self.monitor_set) {
            MonitorSet::Normal {
                mut monitors,
                mut primary_idx,
                mut active_monitor_idx,
            } => {
                let idx = monitors
                    .iter()
                    .position(|mon| &mon.output == output)
                    .expect("trying to remove non-existing output");
                let monitor = monitors.remove(idx);

                self.last_active_workspace_id.insert(
                    monitor.output_name().clone(),
                    monitor.workspaces[monitor.active_workspace_idx].id(),
                );

                let mut workspaces = monitor.into_workspaces();

                if monitors.is_empty() {
                    // Removed the last monitor.

                    for ws in &mut workspaces {
                        // Reset base options to layout ones.
                        ws.update_config(self.options.clone());
                    }

                    MonitorSet::NoOutputs { workspaces }
                } else {
                    if primary_idx >= idx {
                        // Update primary_idx to either still point at the same monitor, or at some
                        // other monitor if the primary has been removed.
                        primary_idx = primary_idx.saturating_sub(1);
                    }
                    if active_monitor_idx >= idx {
                        // Update active_monitor_idx to either still point at the same monitor, or
                        // at some other monitor if the active monitor has
                        // been removed.
                        active_monitor_idx = active_monitor_idx.saturating_sub(1);
                    }

                    let primary = &mut monitors[primary_idx];
                    let mut sticky = Vec::new();
                    workspaces.retain_mut(|workspace| {
                        if workspace.has_non_sticky_windows() {
                            true
                        } else {
                            sticky.extend(workspace.take_sticky_tiles());
                            false
                        }
                    });
                    let target_workspace = primary.active_workspace();
                    for mut removed in sticky {
                        target_workspace.remap_floating_position(
                            &mut removed.tile,
                            removed.floating_working_area,
                        );
                        target_workspace.add_tile(
                            removed.tile,
                            WorkspaceAddWindowTarget::Auto,
                            ActivateWindow::No,
                            removed.width,
                            removed.is_full_width,
                            true,
                            None,
                        );
                    }
                    primary.append_workspaces(workspaces);

                    MonitorSet::Normal {
                        monitors,
                        primary_idx,
                        active_monitor_idx,
                    }
                }
            }
            MonitorSet::NoOutputs { .. } => {
                panic!("tried to remove output when there were already none")
            }
        }
    }

    pub fn add_tiling_tile_by_idx(
        &mut self,
        monitor_idx: usize,
        workspace_idx: usize,
        tile: Tile<W>,
        activate: bool,
    ) {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            panic!()
        };

        monitors[monitor_idx].add_tiling_tile(workspace_idx, tile, activate);

        if activate {
            *active_monitor_idx = monitor_idx;
        }
    }

    /// Adds a new window to the layout.
    ///
    /// Returns an output that the window was added to, if there were any outputs.
    #[allow(clippy::too_many_arguments)]
    pub fn add_window(
        &mut self,
        window: W,
        mut target: AddWindowTarget<W>,
        width: Option<PresetSize>,
        height: Option<PresetSize>,
        is_full_width: bool,
        is_floating: bool,
        activate: ActivateWindow,
    ) -> Option<&Output> {
        let scrolling_height = height.map(SizeChange::from);
        let id = window.id().clone();
        if matches!(target, AddWindowTarget::NextTo(parent) if self.is_scratchpad_hidden(parent)) {
            target = AddWindowTarget::Auto;
        }

        match &mut self.monitor_set {
            MonitorSet::Normal {
                monitors,
                active_monitor_idx,
                ..
            } => {
                let (mon_idx, target) = match target {
                    AddWindowTarget::Auto => (*active_monitor_idx, MonitorAddWindowTarget::Auto),
                    AddWindowTarget::Output(output) => {
                        let mon_idx = monitors
                            .iter()
                            .position(|mon| mon.output == *output)
                            .unwrap();

                        (mon_idx, MonitorAddWindowTarget::Auto)
                    }
                    AddWindowTarget::Workspace(ws_id) => {
                        let mon_idx = monitors.iter().position(|mon| mon.has_ws(ws_id)).unwrap();

                        (
                            mon_idx,
                            MonitorAddWindowTarget::Workspace {
                                id: ws_id,
                                column_idx: None,
                            },
                        )
                    }
                    AddWindowTarget::NextTo(next_to) => {
                        if let Some(output) = self
                            .interactive_move
                            .as_ref()
                            .and_then(|move_| {
                                if let InteractiveMoveState::Moving(move_) = move_ {
                                    Some(move_)
                                } else {
                                    None
                                }
                            })
                            .filter(|move_| next_to == move_.tile.window().id())
                            .map(|move_| move_.output.clone())
                        {
                            // The next_to window is being interactively moved.
                            let mon_idx = monitors
                                .iter()
                                .position(|mon| mon.output == output)
                                .unwrap_or(*active_monitor_idx);

                            (mon_idx, MonitorAddWindowTarget::Auto)
                        } else {
                            let mon_idx = monitors
                                .iter()
                                .position(|mon| {
                                    mon.workspaces.iter().any(|ws| ws.has_window(next_to))
                                })
                                .unwrap();
                            (mon_idx, MonitorAddWindowTarget::NextTo(next_to))
                        }
                    }
                };
                let mon = &mut monitors[mon_idx];

                let (ws_idx, _) = mon.resolve_add_window_target(target);
                let ws = &mon.workspaces[ws_idx];
                let scrolling_width = ws.resolve_scrolling_width(&window, width);

                mon.add_window(
                    window,
                    target,
                    activate,
                    scrolling_width,
                    is_full_width,
                    is_floating,
                );

                if activate.map_smart(|| false) {
                    *active_monitor_idx = mon_idx;
                }

                // Set the default height for scrolling windows.
                if !is_floating {
                    if let Some(change) = scrolling_height {
                        let ws = mon
                            .workspaces
                            .iter_mut()
                            .find(|ws| ws.has_window(&id))
                            .unwrap();
                        ws.set_window_height(
                            Some(&id),
                            change,
                            output_size(&mon.output).to_i32_round(),
                        );
                    }
                }

                Some(&mon.output)
            }
            MonitorSet::NoOutputs { workspaces } => {
                let (ws_idx, target) = match target {
                    AddWindowTarget::Auto => {
                        if workspaces.is_empty() {
                            workspaces.push(Workspace::new_no_outputs(
                                self.clock.clone(),
                                self.options.clone(),
                            ));
                        }

                        (0, WorkspaceAddWindowTarget::Auto)
                    }
                    AddWindowTarget::Output(_) => panic!(),
                    AddWindowTarget::Workspace(ws_id) => {
                        let ws_idx = workspaces.iter().position(|ws| ws.id() == ws_id).unwrap();
                        (ws_idx, WorkspaceAddWindowTarget::Auto)
                    }
                    AddWindowTarget::NextTo(next_to) => {
                        if self
                            .interactive_move
                            .as_ref()
                            .and_then(|move_| {
                                if let InteractiveMoveState::Moving(move_) = move_ {
                                    Some(move_)
                                } else {
                                    None
                                }
                            })
                            .filter(|move_| next_to == move_.tile.window().id())
                            .is_some()
                        {
                            // The next_to window is being interactively moved. If there are no
                            // other windows, we may have no workspaces at all.
                            if workspaces.is_empty() {
                                workspaces.push(Workspace::new_no_outputs(
                                    self.clock.clone(),
                                    self.options.clone(),
                                ));
                            }

                            (0, WorkspaceAddWindowTarget::Auto)
                        } else {
                            let ws_idx = workspaces
                                .iter()
                                .position(|ws| ws.has_window(next_to))
                                .unwrap();
                            (ws_idx, WorkspaceAddWindowTarget::NextTo(next_to))
                        }
                    }
                };
                let ws = &mut workspaces[ws_idx];

                let scrolling_width = ws.resolve_scrolling_width(&window, width);

                let tile = ws.make_tile(window);
                ws.add_tile(
                    tile,
                    target,
                    activate,
                    scrolling_width,
                    is_full_width,
                    is_floating,
                    None,
                );

                // Set the default height for scrolling windows.
                if !is_floating {
                    if let Some(change) = scrolling_height {
                        ws.set_window_height(Some(&id), change, Size::from((1280, 720)));
                    }
                }

                None
            }
        }
    }

    pub fn remove_window(
        &mut self,
        window: &W::Id,
        transaction: Transaction,
    ) -> Option<RemovedTile<W>> {
        let (removed, source_workspace) = self.detach_window(window, transaction)?;
        if let Some(source_workspace) = source_workspace {
            self.clean_up_removed_window_workspace(source_workspace);
        }
        Some(removed)
    }

    fn detach_window(
        &mut self,
        window: &W::Id,
        transaction: Transaction,
    ) -> Option<(RemovedTile<W>, Option<WorkspaceId>)> {
        if let Some(index) = self
            .scratchpad
            .iter()
            .position(|removed| removed.tile.window().id() == window)
        {
            self.scratchpad_windows.retain(|id| id != window);
            return self.scratchpad.remove(index).map(|removed| (removed, None));
        }

        if let Some(state) = &self.interactive_move {
            match state {
                InteractiveMoveState::Starting { window_id, .. } => {
                    if window_id == window {
                        self.interactive_move_end(window);
                    }
                }
                InteractiveMoveState::Moving(move_) => {
                    if move_.tile.window().id() == window {
                        let Some(InteractiveMoveState::Moving(move_)) =
                            self.interactive_move.take()
                        else {
                            unreachable!()
                        };

                        for mon in self.monitors_mut() {
                            mon.dnd_scroll_gesture_end();
                        }

                        // Unlock the view on the workspaces.
                        for ws in self.workspaces_mut() {
                            ws.dnd_scroll_gesture_end();
                        }

                        return Some((
                            RemovedTile {
                                tile: move_.tile,
                                width: move_.width,
                                is_full_width: move_.is_full_width,
                                is_floating: false,
                                floating_working_area: None,
                            },
                            None,
                        ));
                    }
                }
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    if let Some(ws) = mon
                        .workspaces
                        .iter_mut()
                        .find(|workspace| workspace.has_window(window))
                    {
                        let source_workspace = ws.id();
                        let removed = ws.remove_tile(window, transaction);
                        return Some((removed, Some(source_workspace)));
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                if let Some(idx) = workspaces
                    .iter()
                    .position(|workspace| workspace.has_window(window))
                {
                    let removed = workspaces[idx].remove_tile(window, transaction);
                    if !workspaces[idx].must_be_kept() {
                        workspaces.remove(idx);
                    }
                    return Some((removed, None));
                }
            }
        }

        None
    }

    fn clean_up_removed_window_workspace(&mut self, source_workspace: WorkspaceId) {
        let Some(monitor) = self
            .monitors_mut()
            .find(|monitor| monitor.has_ws(source_workspace))
        else {
            return;
        };
        monitor.workspace_switch = None;
        monitor.consider_destroy_workspace(source_workspace);
        if monitor
            .workspaces
            .iter()
            .filter(|workspace| !workspace.has_windows())
            .count()
            > 1
        {
            if let Some(previous) = monitor.previous_workspace_id() {
                monitor.consider_destroy_workspace(previous);
            }
        }
        monitor.reap_empty_workspaces();
    }

    pub fn descendants_added(&mut self, id: &W::Id) -> bool {
        for ws in self.workspaces_mut() {
            if ws.descendants_added(id) {
                return true;
            }
        }

        false
    }

    pub fn update_window(&mut self, window: &W::Id, serial: Option<Serial>) {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if move_.tile.window().id() == window {
                // Do this before calling update_window() so it can get up-to-date info.
                if let Some(serial) = serial {
                    move_.tile.window_mut().on_commit(serial);
                }

                move_.tile.update_window();
                return;
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        if ws.has_window(window) {
                            ws.update_window(window, serial);
                            return;
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    if ws.has_window(window) {
                        ws.update_window(window, serial);
                        return;
                    }
                }
            }
        }
    }

    pub fn workspace_id_at(&self, output: &Output, index: usize) -> Option<WorkspaceId> {
        self.monitor_for_output(output)
            .and_then(|monitor| monitor.workspaces.get(index))
            .map(Workspace::id)
    }

    pub fn find_workspace_by_id(&self, id: WorkspaceId) -> Option<(usize, &Workspace<W>)> {
        match &self.monitor_set {
            MonitorSet::Normal { ref monitors, .. } => {
                for mon in monitors {
                    if let Some(index) = mon.idx_of_ws(id) {
                        let workspace = &mon.workspaces[index];
                        return Some((index, workspace));
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                if let Some((index, workspace)) =
                    workspaces.iter().enumerate().find(|(_, w)| w.id() == id)
                {
                    return Some((index, workspace));
                }
            }
        }

        None
    }

    pub fn find_workspace_by_number(&self, number: &str) -> Option<(usize, &Workspace<W>)> {
        // Prefer an explicitly named workspace over the unnamed placeholder.
        // Both answer to `number 2`, since the placeholder reports a name like
        // "2", but only the named one is the workspace sway would still have,
        // so taking the first match sent an assigned window to the placeholder.
        let mut candidates = self
            .workspaces()
            .filter(|(_, _, workspace)| {
                workspace
                    .sway_name()
                    .is_some_and(|name| workspace_number_matches(&name, number))
            })
            .map(|(_, index, workspace)| (index, workspace))
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, workspace)| workspace.name().is_none());
        candidates.into_iter().next()
    }

    pub fn find_workspace_by_name(&self, workspace_name: &str) -> Option<(usize, &Workspace<W>)> {
        match &self.monitor_set {
            MonitorSet::Normal { ref monitors, .. } => {
                for mon in monitors {
                    if let Some((index, workspace)) =
                        mon.workspaces.iter().enumerate().find(|(_, workspace)| {
                            workspace
                                .sway_name()
                                .is_some_and(|name| name.eq_ignore_ascii_case(workspace_name))
                        })
                    {
                        return Some((index, workspace));
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                if let Some((index, workspace)) =
                    workspaces.iter().enumerate().find(|(_, workspace)| {
                        workspace
                            .sway_name()
                            .is_some_and(|name| name.eq_ignore_ascii_case(workspace_name))
                    })
                {
                    return Some((index, workspace));
                }
            }
        }

        None
    }

    pub fn ensure_sway_workspace(&mut self, workspace_name: &str) {
        if self.find_workspace_by_name(workspace_name).is_some() {
            return;
        }
        let (name, number) = sway_workspace_identity(crate::command::WorkspaceTarget::Name(
            workspace_name.to_owned(),
        ))
        .unwrap();
        if let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        {
            let monitor = &mut monitors[*active_monitor_idx];
            let index = monitor.workspaces.len().saturating_sub(1);
            monitor.add_sway_workspace_at(index, name, number);
            // Sway sorts an output's workspaces on every creation:
            // workspace_create calls output_sort_workspaces
            // (sway/sway/tree/workspace.c:259), which puts numeric names in
            // numeric order and ahead of non-numeric ones
            // (sway/sway/tree/output.c:387-405). Appending without sorting left
            // GET_WORKSPACES in creation order.
            monitor.sort_sway_workspaces();
        }
    }

    pub fn find_workspace_by_ref(
        &mut self,
        reference: WorkspaceReference,
    ) -> Option<&mut Workspace<W>> {
        if let WorkspaceReference::Index(index) = reference {
            self.active_monitor().and_then(|m| {
                let index = index.saturating_sub(1) as usize;
                m.workspaces.get_mut(index)
            })
        } else {
            self.workspaces_mut().find(|ws| match &reference {
                WorkspaceReference::Name(ref_name) => ws
                    .name
                    .as_ref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(ref_name)),
                WorkspaceReference::Id(id) => ws.id().get() == *id,
                WorkspaceReference::Index(_) => unreachable!(),
            })
        }
    }

    pub fn unname_workspace(&mut self, workspace_name: &str) {
        self.unname_workspace_by_ref(WorkspaceReference::Name(workspace_name.into()));
    }

    pub fn unname_workspace_by_ref(&mut self, reference: WorkspaceReference) {
        let id = self.find_workspace_by_ref(reference).map(|ws| ws.id());
        if let Some(id) = id {
            self.unname_workspace_by_id(id);
        }
    }

    pub fn unname_workspace_by_id(&mut self, id: WorkspaceId) {
        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    if mon.unname_workspace(id) {
                        return;
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                for (idx, ws) in workspaces.iter_mut().enumerate() {
                    if ws.id() == id {
                        ws.unname();

                        // Clean up empty workspaces.
                        if !ws.has_windows() {
                            workspaces.remove(idx);
                        }

                        return;
                    }
                }
            }
        }
    }

    pub fn find_window_and_output(&self, wl_surface: &WlSurface) -> Option<(&W, Option<&Output>)> {
        if let Some(window) = self
            .scratchpad
            .iter()
            .map(|removed| removed.tile.window())
            .find(|window| window.is_wl_surface(wl_surface))
        {
            return Some((window, None));
        }

        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().is_wl_surface(wl_surface) {
                return Some((move_.tile.window(), Some(&move_.output)));
            }
        }

        match &self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mon.workspaces {
                        if let Some(window) = ws.find_wl_surface(wl_surface) {
                            return Some((window, Some(&mon.output)));
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                for ws in workspaces {
                    if let Some(window) = ws.find_wl_surface(wl_surface) {
                        return Some((window, None));
                    }
                }
            }
        }

        None
    }

    pub fn find_window_and_output_mut(
        &mut self,
        wl_surface: &WlSurface,
    ) -> Option<(&mut W, Option<&Output>)> {
        if let Some(window) = self
            .scratchpad
            .iter_mut()
            .map(|removed| removed.tile.window_mut())
            .find(|window| window.is_wl_surface(wl_surface))
        {
            return Some((window, None));
        }

        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if move_.tile.window().is_wl_surface(wl_surface) {
                return Some((move_.tile.window_mut(), Some(&move_.output)));
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        if let Some(window) = ws.find_wl_surface_mut(wl_surface) {
                            return Some((window, Some(&mon.output)));
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                for ws in workspaces {
                    if let Some(window) = ws.find_wl_surface_mut(wl_surface) {
                        return Some((window, None));
                    }
                }
            }
        }

        None
    }

    /// Computes the window-geometry-relative target rect for popup unconstraining.
    ///
    /// We will try to fit popups inside this rect.
    pub fn popup_target_rect(&self, window: &W::Id) -> Rectangle<f64, Logical> {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                // Follow the scrolling layout logic and fit the popup horizontally within the
                // window geometry.
                let width = move_.tile.window_size().w;
                let height = output_size(&move_.output).h;
                let mut target = Rectangle::from_size(Size::from((width, height)));
                // FIXME: ideally this shouldn't include the tile render offset, but the code
                // duplication would be a bit annoying for this edge case.
                target.loc.y -= move_.tile_render_location(1.).y;
                target.loc.y -= move_.tile.window_loc().y;
                return target;
            }
        }

        self.workspaces()
            .find_map(|(_, _, ws)| ws.popup_target_rect(window))
            .unwrap()
    }

    pub fn update_output_size(&mut self, output: &Output) {
        let _span = tracy_client::span!("Layout::update_output_size");

        let Some(mon) = self.monitor_for_output_mut(output) else {
            error!("monitor missing in update_output_size()");
            return;
        };

        mon.update_output_size();
    }

    pub fn scroll_amount_to_activate(&self, window: &W::Id) -> f64 {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return 0.;
            }
        }

        for mon in self.monitors() {
            for ws in &mon.workspaces {
                if ws.has_window(window) {
                    return ws.scroll_amount_to_activate(window);
                }
            }
        }

        0.
    }

    pub fn scroll_tab_indicator(&mut self, window: &W::Id, steps: i32) -> Option<W::Id> {
        self.workspaces_mut()
            .find(|workspace| workspace.has_window(window))?
            .scroll_tab_indicator(window, steps)
    }

    pub fn tab_indicator_focus_target(&self, window: &W::Id) -> Option<&W> {
        self.workspaces()
            .find_map(|(_, _, workspace)| workspace.tab_indicator_focus_target(window))
    }

    pub fn should_trigger_focus_follows_mouse_on(&self, window: &W::Id) -> bool {
        // During an animation, it's easy to trigger focus-follows-mouse on the previous workspace,
        // especially when clicking to switch workspace on a bar of some kind. This cancels the
        // workspace switch, which is annoying and not intended.
        //
        // This function allows focus-follows-mouse to trigger only on the animation target
        // workspace.
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return true;
            }
        }

        let MonitorSet::Normal { monitors, .. } = &self.monitor_set else {
            return true;
        };

        let (mon, ws_idx) = monitors
            .iter()
            .find_map(|mon| {
                mon.workspaces
                    .iter()
                    .position(|ws| ws.has_window(window))
                    .map(|ws_idx| (mon, ws_idx))
            })
            .unwrap();

        ws_idx == mon.active_workspace_idx
    }

    pub fn activate_window(&mut self, window: &W::Id) {
        let global = self.workspaces().find_map(|(_, _, workspace)| {
            (workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global)).then(|| {
                (
                    workspace.id(),
                    workspace.tiling().fullscreen_node().unwrap(),
                )
            })
        });
        if let Some((workspace_id, node)) = global {
            let target_is_inside = self.workspaces().any(|(_, _, candidate)| {
                candidate.id() == workspace_id && candidate.has_window(window)
            });
            if !target_is_inside {
                if let Some(workspace) = self
                    .workspaces_mut()
                    .find(|candidate| candidate.id() == workspace_id)
                {
                    workspace.tiling_mut().set_node_fullscreen(node, None);
                }
            }
        }
        let obstructing = self
            .workspaces()
            .find(|(_, _, workspace)| workspace.has_window(window))
            .and_then(|(_, _, workspace)| {
                let fullscreen = workspace.tiling().fullscreen_node()?;
                let target = workspace
                    .tiling()
                    .windows()
                    .find_map(|(id, candidate)| (candidate.id() == window).then_some(id))?;
                (!workspace.tiling().contains_node(fullscreen, target))
                    .then_some((workspace.id(), fullscreen))
            });
        if let Some((workspace_id, fullscreen)) = obstructing {
            if let Some(workspace) = self
                .workspaces_mut()
                .find(|candidate| candidate.id() == workspace_id)
            {
                workspace.tiling_mut().set_node_fullscreen(fullscreen, None);
            }
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return;
            }
        }

        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return;
        };

        for (monitor_idx, mon) in monitors.iter_mut().enumerate() {
            for (workspace_idx, ws) in mon.workspaces.iter_mut().enumerate() {
                if ws.activate_window(window) {
                    *active_monitor_idx = monitor_idx;
                    mon.switch_workspace(workspace_idx);
                    return;
                }
            }
        }
    }

    pub fn activate_window_without_raising(&mut self, window: &W::Id) {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return;
            }
        }

        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return;
        };

        for (monitor_idx, mon) in monitors.iter_mut().enumerate() {
            for (workspace_idx, ws) in mon.workspaces.iter_mut().enumerate() {
                if ws.activate_window_without_raising(window) {
                    *active_monitor_idx = monitor_idx;
                    mon.switch_workspace(workspace_idx);
                    return;
                }
            }
        }
    }

    pub fn active_output(&self) -> Option<&Output> {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &self.monitor_set
        else {
            return None;
        };

        Some(&monitors[*active_monitor_idx].output)
    }

    pub fn active_workspace(&self) -> Option<&Workspace<W>> {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &self.monitor_set
        else {
            return None;
        };

        let mon = &monitors[*active_monitor_idx];
        Some(&mon.workspaces[mon.active_workspace_idx])
    }

    pub fn active_workspace_mut(&mut self) -> Option<&mut Workspace<W>> {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return None;
        };

        let mon = &mut monitors[*active_monitor_idx];
        Some(&mut mon.workspaces[mon.active_workspace_idx])
    }

    pub fn windows_for_output(&self, output: &Output) -> impl Iterator<Item = &W> + '_ {
        let MonitorSet::Normal { monitors, .. } = &self.monitor_set else {
            panic!()
        };

        let moving_window = self
            .interactive_move
            .as_ref()
            .and_then(|x| x.moving())
            .filter(|move_| move_.output == *output)
            .map(|move_| move_.tile.window())
            .into_iter();

        let mon = monitors.iter().find(|mon| &mon.output == output).unwrap();
        let mon_windows = mon.workspaces.iter().flat_map(|ws| ws.windows());

        moving_window.chain(mon_windows)
    }

    pub fn windows_for_output_mut(&mut self, output: &Output) -> impl Iterator<Item = &mut W> + '_ {
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            panic!()
        };

        let moving_window = self
            .interactive_move
            .as_mut()
            .and_then(|x| x.moving_mut())
            .filter(|move_| move_.output == *output)
            .map(|move_| move_.tile.window_mut())
            .into_iter();

        let mon = monitors
            .iter_mut()
            .find(|mon| &mon.output == output)
            .unwrap();
        let mon_windows = mon.workspaces.iter_mut().flat_map(|ws| ws.windows_mut());

        moving_window.chain(mon_windows)
    }

    pub fn with_windows(
        &self,
        mut f: impl FnMut(&W, Option<&Output>, Option<WorkspaceId>, WindowLayout),
    ) {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            // We don't fill any positions for interactively moved windows.
            let layout = move_.tile.ipc_layout_template();
            f(move_.tile.window(), Some(&move_.output), None, layout);
        }

        for removed in &self.scratchpad {
            let layout = removed.tile.ipc_layout_template();
            f(removed.tile.window(), None, None, layout);
        }

        match &self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mon.workspaces {
                        for (tile, layout) in ws.tiles_with_ipc_layouts() {
                            f(tile.window(), Some(&mon.output), Some(ws.id()), layout);
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                for ws in workspaces {
                    for (tile, layout) in ws.tiles_with_ipc_layouts() {
                        f(tile.window(), None, Some(ws.id()), layout);
                    }
                }
            }
        }
    }

    pub fn with_windows_mut(&mut self, mut f: impl FnMut(&mut W, Option<&Output>)) {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            f(move_.tile.window_mut(), Some(&move_.output));
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        for win in ws.windows_mut() {
                            f(win, Some(&mon.output));
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                for ws in workspaces {
                    for win in ws.windows_mut() {
                        f(win, None);
                    }
                }
            }
        }
    }

    fn active_monitor(&mut self) -> Option<&mut Monitor<W>> {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return None;
        };

        Some(&mut monitors[*active_monitor_idx])
    }

    pub fn active_monitor_ref(&self) -> Option<&Monitor<W>> {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &self.monitor_set
        else {
            return None;
        };

        Some(&monitors[*active_monitor_idx])
    }

    pub fn monitors(&self) -> impl Iterator<Item = &Monitor<W>> + '_ {
        let monitors = if let MonitorSet::Normal { monitors, .. } = &self.monitor_set {
            &monitors[..]
        } else {
            &[][..]
        };

        monitors.iter()
    }

    fn output_layout_size(&self) -> Size<i32, Logical> {
        let mut left = 0;
        let mut top = 0;
        let mut right = 0;
        let mut bottom = 0;
        for monitor in self.monitors() {
            let location = monitor.output().current_location();
            let size = output_size(monitor.output()).to_i32_round();
            left = left.min(location.x);
            top = top.min(location.y);
            right = right.max(location.x.saturating_add(size.w));
            bottom = bottom.max(location.y.saturating_add(size.h));
        }
        Size::from((right.saturating_sub(left), bottom.saturating_sub(top)))
    }

    pub fn monitors_mut(&mut self) -> impl Iterator<Item = &mut Monitor<W>> + '_ {
        let monitors = if let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set {
            &mut monitors[..]
        } else {
            &mut [][..]
        };

        monitors.iter_mut()
    }

    pub fn monitor_for_output(&self, output: &Output) -> Option<&Monitor<W>> {
        self.monitors().find(|mon| &mon.output == output)
    }

    pub fn monitor_for_output_mut(&mut self, output: &Output) -> Option<&mut Monitor<W>> {
        self.monitors_mut().find(|mon| &mon.output == output)
    }

    pub fn monitor_for_workspace(&self, workspace_name: &str) -> Option<&Monitor<W>> {
        self.monitors().find(|monitor| {
            monitor.workspaces.iter().any(|workspace| {
                workspace
                    .sway_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(workspace_name))
            })
        })
    }

    pub fn outputs(&self) -> impl Iterator<Item = &Output> + '_ {
        self.monitors().map(|mon| &mon.output)
    }

    pub fn move_left(&mut self) -> bool {
        let Some(workspace) = self.active_workspace_mut() else {
            return false;
        };
        workspace.move_left()
    }

    pub fn move_right(&mut self) -> bool {
        let Some(workspace) = self.active_workspace_mut() else {
            return false;
        };
        workspace.move_right()
    }

    pub fn move_window_in_direction(
        &mut self,
        window: &W::Id,
        direction: tiling_tree::Direction,
        pixels: f64,
    ) -> bool {
        self.workspaces_mut()
            .find(|workspace| workspace.has_window(window))
            .is_some_and(|workspace| workspace.move_window_in_direction(window, direction, pixels))
    }

    pub fn move_tiling_node_in_direction(
        &mut self,
        workspace_id: workspace::WorkspaceId,
        node: tiling_tree::NodeId,
        direction: tiling_tree::Direction,
    ) -> bool {
        self.workspaces_mut()
            .find(|workspace| workspace.id() == workspace_id)
            .is_some_and(|workspace| workspace.move_tiling_node_in_direction(node, direction))
    }

    pub fn move_column_to_first(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.move_column_to_first();
    }

    pub fn move_column_to_last(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.move_column_to_last();
    }

    pub fn move_column_left_or_to_output(&mut self, output: &Output) -> bool {
        if let Some(workspace) = self.active_workspace_mut() {
            if workspace.move_left() {
                return false;
            }
        }

        self.move_column_to_output(output, None, true);
        true
    }

    pub fn move_column_right_or_to_output(&mut self, output: &Output) -> bool {
        if let Some(workspace) = self.active_workspace_mut() {
            if workspace.move_right() {
                return false;
            }
        }

        self.move_column_to_output(output, None, true);
        true
    }

    pub fn move_column_to_index(&mut self, index: usize) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.move_column_to_index(index);
    }

    pub fn move_down(&mut self) -> bool {
        let Some(workspace) = self.active_workspace_mut() else {
            return false;
        };
        workspace.move_down()
    }

    pub fn move_up(&mut self) -> bool {
        let Some(workspace) = self.active_workspace_mut() else {
            return false;
        };
        workspace.move_up()
    }

    pub fn move_down_or_to_workspace_down(&mut self) {
        if self
            .active_workspace_mut()
            .is_some_and(Workspace::move_down)
        {
            return;
        }
        self.move_to_workspace_down(true);
    }

    pub fn move_up_or_to_workspace_up(&mut self) {
        if self.active_workspace_mut().is_some_and(Workspace::move_up) {
            return;
        }
        self.move_to_workspace_up(true);
    }

    pub fn consume_or_expel_window_left(&mut self, window: Option<&W::Id>) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.consume_or_expel_window_left(window);
    }

    pub fn consume_or_expel_window_right(&mut self, window: Option<&W::Id>) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.consume_or_expel_window_right(window);
    }

    pub fn focus_parent(&mut self) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.focus_parent();
        }
    }

    pub fn focus_child(&mut self) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.focus_child();
        }
    }

    pub fn focus_next_prev_sibling(&mut self, next: bool) -> bool {
        self.active_workspace_mut()
            .is_some_and(|workspace| workspace.focus_next_prev_sibling(next))
    }

    pub fn focused_tiling_node(&self) -> Option<tiling_tree::NodeId> {
        self.active_workspace()?.focused_tiling_node()
    }

    pub fn focus_tiling_node(
        &mut self,
        workspace_id: workspace::WorkspaceId,
        id: tiling_tree::NodeId,
    ) -> bool {
        let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.id() == workspace_id)
        else {
            return false;
        };
        workspace.focus_tiling_node(id)
    }

    pub fn set_tiling_node_layout(&mut self, id: tiling_tree::NodeId, layout: tiling_tree::Layout) {
        if let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.contains_tiling_node(id))
        {
            workspace.set_tiling_node_layout(id, layout);
        }
    }

    pub fn focus_next_or_prev(&mut self, next: bool) -> Option<bool> {
        self.active_workspace_mut()
            .map(|workspace| workspace.focus_next_or_prev(next))
            .unwrap_or(Some(false))
    }

    pub fn focus_left(&mut self) -> bool {
        self.active_workspace_mut()
            .is_some_and(Workspace::focus_left)
    }

    pub fn focus_left_without_wrap(&mut self) -> bool {
        self.active_workspace_mut()
            .is_some_and(Workspace::focus_left_without_wrap)
    }

    pub fn focus_right(&mut self) -> bool {
        self.active_workspace_mut()
            .is_some_and(Workspace::focus_right)
    }

    pub fn focus_right_without_wrap(&mut self) -> bool {
        self.active_workspace_mut()
            .is_some_and(Workspace::focus_right_without_wrap)
    }

    pub fn focus_column_first(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_column_first();
    }

    pub fn focus_column_last(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_column_last();
    }

    pub fn focus_column_right_or_first(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_column_right_or_first();
    }

    pub fn focus_column_left_or_last(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_column_left_or_last();
    }

    pub fn focus_column(&mut self, index: usize) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_column(index);
    }

    fn focus_output_from_direction(
        &mut self,
        output: &Output,
        direction: tiling_tree::Direction,
    ) -> bool {
        self.focus_output(output);
        self.active_workspace_mut()
            .is_some_and(|workspace| workspace.focus_from_output_direction(direction))
    }

    pub fn focus_window_up_or_output(&mut self, output: &Output) -> bool {
        if let Some(workspace) = self.active_workspace_mut() {
            if workspace.focus_up_without_wrap() {
                return false;
            }
        }

        self.focus_output_from_direction(output, tiling_tree::Direction::Up);
        true
    }

    pub fn focus_window_down_or_output(&mut self, output: &Output) -> bool {
        if let Some(workspace) = self.active_workspace_mut() {
            if workspace.focus_down_without_wrap() {
                return false;
            }
        }

        self.focus_output_from_direction(output, tiling_tree::Direction::Down);
        true
    }

    pub fn focus_column_left_or_output(&mut self, output: &Output) -> bool {
        if let Some(workspace) = self.active_workspace_mut() {
            if workspace.focus_left_without_wrap() {
                return false;
            }
        }

        self.focus_output_from_direction(output, tiling_tree::Direction::Left);
        true
    }

    pub fn focus_column_right_or_output(&mut self, output: &Output) -> bool {
        if let Some(workspace) = self.active_workspace_mut() {
            if workspace.focus_right_without_wrap() {
                return false;
            }
        }

        self.focus_output_from_direction(output, tiling_tree::Direction::Right);
        true
    }

    pub fn focus_window_in_column(&mut self, index: u8) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_window_in_column(index);
    }

    pub fn focus_down(&mut self) -> bool {
        self.active_workspace_mut()
            .is_some_and(Workspace::focus_down)
    }

    pub fn focus_down_without_wrap(&mut self) -> bool {
        self.active_workspace_mut()
            .is_some_and(Workspace::focus_down_without_wrap)
    }

    pub fn focus_up(&mut self) -> bool {
        self.active_workspace_mut().is_some_and(Workspace::focus_up)
    }

    pub fn focus_up_without_wrap(&mut self) -> bool {
        self.active_workspace_mut()
            .is_some_and(Workspace::focus_up_without_wrap)
    }

    pub fn focus_down_or_left(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_down_or_left();
    }

    pub fn focus_down_or_right(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_down_or_right();
    }

    pub fn focus_up_or_left(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_up_or_left();
    }

    pub fn focus_up_or_right(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_up_or_right();
    }

    pub fn focus_window_or_workspace_down(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.focus_window_or_workspace_down();
    }

    pub fn focus_window_or_workspace_up(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.focus_window_or_workspace_up();
    }

    pub fn focus_window_top(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_window_top();
    }

    pub fn focus_window_bottom(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_window_bottom();
    }

    pub fn focus_window_down_or_top(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_window_down_or_top();
    }

    pub fn focus_window_up_or_bottom(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_window_up_or_bottom();
    }

    pub fn move_to_workspace_up(&mut self, focus: bool) {
        let Some(target) = self.active_monitor_ref().and_then(|monitor| {
            monitor
                .active_workspace_idx
                .checked_sub(1)
                .map(|index| monitor.workspaces[index].id())
        }) else {
            return;
        };
        self.move_to_workspace_id(None, target, Self::move_activation(focus));
    }

    pub fn move_to_workspace_down(&mut self, focus: bool) {
        let Some((output, target_index)) = self
            .active_monitor_ref()
            .filter(|monitor| monitor.active_workspace_ref().active_window().is_some())
            .map(|monitor| (monitor.output.clone(), monitor.active_workspace_idx + 1))
        else {
            return;
        };
        let target = self
            .prepare_workspace_at(&output, target_index)
            .unwrap_or_else(|| self.create_next_workspace(&output));
        self.move_to_workspace_id(None, target, Self::move_activation(focus));
    }

    fn move_activation(focus: bool) -> ActivateWindow {
        if focus {
            ActivateWindow::Smart
        } else {
            ActivateWindow::No
        }
    }

    fn create_workspace_at(&mut self, output: &Output, index: usize) -> WorkspaceId {
        let (name, number) = self.next_free_workspace_identity();
        let monitor = self.monitor_for_output_mut(output).unwrap();
        let id = monitor.add_sway_workspace_at(index, name, number);
        monitor.sort_sway_workspaces();
        id
    }

    fn create_next_workspace(&mut self, output: &Output) -> WorkspaceId {
        let index = self
            .monitor_for_output(output)
            .map(|monitor| monitor.workspaces.len())
            .unwrap();
        self.create_workspace_at(output, index)
    }

    fn prepare_workspace_at(&mut self, output: &Output, index: usize) -> Option<WorkspaceId> {
        let workspace = self.monitor_for_output(output)?.workspaces.get(index)?;
        if workspace.has_sway_identity() {
            Some(workspace.id())
        } else {
            Some(self.create_workspace_at(output, index))
        }
    }

    pub fn move_to_workspace(
        &mut self,
        window: Option<&W::Id>,
        idx: usize,
        activate: ActivateWindow,
    ) {
        if window.is_none()
            && self
                .active_workspace()
                .and_then(Workspace::active_window)
                .is_none()
        {
            return;
        }
        let output = if let Some(window) = window {
            self.monitors()
                .find(|monitor| monitor.has_window(window))
                .map(|monitor| monitor.output.clone())
        } else {
            self.active_output().cloned()
        };
        let Some(output) = output else {
            return;
        };
        if let Some(target) = self.prepare_workspace_at(&output, idx) {
            self.move_to_workspace_id(window, target, activate);
        }
    }

    fn move_to_workspace_id(
        &mut self,
        window: Option<&W::Id>,
        target: WorkspaceId,
        activate: ActivateWindow,
    ) {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let monitor = if let Some(window) = window {
            match &mut self.monitor_set {
                MonitorSet::Normal { monitors, .. } => monitors
                    .iter_mut()
                    .find(|mon| mon.has_window(window))
                    .unwrap(),
                MonitorSet::NoOutputs { .. } => {
                    return;
                }
            }
        } else {
            let Some(monitor) = self.active_monitor() else {
                return;
            };
            monitor
        };
        monitor.move_to_workspace(window, target, activate);
    }

    pub fn move_column_to_workspace_up(&mut self, activate: bool) {
        let Some(target) = self.active_monitor_ref().and_then(|monitor| {
            monitor
                .active_workspace_idx
                .checked_sub(1)
                .map(|index| monitor.workspaces[index].id())
        }) else {
            return;
        };
        self.move_column_to_workspace_id(target, activate);
    }

    pub fn move_column_to_workspace_down(&mut self, activate: bool) {
        let Some((output, target_index)) = self
            .active_monitor_ref()
            .filter(|monitor| monitor.active_workspace_ref().active_window().is_some())
            .map(|monitor| (monitor.output.clone(), monitor.active_workspace_idx + 1))
        else {
            return;
        };
        let target = self
            .prepare_workspace_at(&output, target_index)
            .unwrap_or_else(|| self.create_next_workspace(&output));
        self.move_column_to_workspace_id(target, activate);
    }

    pub fn move_column_to_workspace(&mut self, idx: usize, activate: bool) {
        if self
            .active_workspace()
            .and_then(Workspace::active_window)
            .is_none()
        {
            return;
        }
        let Some(output) = self.active_output().cloned() else {
            return;
        };
        if let Some(target) = self.prepare_workspace_at(&output, idx) {
            self.move_column_to_workspace_id(target, activate);
        }
    }

    fn move_column_to_workspace_id(&mut self, target: WorkspaceId, activate: bool) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.move_column_to_workspace(target, activate);
    }

    pub fn switch_workspace_up(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.switch_workspace_up();
    }

    pub fn switch_workspace_down(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.switch_workspace_down();
    }

    pub fn switch_workspace(&mut self, idx: usize) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.switch_workspace(idx);
    }

    pub fn switch_workspace_auto_back_and_forth(&mut self, idx: usize) {
        let previous_name = {
            let Some(monitor) = self.active_monitor() else {
                return;
            };
            let idx = idx.min(monitor.workspaces.len() - 1);
            (idx == monitor.active_workspace_idx && monitor.previous_workspace_idx().is_none())
                .then(|| monitor.previous_workspace_name().map(str::to_owned))
                .flatten()
        };
        if let Some(previous_name) = previous_name {
            let _ =
                self.activate_sway_workspace(crate::command::WorkspaceTarget::Name(previous_name));
        } else if let Some(monitor) = self.active_monitor() {
            monitor.switch_workspace_auto_back_and_forth(idx);
        }
    }

    pub fn switch_workspace_previous(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.switch_workspace_previous();
    }

    pub fn activate_sway_workspace_auto_back_and_forth(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(), String> {
        use crate::command::WorkspaceTarget;

        let existing = self.workspaces().find_map(|(monitor, index, workspace)| {
            let matches = match &target {
                // Sway matches the digit PREFIX of the name, not a stored
                // number: _workspace_by_number (sway/sway/tree/workspace.c:
                // 493-502) walks the digits of the target against the name and
                // requires the name to have no further digits. So "1" matches
                // "1:first" but not "11". Comparing a stored number missed a
                // workspace whose number was derived rather than stored, and a
                // duplicate was created instead.
                // Only a workspace with a real identity can be matched by
                // number. An unnamed placeholder reports a name like "1", which
                // would otherwise match `workspace number 1` ahead of a real
                // "1:first" and activate the wrong one.
                WorkspaceTarget::Number(value) => {
                    workspace.has_sway_identity()
                        && workspace
                            .sway_name()
                            .is_some_and(|name| workspace_name_matches_number(&name, value))
                }
                WorkspaceTarget::Name(value) => workspace
                    .sway_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case(value)),
                _ => false,
            };
            matches.then(|| (monitor.map(|monitor| monitor.output().clone()), index))
        });
        let Some((output, index)) = existing else {
            return self.activate_sway_workspace(target);
        };
        if let Some(output) = output {
            if self.active_output() == Some(&output) {
                self.switch_workspace_auto_back_and_forth(index);
            } else {
                self.focus_output(&output);
                self.switch_workspace(index);
            }
        } else {
            self.switch_workspace_auto_back_and_forth(index);
        }
        Ok(())
    }

    pub fn activate_sway_workspace(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(), String> {
        use crate::command::WorkspaceTarget;

        match target {
            WorkspaceTarget::Current => return Ok(()),
            WorkspaceTarget::BackAndForth => {
                let Some(monitor) = self.active_monitor() else {
                    return Err("cannot switch workspaces without an output".into());
                };
                if let Some(previous) = monitor.previous_workspace_idx() {
                    self.switch_workspace(previous);
                    return Ok(());
                }
                let Some(previous_name) = monitor.previous_workspace_name().map(str::to_owned)
                else {
                    return Err("There is no previous workspace".into());
                };
                return self.activate_sway_workspace(WorkspaceTarget::Name(previous_name));
            }
            WorkspaceTarget::NextOnOutput | WorkspaceTarget::PrevOnOutput => {
                let next = target == WorkspaceTarget::NextOnOutput;
                let Some((output, workspace)) =
                    self.relative_sway_workspace_position_on_output(next)
                else {
                    return Err("cannot switch workspaces without an output".into());
                };
                if let Some(output) = output {
                    self.focus_output(&output);
                }
                self.switch_workspace(workspace);
                return Ok(());
            }
            WorkspaceTarget::Next | WorkspaceTarget::Prev => {
                return self.activate_relative_sway_workspace(target == WorkspaceTarget::Next);
            }
            _ => {}
        }

        // Collect every candidate rather than taking the first. We keep an
        // unnamed placeholder that sway would have destroyed outright
        // (workspace_consider_destroy), so after `workspace 1:first` both it and
        // "1:first" answer to `workspace number 1`. Sway never sees that pair,
        // so prefer the explicitly named workspace, which is the one sway would
        // still have.
        let mut candidates = self
            .workspaces()
            .filter_map(|(monitor, index, workspace)| {
                let matches = match &target {
                    // Match the digit prefix of the name, as sway's
                    // _workspace_by_number does (sway/sway/tree/workspace.c:
                    // 493-502), and only for a workspace with a real identity: an
                    // unnamed placeholder reports a name like "1" and would
                    // otherwise be activated ahead of a real "1:first".
                    WorkspaceTarget::Number(value) => {
                        workspace.has_sway_identity()
                            && workspace
                                .sway_name()
                                .is_some_and(|name| workspace_name_matches_number(&name, value))
                    }
                    WorkspaceTarget::Name(value) => workspace
                        .sway_name()
                        .is_some_and(|name| name.eq_ignore_ascii_case(value)),
                    _ => false,
                };
                matches.then(|| {
                    (
                        monitor.map(|monitor| monitor.output().clone()),
                        index,
                        workspace.name().is_some(),
                    )
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, _, named)| !*named);
        let existing = candidates
            .into_iter()
            .next()
            .map(|(output, index, _)| (output, index));

        if let Some((output, index)) = existing {
            self.activate_workspace_at(output.as_ref(), index);
            return Ok(());
        }

        let (name, number) = sway_workspace_identity(target)?;
        let workspace_name = name
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| number.unwrap().to_string());
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return Err("cannot create a workspace without an output".into());
        };
        let monitor_idx = self
            .workspace_configs
            .iter()
            .find(|config| config.name.0.eq_ignore_ascii_case(&workspace_name))
            .and_then(|config| config.sway_output_assignment.as_ref())
            .and_then(|outputs| {
                outputs.iter().find_map(|name| {
                    monitors
                        .iter()
                        .position(|monitor| output_matches_name(&monitor.output, name))
                })
            })
            .unwrap_or(*active_monitor_idx);
        let monitor = &mut monitors[monitor_idx];
        let index = monitor.workspaces_len().saturating_sub(1);
        let id = monitor.add_sway_workspace_at(index, name, number);
        // Sway sorts on every creation: workspace_create calls
        // output_sort_workspaces (sway/sway/tree/workspace.c:259), which orders
        // numeric names numerically and ahead of non-numeric ones
        // (sway/sway/tree/output.c:387-405). The new workspace therefore does
        // not stay where it was inserted, so re-find it before activating.
        monitor.sort_sway_workspaces();
        let index = monitor.idx_of_ws(id).unwrap_or(index);
        monitor.activate_workspace(index);
        Ok(())
    }

    pub fn activate_workspace_at(&mut self, output: Option<&Output>, index: usize) {
        if let Some(output) = output {
            self.focus_output(output);
        }
        self.switch_workspace(index);
    }

    pub fn rename_sway_workspace(
        &mut self,
        old: Option<crate::command::WorkspaceTarget>,
        new_name: String,
    ) -> Result<(), String> {
        let id = match old {
            Some(ref target) => self
                .workspaces()
                .find(|(_, _, workspace)| workspace_matches_target(workspace, target))
                .map(|(_, _, workspace)| workspace.id()),
            None => self.active_workspace().map(Workspace::id),
        }
        .ok_or_else(|| "There is no workspace with that name".to_owned())?;
        if matches!(
            new_name.to_ascii_lowercase().as_str(),
            "next"
                | "prev"
                | "next_on_output"
                | "prev_on_output"
                | "back_and_forth"
                | "current"
                | "number"
        ) {
            return Err(format!("Cannot use special workspace name '{new_name}'"));
        }
        // Ignore a workspace that sway would already have destroyed. Sway
        // destroys an empty, non-visible workspace no seat retains, and does so
        // when focus LEAVES it (seat_set_focus, sway/sway/input/seat.c:1244),
        // so by the time a rename runs the name is free. We keep such a
        // workspace, so a stale one blocked the rename with `Workspace already
        // exists`.
        if let Some(existing) = self.workspaces().find_map(|(monitor, index, workspace)| {
            // Sway also retains the workspace a seat's focus-inactive points
            // at (workspace_consider_destroy, sway/sway/tree/workspace.c:
            // 322-329), which is the one we would return to via
            // back_and_forth, so do not treat that one as destroyed.
            let retained_by_seat = monitor
                .is_some_and(|monitor| monitor.previous_workspace_id() == Some(workspace.id()));
            let destroyed_by_sway = !workspace.has_windows()
                && !retained_by_seat
                && monitor.is_some_and(|monitor| monitor.active_workspace_idx() != index);
            if destroyed_by_sway {
                return None;
            }
            workspace
                .sway_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(&new_name))
                .then(|| workspace.id())
        }) {
            return (existing == id)
                .then_some(())
                .ok_or_else(|| "Workspace already exists".into());
        }

        let (name, number) =
            sway_workspace_identity(crate::command::WorkspaceTarget::Name(new_name))?;
        self.workspaces_mut()
            .find(|workspace| workspace.id() == id)
            .unwrap()
            .set_sway_identity(name, number);
        if let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set {
            if let Some(monitor) = monitors.iter_mut().find(|monitor| monitor.has_ws(id)) {
                monitor.sort_sway_workspaces();
                // Refresh the back-and-forth target if it names this workspace.
                // Sway stores a workspace pointer, so a rename is transparent to
                // it; we cache the name, which went stale and sent
                // `workspace back_and_forth` to the old name.
                monitor.refresh_previous_workspace_name(id);
            }
        }
        Ok(())
    }

    fn activate_relative_sway_workspace(&mut self, next: bool) -> Result<(), String> {
        let Some((output, workspace)) = self.relative_sway_workspace_position(next) else {
            return Err("cannot switch workspaces without an output".into());
        };
        if let Some(output) = output {
            self.focus_output(&output);
        }
        self.switch_workspace(workspace);
        Ok(())
    }

    fn relative_sway_workspace_position(&self, next: bool) -> Option<(Option<Output>, usize)> {
        let active = self.active_workspace()?;
        let current_number = active.number();
        let active_id = active.id();
        let positions = self
            .workspaces()
            .filter(|(_, _, workspace)| {
                workspace.has_windows()
                    || workspace.has_sway_identity()
                    || workspace.id() == active_id
            })
            .map(|(monitor, index, workspace)| {
                (
                    monitor.map(|monitor| monitor.output().clone()),
                    index,
                    workspace.id(),
                    workspace.number(),
                )
            })
            .collect::<Vec<_>>();
        let current = positions
            .iter()
            .position(|(_, _, id, _)| *id == active.id())?;

        let target = if let Some(number) = current_number {
            let numbered = positions
                .iter()
                .filter(|(_, _, _, candidate)| candidate.is_some());
            let relative = numbered
                .clone()
                .filter(|(_, _, _, candidate)| {
                    candidate.is_some_and(|candidate| {
                        if next {
                            candidate > number
                        } else {
                            candidate < number
                        }
                    })
                })
                .min_by_key(|(_, _, _, candidate)| {
                    candidate.map(|candidate| candidate.abs_diff(number))
                });
            relative.or_else(|| {
                let named = positions
                    .iter()
                    .filter(|(_, _, _, candidate)| candidate.is_none());
                let other = if next {
                    named.clone().next()
                } else {
                    named.clone().next_back()
                };
                other.or_else(|| {
                    if next {
                        numbered.min_by_key(|(_, _, _, candidate)| *candidate)
                    } else {
                        numbered.max_by_key(|(_, _, _, candidate)| *candidate)
                    }
                })
            })
        } else {
            let named = positions
                .iter()
                .enumerate()
                .filter(|(_, (_, _, _, number))| number.is_none());
            let relative = if next {
                named.clone().find(|(index, _)| *index > current)
            } else {
                named.clone().rev().find(|(index, _)| *index < current)
            };
            relative.map(|(_, position)| position).or_else(|| {
                let numbered = positions
                    .iter()
                    .filter(|(_, _, _, number)| number.is_some());
                if next {
                    numbered.min_by_key(|(_, _, _, number)| *number)
                } else {
                    numbered.max_by_key(|(_, _, _, number)| *number)
                }
                .or_else(|| {
                    if next {
                        named.map(|(_, position)| position).next()
                    } else {
                        named.map(|(_, position)| position).next_back()
                    }
                })
            })
        }?;
        Some((target.0.clone(), target.1))
    }

    fn relative_sway_workspace_position_on_output(
        &self,
        next: bool,
    ) -> Option<(Option<Output>, usize)> {
        let output = self.active_output()?;
        let monitor = self.monitor_for_output(output)?;
        let current = monitor.active_workspace_idx;
        let positions = monitor
            .workspaces
            .iter()
            .enumerate()
            .filter(|(index, workspace)| {
                workspace.has_windows() || workspace.has_sway_identity() || *index == current
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let current = positions.iter().position(|index| *index == current)?;
        let target = if next {
            positions.get(current + 1).or_else(|| positions.first())
        } else {
            current
                .checked_sub(1)
                .and_then(|index| positions.get(index))
                .or_else(|| positions.last())
        }?;
        Some((Some(output.clone()), *target))
    }

    fn active_workspace_position(&self) -> Option<(Option<Output>, usize)> {
        let output = self.active_output()?.clone();
        let monitor = self.monitor_for_output(&output)?;
        Some((Some(output), monitor.active_workspace_idx))
    }

    fn previous_workspace_position(&self) -> Option<(Option<Output>, usize)> {
        let output = self.active_output()?.clone();
        let monitor = self.monitor_for_output(&output)?;
        Some((Some(output), monitor.previous_workspace_idx()?))
    }

    pub fn move_window_to_sway_workspace(
        &mut self,
        window: &W::Id,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> Result<(), String> {
        self.move_to_sway_workspace_inner(Some(window), target, auto_back_and_forth)
    }

    pub fn move_to_sway_workspace(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(), String> {
        self.move_to_sway_workspace_inner(None, target, true)
    }

    pub fn is_tiling_root(&self, workspace: WorkspaceId, node: tiling_tree::NodeId) -> bool {
        self.workspaces()
            .find(|(_, _, candidate)| candidate.id() == workspace)
            .is_some_and(|(_, _, candidate)| candidate.tiling().is_root(node))
    }

    pub fn swap_tiling_nodes(
        &mut self,
        workspace: WorkspaceId,
        first: tiling_tree::NodeId,
        second: tiling_tree::NodeId,
    ) -> Result<(), String> {
        let workspace = self
            .workspaces_mut()
            .find(|candidate| candidate.id() == workspace)
            .ok_or_else(|| "No matching node.".to_owned())?;
        workspace
            .swap_tiling_nodes(first, second)
            .map_err(str::to_owned)
    }

    #[allow(clippy::type_complexity)]
    pub fn swap_tiling_nodes_between_workspaces(
        &mut self,
        first_workspace: WorkspaceId,
        first: tiling_tree::NodeId,
        second_workspace: WorkspaceId,
        second: tiling_tree::NodeId,
    ) -> Result<
        (
            Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>,
            Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>,
        ),
        String,
    > {
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot swap containers without an output".into());
        };
        let first_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(first_workspace))
            .ok_or_else(|| "No matching node.".to_owned())?;
        let second_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(second_workspace))
            .ok_or_else(|| "No matching node.".to_owned())?;
        let (first_ws, second_ws) = if first_monitor == second_monitor {
            let monitor = &mut monitors[first_monitor];
            let first_idx = monitor.idx_of_ws(first_workspace).unwrap();
            let second_idx = monitor.idx_of_ws(second_workspace).unwrap();
            if first_idx < second_idx {
                let (before, after) = monitor.workspaces.split_at_mut(second_idx);
                (&mut before[first_idx], &mut after[0])
            } else {
                let (before, after) = monitor.workspaces.split_at_mut(first_idx);
                (&mut after[0], &mut before[second_idx])
            }
        } else if first_monitor < second_monitor {
            let (before, after) = monitors.split_at_mut(second_monitor);
            let first_idx = before[first_monitor].idx_of_ws(first_workspace).unwrap();
            let second_idx = after[0].idx_of_ws(second_workspace).unwrap();
            (
                &mut before[first_monitor].workspaces[first_idx],
                &mut after[0].workspaces[second_idx],
            )
        } else {
            let (before, after) = monitors.split_at_mut(first_monitor);
            let first_idx = after[0].idx_of_ws(first_workspace).unwrap();
            let second_idx = before[second_monitor].idx_of_ws(second_workspace).unwrap();
            (
                &mut after[0].workspaces[first_idx],
                &mut before[second_monitor].workspaces[second_idx],
            )
        };
        let (mut first_subtree, first_slot) = first_ws
            .detach_tiling_subtree_for_swap(first)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let (mut second_subtree, second_slot) = second_ws
            .detach_tiling_subtree_for_swap(second)
            .ok_or_else(|| "No matching node.".to_owned())?;
        first_subtree.swap_root_mode(&mut second_subtree);
        let second_remapped = first_ws
            .attach_tiling_subtree_for_swap(second_subtree, first_slot)
            .1;
        let first_remapped = second_ws
            .attach_tiling_subtree_for_swap(first_subtree, second_slot)
            .1;
        first_ws.finish_tiling_subtree_detach(None);
        second_ws.finish_tiling_subtree_detach(None);
        Ok((first_remapped, second_remapped))
    }

    pub fn move_tiling_subtree_to_node(
        &mut self,
        source_workspace: WorkspaceId,
        source: tiling_tree::NodeId,
        target_workspace: WorkspaceId,
        target: tiling_tree::NodeId,
    ) -> Result<Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>, String> {
        if source_workspace == target_workspace {
            let workspace = self
                .workspaces_mut()
                .find(|workspace| workspace.id() == source_workspace)
                .ok_or_else(|| "No matching node.".to_owned())?;
            if !workspace.contains_tiling_node(source) || !workspace.contains_tiling_node(target) {
                return Err("No matching node.".to_owned());
            }
            workspace.move_tiling_subtree_to_node(source, target);
            Ok(Vec::new())
        } else {
            let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
                return Err("cannot move a container without an output".into());
            };
            let source_monitor = monitors
                .iter()
                .position(|monitor| monitor.has_ws(source_workspace))
                .ok_or_else(|| "No matching node.".to_owned())?;
            let target_monitor = monitors
                .iter()
                .position(|monitor| monitor.has_ws(target_workspace))
                .ok_or_else(|| "No matching node.".to_owned())?;
            let (source_ws, target_ws) = if source_monitor == target_monitor {
                let monitor = &mut monitors[source_monitor];
                let source_idx = monitor.idx_of_ws(source_workspace).unwrap();
                let target_idx = monitor.idx_of_ws(target_workspace).unwrap();
                if source_idx < target_idx {
                    let (before, after) = monitor.workspaces.split_at_mut(target_idx);
                    (&mut before[source_idx], &mut after[0])
                } else {
                    let (before, after) = monitor.workspaces.split_at_mut(source_idx);
                    (&mut after[0], &mut before[target_idx])
                }
            } else if source_monitor < target_monitor {
                let (before, after) = monitors.split_at_mut(target_monitor);
                let source_idx = before[source_monitor].idx_of_ws(source_workspace).unwrap();
                let target_idx = after[0].idx_of_ws(target_workspace).unwrap();
                (
                    &mut before[source_monitor].workspaces[source_idx],
                    &mut after[0].workspaces[target_idx],
                )
            } else {
                let (before, after) = monitors.split_at_mut(source_monitor);
                let source_idx = after[0].idx_of_ws(source_workspace).unwrap();
                let target_idx = before[target_monitor].idx_of_ws(target_workspace).unwrap();
                (
                    &mut after[0].workspaces[source_idx],
                    &mut before[target_monitor].workspaces[target_idx],
                )
            };
            let (subtree, old_parent) = source_ws
                .detach_tiling_subtree(source)
                .ok_or_else(|| "No matching node.".to_owned())?;
            let remapped = target_ws.attach_tiling_subtree_at(subtree, Some(target)).1;
            source_ws.finish_tiling_subtree_detach(old_parent);
            if monitors[source_monitor].workspace_switch.is_none() {
                monitors[source_monitor].clean_up_workspaces();
            }
            Ok(remapped)
        }
    }

    pub fn window_workspace_id(&self, window: &W::Id) -> Option<WorkspaceId> {
        self.workspaces()
            .find_map(|(_, _, workspace)| workspace.has_window(window).then(|| workspace.id()))
    }

    pub fn move_window_to_workspace_id(
        &mut self,
        window: &W::Id,
        target: WorkspaceId,
    ) -> Result<(), String> {
        let (target_output, target_index) = self
            .workspaces()
            .find_map(|(monitor, index, workspace)| {
                (workspace.id() == target)
                    .then(|| (monitor.map(|monitor| monitor.output().clone()), index))
            })
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        let source_output = self
            .workspaces()
            .find(|(_, _, workspace)| workspace.has_window(window))
            .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output().clone()));
        if target_output != source_output {
            let output =
                target_output.ok_or_else(|| "target workspace has no output".to_owned())?;
            self.move_to_output(
                Some(window),
                &output,
                Some(target_index),
                ActivateWindow::No,
            );
        } else {
            self.move_to_workspace_id(Some(window), target, ActivateWindow::No);
        }
        Ok(())
    }

    pub fn tiling_target_for_window(
        &self,
        window: &W::Id,
    ) -> Option<(WorkspaceId, tiling_tree::NodeId)> {
        self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .tiling_node_for_window(window)
                .map(|node| (workspace.id(), node))
        })
    }

    pub fn move_tiling_subtree_to_sway_workspace(
        &mut self,
        source_workspace: WorkspaceId,
        node: tiling_tree::NodeId,
        target: crate::command::WorkspaceTarget,
        preserve_empty_workspace: bool,
        auto_back_and_forth: bool,
    ) -> Result<(WorkspaceId, Vec<(tiling_tree::NodeId, tiling_tree::NodeId)>), String> {
        let (floating, empty_root) = self
            .workspaces()
            .find(|(_, _, workspace)| workspace.id() == source_workspace)
            .filter(|(_, _, workspace)| workspace.tiling().is_root(node))
            .map(|(_, _, workspace)| {
                (
                    workspace
                        .tiles()
                        .filter(|tile| workspace.is_floating(tile.window().id()))
                        .map(|tile| tile.window().id().clone())
                        .collect::<Vec<_>>(),
                    workspace.tiling().tiles().next().is_none(),
                )
            })
            .unwrap_or_default();
        let target =
            self.resolve_move_workspace_target(source_workspace, target, auto_back_and_forth);
        let floating_target = target.clone();
        let (target_output, target_index) = self.resolve_sway_workspace_target(target)?;
        let target_workspace = match target_output.as_ref() {
            Some(output) => self
                .monitor_for_output(output)
                .and_then(|monitor| monitor.workspaces.get(target_index))
                .map(Workspace::id),
            None => self
                .workspaces()
                .nth(target_index)
                .map(|(_, _, workspace)| workspace.id()),
        }
        .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if source_workspace == target_workspace {
            return Ok((target_workspace, Vec::new()));
        }
        if empty_root {
            for window in floating {
                self.move_window_to_sway_workspace(&window, floating_target.clone(), false)?;
            }
            return Ok((target_workspace, Vec::new()));
        }

        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot move a container without an output".into());
        };
        let source_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(source_workspace))
            .ok_or_else(|| "No matching node.".to_owned())?;
        let target_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(target_workspace))
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if source_monitor == target_monitor {
            let remapped = monitors[source_monitor]
                .move_tiling_subtree_to_workspace(
                    source_workspace,
                    node,
                    target_workspace,
                    preserve_empty_workspace || !floating.is_empty(),
                )
                .ok_or_else(|| "No matching node.".to_owned())?;
            for window in floating {
                self.move_window_to_sway_workspace(&window, floating_target.clone(), false)?;
            }
            return Ok((target_workspace, remapped));
        }

        let (source, target) = if source_monitor < target_monitor {
            let (before_target, target_and_after) = monitors.split_at_mut(target_monitor);
            (&mut before_target[source_monitor], &mut target_and_after[0])
        } else {
            let (before_source, source_and_after) = monitors.split_at_mut(source_monitor);
            (&mut source_and_after[0], &mut before_source[target_monitor])
        };
        let source_idx = source
            .idx_of_ws(source_workspace)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let target_idx = target
            .idx_of_ws(target_workspace)
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        let (subtree, old_parent) = source.workspaces[source_idx]
            .detach_tiling_subtree(node)
            .ok_or_else(|| "No matching node.".to_owned())?;
        let remapped = target.workspaces[target_idx]
            .attach_tiling_subtree(subtree)
            .1;
        source.workspaces[source_idx].finish_tiling_subtree_detach(old_parent);
        if !preserve_empty_workspace && floating.is_empty() && source.workspace_switch.is_none() {
            source.clean_up_workspaces();
        }
        for window in floating {
            self.move_window_to_sway_workspace(&window, floating_target.clone(), false)?;
        }
        Ok((target_workspace, remapped))
    }

    fn resolve_sway_workspace_target(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(Option<Output>, usize), String> {
        use crate::command::WorkspaceTarget;

        let target_position = match target {
            WorkspaceTarget::Current => self.active_workspace_position(),
            WorkspaceTarget::BackAndForth => self.previous_workspace_position(),
            WorkspaceTarget::Next | WorkspaceTarget::Prev => {
                let next = target == WorkspaceTarget::Next;
                self.relative_sway_workspace_position(next)
            }
            WorkspaceTarget::NextOnOutput | WorkspaceTarget::PrevOnOutput => {
                let next = target == WorkspaceTarget::NextOnOutput;
                self.relative_sway_workspace_position_on_output(next)
            }
            _ => {
                // Prefer an explicitly named workspace over the unnamed
                // placeholder, which sway would have destroyed and which
                // otherwise wins `number 1` against a real "1:first".
                let mut found = self
                    .workspaces()
                    .filter(|(_, _, workspace)| workspace_matches_target(workspace, &target))
                    .map(|(monitor, index, workspace)| {
                        (
                            monitor.map(|monitor| monitor.output().clone()),
                            index,
                            workspace.name().is_some(),
                        )
                    })
                    .collect::<Vec<_>>();
                found.sort_by_key(|(_, _, named)| !*named);
                found
                    .into_iter()
                    .next()
                    .map(|(output, index, _)| (output, index))
            }
        };
        if let Some(position) = target_position {
            Ok(position)
        } else {
            let (name, number) = sway_workspace_identity(target)?;
            let MonitorSet::Normal {
                monitors,
                active_monitor_idx,
                ..
            } = &mut self.monitor_set
            else {
                return Err("cannot create a workspace without an output".into());
            };
            let monitor = &mut monitors[*active_monitor_idx];
            let id = monitor.add_sway_workspace_at(monitor.workspaces.len(), name, number);
            monitor.sort_sway_workspaces();
            let index = monitor.idx_of_ws(id).unwrap();
            Ok((Some(monitor.output().clone()), index))
        }
    }

    fn resolve_move_workspace_target(
        &self,
        source_workspace: WorkspaceId,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> crate::command::WorkspaceTarget {
        if !auto_back_and_forth {
            return target;
        }
        let targets_source = self
            .workspaces()
            .find(|(_, _, workspace)| workspace.id() == source_workspace)
            .is_some_and(|(_, _, workspace)| workspace_matches_target(workspace, &target));
        if !targets_source {
            return target;
        }
        self.active_monitor_ref()
            .and_then(|monitor| monitor.previous_workspace_name())
            .map(|name| crate::command::WorkspaceTarget::Name(name.to_owned()))
            .unwrap_or(target)
    }

    fn move_to_sway_workspace_inner(
        &mut self,
        window: Option<&W::Id>,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> Result<(), String> {
        let source_workspace = window
            .and_then(|window| {
                self.workspaces()
                    .find(|(_, _, workspace)| workspace.has_window(window))
                    .map(|(_, _, workspace)| workspace.id())
            })
            .or_else(|| self.active_workspace().map(Workspace::id));
        let target = source_workspace.map_or(target.clone(), |source| {
            self.resolve_move_workspace_target(source, target, auto_back_and_forth)
        });
        let (target_output, target_index) = self.resolve_sway_workspace_target(target)?;
        let source_output = window
            .and_then(|window| {
                self.workspaces()
                    .find(|(_, _, workspace)| workspace.has_window(window))
                    .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output().clone()))
            })
            .or_else(|| self.active_output().cloned());
        let target_workspace = match target_output.as_ref() {
            Some(output) => self
                .monitor_for_output(output)
                .and_then(|monitor| monitor.workspaces.get(target_index)),
            None => self
                .workspaces()
                .nth(target_index)
                .map(|(_, _, workspace)| workspace),
        }
        .map(Workspace::id)
        .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if target_output != source_output {
            let output =
                target_output.ok_or_else(|| "target workspace has no output".to_owned())?;
            self.move_to_output(window, &output, Some(target_index), ActivateWindow::No);
        } else {
            self.move_to_workspace_id(window, target_workspace, ActivateWindow::No);
        }
        Ok(())
    }

    pub fn window_border(
        &self,
        window: &W::Id,
    ) -> Option<(swayward_ipc::command::BorderStyle, u16)> {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return Some(move_.tile.sway_border());
            }
        }
        if let Some(removed) = self
            .scratchpad
            .iter()
            .find(|removed| removed.tile.window().id() == window)
        {
            return Some(removed.tile.sway_border());
        }
        self.workspaces()
            .find(|(_, _, workspace)| workspace.has_window(window))
            .and_then(|(_, _, workspace)| workspace.window_border(window))
    }

    pub fn set_window_border(
        &mut self,
        window: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> Result<(), &'static str> {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if move_.tile.window().id() == window {
                return move_
                    .tile
                    .set_sway_border(style, width, move_.is_floating)
                    .map(|_| ());
            }
        }
        if let Some(removed) = self
            .scratchpad
            .iter_mut()
            .find(|removed| removed.tile.window().id() == window)
        {
            return removed.tile.set_sway_border(style, width, true).map(|_| ());
        }
        self.workspaces_mut()
            .find(|workspace| workspace.has_window(window))
            .ok_or("Only views can have borders")?
            .set_window_border(window, style, width)
    }

    pub fn set_window_sticky(&mut self, window: &W::Id, value: &str) -> bool {
        let current = self
            .workspaces()
            .any(|(_, _, workspace)| workspace.is_window_sticky(window));
        let sticky = swayward_ipc::command::parse_boolean(value, current);
        let Some(monitor) = self
            .monitors_mut()
            .find(|monitor| monitor.has_window(window))
        else {
            return false;
        };
        let Some(source) = monitor
            .workspaces
            .iter()
            .find(|workspace| workspace.has_window(window))
            .map(Workspace::id)
        else {
            return false;
        };
        let source_idx = monitor.idx_of_ws(source).unwrap();
        if !monitor.workspaces[source_idx].set_window_sticky(window, sticky) {
            return true;
        }
        let target = monitor.active_workspace_ref().id();
        if sticky && source != target {
            let removed = monitor.workspaces[source_idx].take_sticky_tiles();
            let target_idx = monitor.idx_of_ws(target).unwrap();
            for removed in removed {
                monitor.workspaces[target_idx].add_tile(
                    removed.tile,
                    WorkspaceAddWindowTarget::Auto,
                    ActivateWindow::Yes,
                    removed.width,
                    removed.is_full_width,
                    true,
                    None,
                );
            }
            if monitor.workspace_switch.is_none() {
                monitor.clean_up_workspaces();
            }
        }
        true
    }

    pub fn move_to_scratchpad(&mut self, window: Option<&W::Id>) {
        let window = window
            .cloned()
            .or_else(|| self.focus().map(|window| window.id().clone()));
        let Some(window) = window else {
            return;
        };
        if self
            .scratchpad
            .iter()
            .any(|removed| removed.tile.window().id() == &window)
        {
            return;
        }
        let automatic_maximum = self.output_layout_size();
        let mut floating_working_area = None;
        if let Some(workspace) = self.workspaces_mut().find(|ws| ws.has_window(&window)) {
            workspace.prepare_tiled_window_for_scratchpad(&window, automatic_maximum);
            if workspace.fullscreen_contains_window(&window) {
                workspace.set_fullscreen(&window, false);
            }
            floating_working_area = Some(workspace.working_area());
        }
        let Some((mut removed, source_workspace)) = self.detach_window(&window, Transaction::new())
        else {
            return;
        };
        removed.floating_working_area = floating_working_area;
        if !self.scratchpad_windows.contains(&window) {
            self.scratchpad_windows.push(window);
        }
        self.scratchpad.push_back(removed);
        if let Some(source_workspace) = source_workspace {
            self.clean_up_removed_window_workspace(source_workspace);
        }
    }

    pub fn show_scratchpad(&mut self, window: Option<&W::Id>) -> Option<W::Id> {
        let focused = self.focus().map(|window| window.id().clone());
        let shown = focused
            .filter(|id| self.scratchpad_windows.contains(id) && !self.is_scratchpad_hidden(id))
            .or_else(|| {
                self.scratchpad_windows
                    .iter()
                    .find(|id| !self.is_scratchpad_hidden(id))
                    .cloned()
            });
        let mut target_index = window.and_then(|window| {
            self.scratchpad
                .iter()
                .position(|removed| removed.tile.window().id() == window)
        });
        if let Some(window) = window {
            if target_index.is_none() {
                let on_active_workspace = self
                    .active_workspace()
                    .is_some_and(|workspace| workspace.has_window(window));
                self.move_to_scratchpad(Some(window));
                if on_active_workspace {
                    return None;
                }
                target_index = self
                    .scratchpad
                    .iter()
                    .position(|removed| removed.tile.window().id() == window);
            }
        } else if let Some(shown) = shown {
            if self.focus().is_some_and(|focused| focused.id() == &shown) {
                self.move_to_scratchpad(Some(&shown));
                return None;
            }
            self.move_to_scratchpad(Some(&shown));
            target_index = self
                .scratchpad
                .iter()
                .position(|removed| removed.tile.window().id() == &shown);
        }

        let index = target_index.unwrap_or(0);
        self.scratchpad.get(index)?;
        let active_workspace = self.active_workspace()?.id();
        for workspace in self.workspaces_mut() {
            let disables_fullscreen = workspace.id() == active_workspace
                || workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global);
            if disables_fullscreen {
                if let Some(window) = workspace.fullscreen_window().cloned() {
                    workspace.set_fullscreen(&window, false);
                }
            }
        }

        let mut removed = self.scratchpad.remove(index)?;
        removed.is_floating = true;
        let shown = removed.tile.window().id().clone();
        let workspace = self
            .workspaces_mut()
            .find(|workspace| workspace.id() == active_workspace)
            .unwrap();
        workspace.remap_floating_position(&mut removed.tile, removed.floating_working_area);
        workspace.add_tile(
            removed.tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            removed.width,
            removed.is_full_width,
            true,
            None,
        );
        Some(shown)
    }

    pub fn scratchpad_windows(&self) -> impl Iterator<Item = &W> {
        self.scratchpad.iter().map(|removed| removed.tile.window())
    }

    pub fn scratchpad_is_empty(&self) -> bool {
        self.scratchpad_windows.is_empty()
    }

    pub fn is_scratchpad_window(&self, window: &W::Id) -> bool {
        self.scratchpad_windows.contains(window)
    }

    pub fn is_scratchpad_hidden(&self, window: &W::Id) -> bool {
        self.scratchpad
            .iter()
            .any(|removed| removed.tile.window().id() == window)
    }

    pub fn assign_sway_workspace(
        &mut self,
        target: crate::command::WorkspaceTarget,
        output_name: &str,
    ) -> Result<(), String> {
        let output = self
            .outputs()
            .find(|output| output_matches_name(output, output_name))
            .cloned()
            .ok_or_else(|| format!("unknown output '{output_name}'"))?;
        let (old_monitor, _, workspace) = self
            .workspaces()
            .find(|(_, _, workspace)| workspace_matches_target(workspace, &target))
            .ok_or_else(|| "workspace does not exist".to_owned())?;
        let old_output = old_monitor.map(|monitor| monitor.output().clone());
        self.move_workspace_to_output_by_id(workspace.id(), old_output, &output);
        Ok(())
    }

    pub fn consume_into_column(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.consume_into_column();
    }

    pub fn expel_from_column(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.expel_from_column();
    }

    pub fn swap_window_in_direction(&mut self, direction: ScrollDirection) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.swap_window_in_direction(direction);
    }

    pub fn toggle_column_tabbed_display(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.toggle_column_tabbed_display();
    }

    pub fn set_focused_layout(&mut self, layout: tiling_tree::Layout) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.set_focused_layout(layout);
        }
    }

    pub fn split_focused(&mut self, layout: tiling_tree::Layout) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.split_focused(layout);
        }
    }

    pub fn toggle_focused_layout(&mut self, toggle: &swayward_ipc::command::LayoutToggle) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.toggle_focused_layout(toggle);
        }
    }

    pub fn restore_focused_split_layout(&mut self) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.restore_focused_split_layout();
        }
    }

    pub fn toggle_focused_layout_split(&mut self) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.toggle_focused_layout_split();
        }
    }

    pub fn toggle_focused_split(&mut self) {
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.toggle_focused_split();
        }
    }

    pub fn set_column_display(&mut self, display: ColumnDisplay) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.set_column_display(display);
    }

    pub fn center_column(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.center_column();
    }

    pub fn center_window(&mut self, id: Option<&W::Id>) {
        if id.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if id.is_none() || id == Some(move_.tile.window().id()) {
                return;
            }
        }

        let workspace = if let Some(id) = id {
            Some(self.workspaces_mut().find(|ws| ws.has_window(id)).unwrap())
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.center_window(id);
    }

    pub fn center_visible_columns(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.center_visible_columns();
    }

    pub fn focus(&self) -> Option<&W> {
        self.focus_with_output().map(|(win, _out)| win)
    }

    pub fn focus_with_output(&self) -> Option<(&W, &Output)> {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            return Some((move_.tile.window(), &move_.output));
        }

        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &self.monitor_set
        else {
            return None;
        };

        let mon = &monitors[*active_monitor_idx];
        mon.active_window().map(|win| (win, &mon.output))
    }

    pub fn interactive_moved_window_under(
        &self,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> Option<(&W, HitType)> {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.output == *output {
                if self.overview_progress.is_some() {
                    let zoom = self.overview_zoom();
                    let tile_pos = move_.tile_render_location(zoom);
                    let pos_within_tile = (pos_within_output - tile_pos).downscale(zoom);
                    // During the overview animation, we cannot do input hits because we cannot
                    // really represent scaled windows properly.
                    let (win, hit) =
                        HitType::hit_tile(&move_.tile, Point::from((0., 0.)), pos_within_tile)?;
                    Some((win, hit.to_activate()))
                } else {
                    let tile_pos = move_.tile_render_location(1.);
                    HitType::hit_tile(&move_.tile, tile_pos, pos_within_output)
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Returns the window under the cursor and the hit type.
    pub fn window_under(
        &self,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> Option<(&W, HitType)> {
        let mon = self.monitor_for_output(output)?;
        mon.window_under(pos_within_output)
    }

    pub fn resize_edges_under(
        &self,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> Option<ResizeEdge> {
        let mon = self.monitor_for_output(output)?;
        mon.resize_edges_under(pos_within_output)
    }

    pub fn workspace_under(
        &self,
        extended_bounds: bool,
        output: &Output,
        pos_within_output: Point<f64, Logical>,
    ) -> Option<&Workspace<W>> {
        if self
            .interactive_moved_window_under(output, pos_within_output)
            .is_some()
        {
            return None;
        }

        let mon = self.monitor_for_output(output)?;
        if extended_bounds {
            mon.workspace_under(pos_within_output).map(|(ws, _)| ws)
        } else {
            mon.workspace_under_narrow(pos_within_output)
        }
    }

    pub fn overview_zoom(&self) -> f64 {
        let progress = self.overview_progress.as_ref().map(|p| p.value());
        compute_overview_zoom(&self.options, progress)
    }

    #[cfg(test)]
    fn verify_invariants(&self) {
        use std::collections::HashSet;

        use approx::assert_abs_diff_eq;

        let zoom = self.overview_zoom();

        let mut move_win_id = None;
        if let Some(state) = &self.interactive_move {
            match state {
                InteractiveMoveState::Starting {
                    window_id,
                    pointer_delta: _,
                    pointer_ratio_within_window: _,
                } => {
                    assert!(
                        self.has_window(window_id),
                        "interactive move must be on an existing window"
                    );
                    move_win_id = Some(window_id.clone());
                }
                InteractiveMoveState::Moving(move_) => {
                    assert_eq!(self.clock, move_.tile.clock);
                    assert!(move_.tile.window().pending_sizing_mode().is_normal());

                    move_.tile.verify_invariants();

                    let scale = move_.output.current_scale().fractional_scale();
                    let options = Options::clone(&self.options)
                        .with_merged_layout(move_.output_config.as_ref())
                        .with_merged_layout(move_.workspace_config.as_ref().map(|(_, c)| c))
                        .adjusted_for_scale(scale);
                    assert_eq!(
                        &*move_.tile.options, &options,
                        "interactive moved tile options must be \
                         base options adjusted for output scale"
                    );

                    let tile_pos = move_.tile_render_location(zoom);
                    let rounded_pos = tile_pos.to_physical_precise_round(scale).to_logical(scale);

                    // Tile position must be rounded to physical pixels.
                    assert_abs_diff_eq!(tile_pos.x, rounded_pos.x, epsilon = 1e-5);
                    assert_abs_diff_eq!(tile_pos.y, rounded_pos.y, epsilon = 1e-5);

                    if let Some(alpha) = &move_.tile.alpha_animation {
                        if move_.is_floating {
                            assert_eq!(
                                alpha.anim.to(),
                                1.,
                                "interactively moved floating tile can animate alpha only to 1"
                            );

                            assert!(
                                !alpha.hold_after_done,
                                "interactively moved floating tile \
                                 cannot have held alpha animation"
                            );
                        } else {
                            assert_ne!(
                                alpha.anim.to(),
                                1.,
                                "interactively moved scrolling tile must animate alpha to not 1"
                            );

                            assert!(
                                alpha.hold_after_done,
                                "interactively moved scrolling tile \
                                 must have held alpha animation"
                            );
                        }
                    }
                }
            }
        }

        let mut seen_workspace_id = HashSet::new();
        let mut seen_workspace_name = Vec::<String>::new();

        let (monitors, &primary_idx, &active_monitor_idx) = match &self.monitor_set {
            MonitorSet::Normal {
                monitors,
                primary_idx,
                active_monitor_idx,
            } => (monitors, primary_idx, active_monitor_idx),
            MonitorSet::NoOutputs { workspaces } => {
                for workspace in workspaces {
                    assert!(
                        workspace.must_be_kept(),
                        "with no outputs there cannot be empty unnamed workspaces"
                    );

                    assert_eq!(self.clock, workspace.clock);

                    assert_eq!(
                        workspace.base_options, self.options,
                        "workspace base options must be synchronized with layout"
                    );

                    assert!(
                        seen_workspace_id.insert(workspace.id()),
                        "workspace id must be unique"
                    );

                    if let Some(name) = &workspace.name {
                        assert!(
                            !seen_workspace_name
                                .iter()
                                .any(|n| n.eq_ignore_ascii_case(name)),
                            "workspace name must be unique"
                        );
                        seen_workspace_name.push(name.clone());
                    }

                    workspace.verify_invariants(move_win_id.as_ref());
                }

                return;
            }
        };

        assert!(primary_idx < monitors.len());
        assert!(active_monitor_idx < monitors.len());

        let mut saw_view_offset_gesture = false;

        for (idx, monitor) in monitors.iter().enumerate() {
            assert_eq!(self.clock, monitor.clock);
            assert_eq!(
                monitor.base_options, self.options,
                "monitor base options must be synchronized with layout"
            );

            assert_eq!(self.overview_open, monitor.overview_open);
            assert_eq!(
                self.overview_progress.as_ref().map(|p| p.value()),
                monitor.overview_progress_value()
            );

            monitor.verify_invariants();

            if idx == primary_idx {
                for ws in &monitor.workspaces {
                    if ws.original_output.matches(&monitor.output) {
                        // This is the primary monitor's own workspace.
                        continue;
                    }

                    let own_monitor_exists = monitors
                        .iter()
                        .any(|m| ws.original_output.matches(&m.output));
                    assert!(
                        !own_monitor_exists,
                        "primary monitor cannot have workspaces for which their own monitor exists"
                    );
                }
            } else {
                assert!(
                    monitor
                        .workspaces
                        .iter()
                        .any(|workspace| workspace.original_output.matches(&monitor.output)),
                    "secondary monitor must not have any non-own workspaces"
                );
            }

            // FIXME: verify that primary doesn't have any workspaces for which their own monitor
            // exists.

            for workspace in &monitor.workspaces {
                assert!(
                    seen_workspace_id.insert(workspace.id()),
                    "workspace id must be unique"
                );

                if let Some(name) = &workspace.name {
                    assert!(
                        !seen_workspace_name
                            .iter()
                            .any(|n| n.eq_ignore_ascii_case(name)),
                        "workspace name must be unique"
                    );
                    seen_workspace_name.push(name.clone());
                }

                workspace.verify_invariants(move_win_id.as_ref());

                let has_view_offset_gesture = workspace.tiling().has_view_offset_gesture();
                if self.dnd.is_some() || self.interactive_move.is_some() {
                    // We'd like to check that all workspaces have the gesture here, furthermore we
                    // want to check that they have the gesture only if the interactive move
                    // targets the scrolling layout. However, we cannot do that because we start
                    // and stop the gesture lazily. Otherwise the gesture code would pollute a lot
                    // of places like adding new workspaces, implicitly moving windows between
                    // floating and tiling on fullscreen, etc.
                    //
                    // assert!(
                    //     has_view_offset_gesture,
                    //     "during an interactive move in the scrolling layout, \
                    //      all workspaces should be in a view offset gesture"
                    // );
                } else if saw_view_offset_gesture {
                    assert!(
                        !has_view_offset_gesture,
                        "only one workspace can have an ongoing view offset gesture"
                    );
                }
                saw_view_offset_gesture = has_view_offset_gesture;
            }
        }
    }

    pub fn advance_animations(&mut self) {
        let _span = tracy_client::span!("Layout::advance_animations");

        let mut dnd_scroll = None;
        let mut is_dnd = false;
        if let Some(dnd) = &self.dnd {
            dnd_scroll = Some((dnd.output.clone(), dnd.pointer_pos_within_output, true));
            is_dnd = true;
        }

        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            move_.tile.advance_animations();

            if dnd_scroll.is_none() {
                dnd_scroll = Some((
                    move_.output.clone(),
                    move_.pointer_pos_within_output,
                    !move_.is_floating,
                ));
            }
        }

        let is_overview_open = self.overview_open;

        // Scroll the view if needed.
        if let Some((output, pos_within_output, is_scrolling)) = dnd_scroll {
            if let Some(mon) = self.monitor_for_output_mut(&output) {
                let mut scrolled = false;

                let zoom = mon.overview_zoom();
                scrolled |= mon.dnd_scroll_gesture_scroll(pos_within_output, 1. / zoom);

                if is_scrolling {
                    if let Some((ws, geo)) = mon.workspace_under(pos_within_output) {
                        let idx = mon.idx_of_ws(ws.id()).unwrap();
                        let ws = &mut mon.workspaces[idx];
                        // As far as the DnD scroll gesture is concerned, the workspace spans across
                        // the whole monitor horizontally.
                        let ws_pos = Point::from((0., geo.loc.y));
                        scrolled |=
                            ws.dnd_scroll_gesture_scroll(pos_within_output - ws_pos, 1. / zoom);
                    }
                }

                if scrolled {
                    // Don't trigger DnD hold while scrolling.
                    if let Some(dnd) = &mut self.dnd {
                        dnd.hold = None;
                    }
                } else if is_dnd {
                    let target = mon
                        .window_under(pos_within_output)
                        .map(|(win, _)| DndHoldTarget::Window(win.id().clone()))
                        .or_else(|| {
                            mon.workspace_under_narrow(pos_within_output)
                                .map(|ws| DndHoldTarget::Workspace(ws.id()))
                        });

                    let dnd = self.dnd.as_mut().unwrap();
                    if let Some(target) = target {
                        let now = self.clock.now_unadjusted();
                        let start_time = if let Some(hold) = &mut dnd.hold {
                            if hold.target != target {
                                hold.start_time = now;
                            }
                            hold.target = target;
                            hold.start_time
                        } else {
                            let hold = dnd.hold.insert(DndHold {
                                start_time: now,
                                target,
                            });
                            hold.start_time
                        };

                        // Delay copied from gnome-shell.
                        let delay = Duration::from_millis(750);
                        if delay <= now.saturating_sub(start_time) {
                            let hold = dnd.hold.take().unwrap();

                            // Synchronize workspace switch to overview close to get a monotonic
                            // animation.
                            let config = is_overview_open
                                .then_some(self.options.animations.overview_open_close.0);

                            let mon = self.monitor_for_output_mut(&output).unwrap();

                            let ws_idx = match hold.target {
                                DndHoldTarget::Window(id) => mon
                                    .workspaces
                                    .iter_mut()
                                    .position(|ws| ws.activate_window(&id))
                                    .unwrap(),
                                DndHoldTarget::Workspace(id) => mon.idx_of_ws(id).unwrap(),
                            };

                            mon.dnd_scroll_gesture_end();
                            mon.activate_workspace_with_anim_config(ws_idx, config);

                            self.focus_output(&output);

                            if is_overview_open {
                                self.close_overview();
                            }
                        }
                    } else {
                        // No target, reset the hold timer.
                        dnd.hold = None;
                    }
                }
            }
        }

        if let Some(OverviewProgress::Animation(anim)) = &mut self.overview_progress {
            if anim.is_done() {
                if self.overview_open {
                    self.overview_progress = Some(OverviewProgress::Open);
                } else {
                    self.overview_progress = None;
                }
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    mon.set_overview_progress(self.overview_progress.as_ref());
                    mon.advance_animations();
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    ws.advance_animations();
                }
            }
        }
    }

    pub fn are_animations_ongoing(&self, output: Option<&Output>) -> bool {
        // Keep advancing animations if we might need to scroll the view.
        if let Some(dnd) = &self.dnd {
            if output.is_none_or(|output| *output == dnd.output) {
                return true;
            }
        }

        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if output.is_none_or(|output| *output == move_.output) {
                if move_.tile.are_animations_ongoing() {
                    return true;
                }

                // Keep advancing animations if we might need to scroll the view.
                if !move_.is_floating || self.overview_open {
                    return true;
                }
            }
        }

        if self
            .overview_progress
            .as_ref()
            .is_some_and(|p| p.is_animation())
        {
            return true;
        }

        for mon in self.monitors() {
            if output.is_some_and(|output| mon.output != *output) {
                continue;
            }

            if mon.are_animations_ongoing() {
                return true;
            }
        }

        false
    }

    pub fn update_render_elements(&mut self, output: Option<&Output>) {
        let _span = tracy_client::span!("Layout::update_render_elements");

        self.update_render_elements_time = self.clock.now();

        let zoom = self.overview_zoom();
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if output.is_none_or(|output| move_.output == *output) {
                let pos_within_output = move_.tile_render_location(zoom);

                // We're not on any specific workspace so we can't compute a "workspace view" rect.
                // Let's instead compute a rect relative to the output.
                //
                // FIXME: we could make the colors match up better in the overview by figuring out
                // where a centered workspace would currently be, and computing the view rect
                // against that. Since most of the time the dragged window will be on a centered
                // workspace.
                let view_rect =
                    Rectangle::new(pos_within_output.upscale(-1.), output_size(&move_.output))
                        .downscale(zoom);

                move_.tile.update_render_elements(true, view_rect);
            }
        }

        self.update_insert_hint(output);

        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            if output.is_some() {
                error!("update_render_elements called with no monitors but Some output");
            }
            return;
        };

        for (idx, mon) in monitors.iter_mut().enumerate() {
            if output.is_none_or(|output| mon.output == *output) {
                let is_active = self.is_active
                    && idx == *active_monitor_idx
                    && !matches!(self.interactive_move, Some(InteractiveMoveState::Moving(_)));
                mon.set_overview_progress(self.overview_progress.as_ref());
                mon.update_render_elements(is_active);
            }
        }
    }

    pub fn update_shaders(&mut self) {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            move_.tile.update_shaders();
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    mon.update_shaders();
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    ws.update_shaders();
                }
            }
        }
    }

    fn update_insert_hint(&mut self, output: Option<&Output>) {
        let _span = tracy_client::span!("Layout::update_insert_hint");

        for mon in self.monitors_mut() {
            mon.insert_hint = None;
        }

        if !matches!(self.interactive_move, Some(InteractiveMoveState::Moving(_))) {
            return;
        }
        let Some(InteractiveMoveState::Moving(move_)) = self.interactive_move.take() else {
            unreachable!()
        };
        if output.is_some_and(|out| &move_.output != out) {
            self.interactive_move = Some(InteractiveMoveState::Moving(move_));
            return;
        }

        let _span = tracy_client::span!("Layout::update_insert_hint::update");

        if let Some(mon) = self.monitor_for_output_mut(&move_.output) {
            let zoom = mon.overview_zoom();
            let (insert_ws, geo) = mon.insert_position(move_.pointer_pos_within_output);
            match insert_ws {
                InsertWorkspace::Existing(ws_id) => {
                    let idx = mon.idx_of_ws(ws_id).unwrap();
                    let ws = &mut mon.workspaces[idx];
                    let pos_within_workspace =
                        (move_.pointer_pos_within_output - geo.loc).downscale(zoom);
                    let position = if move_.is_floating {
                        InsertPosition::Floating
                    } else {
                        ws.scrolling_insert_position(pos_within_workspace)
                    };

                    let border_width = move_.tile.effective_border_width().unwrap_or(0.);
                    let corner_radius = move_
                        .tile
                        .window()
                        .geometry_corner_radius()
                        .expanded_by(border_width as f32);
                    mon.insert_hint = Some(InsertHint {
                        workspace: insert_ws,
                        position,
                        corner_radius,
                    });
                }
                InsertWorkspace::Preview(_) => {
                    let position = if move_.is_floating {
                        InsertPosition::Floating
                    } else {
                        InsertPosition::NewColumn(0)
                    };
                    mon.insert_hint = Some(InsertHint {
                        workspace: insert_ws,
                        position,
                        corner_radius: CornerRadius::default(),
                    });
                }
            }
        }

        self.interactive_move = Some(InteractiveMoveState::Moving(move_));
    }

    pub fn ensure_named_workspace(&mut self, ws_config: &WorkspaceConfig) {
        if self.find_workspace_by_name(&ws_config.name.0).is_some() {
            return;
        }

        let clock = self.clock.clone();
        let options = self.options.clone();

        match &mut self.monitor_set {
            MonitorSet::Normal {
                monitors,
                primary_idx,
                active_monitor_idx,
            } => {
                let mon_idx = ws_config
                    .sway_output_assignment
                    .as_ref()
                    .and_then(|outputs| {
                        outputs.iter().find_map(|name| {
                            monitors
                                .iter()
                                .position(|monitor| output_matches_name(&monitor.output, name))
                        })
                    })
                    .or_else(|| {
                        ws_config.open_on_output.as_deref().map(|name| {
                            monitors
                                .iter()
                                .position(|monitor| output_matches_name(&monitor.output, name))
                                .unwrap_or(*primary_idx)
                        })
                    })
                    .unwrap_or(*active_monitor_idx);
                let mon = &mut monitors[mon_idx];

                let ws = Workspace::new_with_config(
                    mon.output.clone(),
                    Some(ws_config.clone()),
                    clock,
                    options,
                );
                mon.insert_workspace(ws, 0, false);
                mon.reap_empty_workspaces();
            }
            MonitorSet::NoOutputs { workspaces } => {
                let ws =
                    Workspace::new_with_config_no_outputs(Some(ws_config.clone()), clock, options);
                workspaces.insert(0, ws);
            }
        }
    }

    pub fn update_config(&mut self, config: &Config) {
        self.initial_workspace_names = initial_workspace_names(config);
        self.workspace_configs = config.workspaces.clone();

        // Update workspace-specific config for all named workspaces.
        for ws in self.workspaces_mut() {
            let workspace_config = ws.sway_name().and_then(|name| {
                config
                    .workspaces
                    .iter()
                    .find(|candidate| candidate.name.0 == name)
            });
            ws.update_layout_config(
                workspace_config.and_then(|config| config.layout.clone().map(|x| x.0)),
            );
        }

        self.update_options(Options::from_config(config));
    }

    fn update_options(&mut self, options: Options) {
        let options = Rc::new(options);

        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            let view_size = output_size(&move_.output);
            let scale = move_.output.current_scale().fractional_scale();
            let options = Options::clone(&options)
                .with_merged_layout(move_.output_config.as_ref())
                .with_merged_layout(move_.workspace_config.as_ref().map(|(_, c)| c))
                .adjusted_for_scale(scale);
            move_.tile.update_config(view_size, scale, Rc::new(options));
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    mon.update_config(options.clone());
                }
            }
            MonitorSet::NoOutputs { workspaces } => {
                for ws in workspaces {
                    ws.update_config(options.clone());
                }
            }
        }

        self.options = options;
    }

    pub fn toggle_width(&mut self, forwards: bool) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.toggle_width(forwards);
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.toggle_window_width(window, forwards);
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.toggle_window_height(window, forwards);
    }

    pub fn toggle_full_width(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.toggle_full_width();
    }

    pub fn set_column_width(&mut self, change: SizeChange) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.set_column_width(change);
    }

    pub fn set_window_width(&mut self, window: Option<&W::Id>, change: SizeChange) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let automatic_maximum = self.output_layout_size();
        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.set_window_width(window, change, automatic_maximum);
    }

    pub fn set_tiling_node_size_sway(
        &mut self,
        workspace_id: workspace::WorkspaceId,
        node: tiling_tree::NodeId,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) {
        if let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.id() == workspace_id)
        {
            workspace.set_tiling_node_size_sway(node, width, height);
        }
    }

    pub fn set_window_size_sway(
        &mut self,
        window: &W::Id,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) {
        if self.is_scratchpad_hidden(window) {
            return;
        }
        let automatic_maximum = self.output_layout_size();
        let Some(workspace) = self.workspaces_mut().find(|ws| ws.has_window(window)) else {
            return;
        };
        workspace.set_window_size_sway(window, width, height, automatic_maximum);
    }

    pub fn set_window_height(&mut self, window: Option<&W::Id>, change: SizeChange) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let automatic_maximum = self.output_layout_size();
        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.set_window_height(window, change, automatic_maximum);
    }

    pub fn resize_window_edge(
        &mut self,
        window: Option<&W::Id>,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> Option<bool> {
        let workspace = if let Some(window) = window {
            self.workspaces_mut().find(|ws| ws.has_window(window))
        } else {
            self.active_workspace_mut()
        };
        workspace.and_then(|workspace| workspace.resize_window_edge(window, edge, change))
    }

    pub fn reset_window_height(&mut self, window: Option<&W::Id>) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.reset_window_height(window);
    }

    pub fn expand_column_to_available_width(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.expand_column_to_available_width();
    }

    pub fn toggle_window_floating(&mut self, window: Option<&W::Id>) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                move_.is_floating = !move_.is_floating;

                // When going to floating, restore the floating window size.
                if move_.is_floating {
                    let floating_size = move_.tile.floating_window_size;
                    let win = move_.tile.window_mut();
                    let mut size = floating_size.unwrap_or_else(|| win.natural_size());

                    // Apply min/max size window rules. If requesting a concrete size, apply
                    // completely; if requesting (0, 0), apply only when min/max results in a fixed
                    // size.
                    let min_size = win.min_size();
                    let max_size = win.max_size();
                    size.w = ensure_min_max_size_maybe_zero(size.w, min_size.w, max_size.w);
                    size.h = ensure_min_max_size_maybe_zero(size.h, min_size.h, max_size.h);

                    win.request_size_once(size, true);

                    // Animate the tile back to opaque.
                    move_.tile.animate_alpha(
                        INTERACTIVE_MOVE_ALPHA,
                        1.,
                        self.options.animations.window_movement.0,
                    );

                    // Unlock the view on the workspaces.
                    for ws in self.workspaces_mut() {
                        ws.dnd_scroll_gesture_end();
                    }
                } else {
                    // Animate the tile back to semitransparent.
                    move_.tile.animate_alpha(
                        1.,
                        INTERACTIVE_MOVE_ALPHA,
                        self.options.animations.window_movement.0,
                    );
                    move_.tile.hold_alpha_animation_after_done();
                }

                return;
            }
        }

        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.toggle_window_floating(window);
    }

    pub fn set_window_floating(&mut self, window: Option<&W::Id>, floating: bool) {
        if window.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                if move_.is_floating != floating {
                    self.toggle_window_floating(window);
                }
                return;
            }
        }

        let workspace = if let Some(window) = window {
            Some(
                self.workspaces_mut()
                    .find(|ws| ws.has_window(window))
                    .unwrap(),
            )
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.set_window_floating(window, floating);
    }

    pub fn focus_floating(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_floating();
    }

    pub fn focus_tiling(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.focus_tiling();
    }

    pub fn switch_focus_floating_tiling(&mut self) {
        let Some(workspace) = self.active_workspace_mut() else {
            return;
        };
        workspace.switch_focus_floating_tiling();
    }

    pub fn move_floating_window(
        &mut self,
        id: Option<&W::Id>,
        x: PositionChange,
        y: PositionChange,
        animate: bool,
    ) {
        if id.is_some_and(|window| self.is_scratchpad_hidden(window)) {
            return;
        }
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if id.is_none() || id == Some(move_.tile.window().id()) {
                return;
            }
        }

        let workspace = if let Some(id) = id {
            Some(self.workspaces_mut().find(|ws| ws.has_window(id)).unwrap())
        } else {
            self.active_workspace_mut()
        };

        let Some(workspace) = workspace else {
            return;
        };
        workspace.move_floating_window(id, x, y, animate);
    }

    pub fn focus_output(&mut self, output: &Output) {
        let active_needs_identity = self
            .active_workspace()
            .is_some_and(|workspace| !workspace.has_sway_identity());
        if active_needs_identity {
            let (name, number) = self.next_free_workspace_identity();
            self.active_workspace_mut()
                .unwrap()
                .set_sway_identity(name, number);
        }
        let target_identity = self.next_free_workspace_identity();
        if let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        {
            for (idx, mon) in monitors.iter_mut().enumerate() {
                if &mon.output == output {
                    let workspace = mon.active_workspace();
                    if !workspace.has_sway_identity() {
                        workspace.set_sway_identity(target_identity.0, target_identity.1);
                    }
                    *active_monitor_idx = idx;
                    return;
                }
            }
        }
    }

    pub fn move_to_output(
        &mut self,
        window: Option<&W::Id>,
        output: &Output,
        target_ws_idx: Option<usize>,
        activate: ActivateWindow,
    ) {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        if let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        {
            let new_idx = monitors
                .iter()
                .position(|mon| &mon.output == output)
                .unwrap();

            let (mon_idx, ws_idx) = if let Some(window) = window {
                monitors
                    .iter()
                    .enumerate()
                    .find_map(|(mon_idx, mon)| {
                        mon.workspaces
                            .iter()
                            .position(|ws| ws.has_window(window))
                            .map(|ws_idx| (mon_idx, ws_idx))
                    })
                    .unwrap()
            } else {
                let mon_idx = *active_monitor_idx;
                let mon = &monitors[mon_idx];
                (mon_idx, mon.active_workspace_idx)
            };

            let workspace_idx = target_ws_idx.unwrap_or(monitors[new_idx].active_workspace_idx);
            if mon_idx == new_idx && ws_idx == workspace_idx {
                return;
            }
            let source_ws_id = monitors[mon_idx].workspaces[ws_idx].id();

            let destination_output = monitors[new_idx].output.clone();
            let Some(ws_id) = self.prepare_workspace_at(&destination_output, workspace_idx) else {
                return;
            };

            let MonitorSet::Normal {
                monitors,
                active_monitor_idx,
                ..
            } = &mut self.monitor_set
            else {
                unreachable!()
            };
            let new_idx = monitors
                .iter()
                .position(|mon| mon.output == destination_output)
                .unwrap();
            let ws_idx = monitors[mon_idx].idx_of_ws(source_ws_id).unwrap();

            let mon = &mut monitors[mon_idx];
            let activate = activate.map_smart(|| {
                window.is_none_or(|win| {
                    mon_idx == *active_monitor_idx
                        && mon.active_window().map(|win| win.id()) == Some(win)
                })
            });
            let activate = if activate {
                ActivateWindow::Yes
            } else {
                ActivateWindow::No
            };

            let ws = &mut mon.workspaces[ws_idx];
            let Some(window) = window.or_else(|| ws.active_window().map(|win| win.id())) else {
                return;
            };
            let window = window.clone();

            let transaction = Transaction::new();
            let mut removed = ws.remove_tile(&window, transaction);

            removed.tile.stop_move_animations();

            let mon = &mut monitors[new_idx];
            mon.add_tile(
                removed.tile,
                MonitorAddWindowTarget::Workspace {
                    id: ws_id,
                    column_idx: None,
                },
                activate,
                true,
                removed.width,
                removed.is_full_width,
                removed.is_floating,
                None,
            );
            if activate.map_smart(|| false) {
                *active_monitor_idx = new_idx;
            }

            let mon = &mut monitors[mon_idx];
            if mon.workspace_switch.is_none() {
                monitors[mon_idx].clean_up_workspaces();
            }
            if let Some(workspace_idx) = monitors[new_idx].idx_of_ws(ws_id) {
                monitors[new_idx].workspaces[workspace_idx].sort_tiling_focus_by_timestamp();
            }
        }
    }

    pub fn move_column_to_output(
        &mut self,
        output: &Output,
        target_ws_idx: Option<usize>,
        activate: bool,
    ) {
        let Some((source_workspace, floating)) = self
            .active_workspace()
            .map(|workspace| (workspace.id(), workspace.floating_is_active()))
        else {
            return;
        };
        if floating {
            self.move_to_output(None, output, None, ActivateWindow::Smart);
            return;
        }
        if self
            .active_workspace()
            .and_then(Workspace::active_window)
            .is_none()
        {
            return;
        }

        let target_index = target_ws_idx.or_else(|| {
            self.monitor_for_output(output)
                .map(Monitor::active_workspace_idx)
        });
        let Some(target) = target_index.and_then(|index| self.prepare_workspace_at(output, index))
        else {
            return;
        };

        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return;
        };
        let source_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(source_workspace))
            .unwrap();
        let target_monitor = monitors
            .iter()
            .position(|monitor| monitor.has_ws(target))
            .unwrap();
        if source_monitor == target_monitor {
            monitors[source_monitor].move_column_to_workspace(target, activate);
            return;
        }

        let source_idx = monitors[source_monitor]
            .idx_of_ws(source_workspace)
            .unwrap();
        let Some(tile) =
            monitors[source_monitor].workspaces[source_idx].remove_active_tiling_tile()
        else {
            return;
        };
        let target_idx = monitors[target_monitor].idx_of_ws(target).unwrap();
        monitors[target_monitor].add_tiling_tile(target_idx, tile, activate);
        if activate {
            *active_monitor_idx = target_monitor;
        }
        if monitors[source_monitor].workspace_switch.is_none() {
            monitors[source_monitor].clean_up_workspaces();
        }
    }

    pub fn move_workspace_to_output(&mut self, output: &Output) -> bool {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &self.monitor_set
        else {
            return false;
        };

        let id = monitors[*active_monitor_idx].active_workspace_ref().id();
        self.move_workspace_to_output_by_id(id, None, output)
    }

    pub fn move_workspace_to_output_by_id(
        &mut self,
        workspace_id: WorkspaceId,
        old_output: Option<Output>,
        new_output: &Output,
    ) -> bool {
        // Name the replacement workspace the way a newly enabled output would
        // be named, so an output vacated by this move gets back the workspace
        // its `workspace <name> output <output>` assignment claims rather than
        // a bare free number. Sway re-runs workspace_next_name for the same
        // reason when a workspace leaves an output.
        let replacement_identity = old_output
            .as_ref()
            .and_then(|output| self.next_initial_workspace_name_for_output(Some(output)))
            .map(|name| {
                sway_workspace_identity(crate::command::WorkspaceTarget::Name(name)).unwrap()
            })
            .unwrap_or_else(|| self.next_free_workspace_identity_for_output(old_output.as_ref()));
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &mut self.monitor_set
        else {
            return false;
        };

        let current_idx = if let Some(old_output) = &old_output {
            monitors
                .iter()
                .position(|mon| mon.output == *old_output)
                .unwrap()
        } else {
            *active_monitor_idx
        };
        let target_idx = monitors
            .iter()
            .position(|mon| mon.output == *new_output)
            .unwrap();

        let Some(old_idx) = monitors[current_idx].idx_of_ws(workspace_id) else {
            return false;
        };

        // Do not do anything if the output is already correct.
        if current_idx == target_idx {
            let current = &mut monitors[current_idx];
            current.workspaces[old_idx].original_output = OutputId::new(&current.output);
            return false;
        }

        let source_was_active = workspace_id == monitors[current_idx].active_workspace_ref().id();
        let activate = current_idx == *active_monitor_idx && source_was_active;
        let moved_workspace_is_empty = !monitors[current_idx].workspaces[old_idx].has_windows();

        let source_replacement = if (old_output.is_some() || moved_workspace_is_empty)
            && monitors[current_idx]
                .workspaces
                .iter()
                .filter(|workspace| workspace.id() == workspace_id || workspace.must_be_kept())
                .count()
                == 1
        {
            let (name, number) = replacement_identity;
            Some(
                if let Some(workspace) =
                    monitors[current_idx]
                        .workspaces
                        .iter_mut()
                        .find(|workspace| {
                            workspace.id() != workspace_id
                                && !workspace.has_sway_identity()
                                && !workspace.has_windows()
                        })
                {
                    workspace.set_sway_identity(name, number);
                    workspace.id()
                } else {
                    monitors[current_idx].add_sway_workspace_at(1, name, number)
                },
            )
        } else {
            None
        };

        let Some(mut ws) = monitors[current_idx].detach_workspace(workspace_id) else {
            return false;
        };
        monitors[current_idx].reap_empty_workspaces();
        if source_was_active {
            monitors[current_idx].active_workspace_idx = source_replacement
                .and_then(|id| monitors[current_idx].idx_of_ws(id))
                .unwrap_or(monitors[current_idx].workspaces.len() - 1);
        }
        ws.original_output = OutputId::new(new_output);

        let target_active = monitors[target_idx].active_workspace_ref().id();
        let insert_idx = monitors[target_idx].active_workspace_idx + 1;
        monitors[target_idx].attach_workspace(ws, insert_idx, activate);
        monitors[target_idx].sort_sway_workspaces();
        if !activate {
            if let Some(active_idx) = monitors[target_idx].idx_of_ws(target_active) {
                monitors[target_idx].active_workspace_idx = active_idx;
            }
        }

        monitors[current_idx].clean_up_workspaces();
        monitors[target_idx].clean_up_workspaces();
        monitors[target_idx].reap_empty_workspaces();

        if activate {
            *active_monitor_idx = target_idx;
        }

        activate
    }

    pub fn focused_fullscreen_mode(&self) -> Option<tiling_tree::FullscreenMode> {
        self.active_workspace().and_then(Workspace::fullscreen_mode)
    }

    pub fn global_fullscreen_active(&self) -> bool {
        self.workspaces().any(|(_, _, workspace)| {
            workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global)
        })
    }

    pub fn focused_window_is_fullscreen_or_child(&self) -> bool {
        let Some(window) = self.focus() else {
            return false;
        };
        self.active_workspace()
            .is_some_and(|workspace| workspace.fullscreen_contains_window(window.id()))
    }

    pub fn disable_active_workspace_fullscreen(&mut self) {
        let window = self.active_workspace().and_then(|workspace| {
            let fullscreen = workspace.tiling().fullscreen_node()?;
            workspace.tiling().windows().find_map(|(id, window)| {
                workspace
                    .tiling()
                    .contains_node(fullscreen, id)
                    .then(|| window.id().clone())
            })
        });
        if let (Some(workspace), Some(window)) = (self.active_workspace_mut(), window) {
            workspace.set_fullscreen(&window, false);
        }
    }

    pub fn set_focused_fullscreen_mode(&mut self, mode: Option<tiling_tree::FullscreenMode>) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    let node = workspace.tiling().fullscreen_node().unwrap();
                    workspace.tiling_mut().set_node_fullscreen(node, None);
                    break;
                }
            }
        }
        if let Some(workspace) = self.active_workspace_mut() {
            workspace.set_focused_fullscreen(mode);
        }
    }

    pub fn fullscreen_mode(&self, id: &W::Id) -> Option<tiling_tree::FullscreenMode> {
        self.workspaces()
            .find(|(_, _, workspace)| workspace.has_window(id))
            .and_then(|(_, _, workspace)| workspace.fullscreen_mode())
    }

    pub fn set_fullscreen_mode(&mut self, id: &W::Id, mode: Option<tiling_tree::FullscreenMode>) {
        if mode.is_some() {
            for workspace in self.workspaces_mut() {
                if workspace.fullscreen_mode() == Some(tiling_tree::FullscreenMode::Global) {
                    let node = workspace.tiling().fullscreen_node().unwrap();
                    workspace.tiling_mut().set_node_fullscreen(node, None);
                    break;
                }
            }
        }
        if mode == Some(tiling_tree::FullscreenMode::Global) {
            self.activate_window(id);
        }
        if let Some(workspace) = self
            .workspaces_mut()
            .find(|workspace| workspace.has_window(id))
        {
            workspace.activate_window(id);
            if workspace.is_floating(id) {
                workspace.set_fullscreen(id, mode.is_some());
                workspace.activate_window(id);
                if mode == Some(tiling_tree::FullscreenMode::Global) {
                    workspace.set_focused_fullscreen(mode);
                }
            } else {
                workspace.set_focused_fullscreen(mode);
            }
        }
    }

    pub fn set_fullscreen(&mut self, id: &W::Id, is_fullscreen: bool) {
        // Check if this is a request to unset the windowed fullscreen state.
        if !is_fullscreen {
            let mut handled = false;
            self.with_windows_mut(|window, _| {
                if window.id() == id && window.is_pending_windowed_fullscreen() {
                    window.request_windowed_fullscreen(false);
                    handled = true;
                }
            });
            if handled {
                return;
            }
        }

        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == id {
                return;
            }
        }

        for ws in self.workspaces_mut() {
            if ws.has_window(id) {
                ws.set_fullscreen(id, is_fullscreen);
                return;
            }
        }
    }

    pub fn toggle_fullscreen(&mut self, id: &W::Id) {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == id {
                return;
            }
        }

        for ws in self.workspaces_mut() {
            if ws.has_window(id) {
                ws.toggle_fullscreen(id);
                return;
            }
        }
    }

    pub fn toggle_windowed_fullscreen(&mut self, id: &W::Id) {
        let (_, window) = self.windows().find(|(_, win)| win.id() == id).unwrap();
        if window.pending_sizing_mode().is_fullscreen() {
            // Remove the real fullscreen.
            for ws in self.workspaces_mut() {
                if ws.has_window(id) {
                    ws.set_fullscreen(id, false);
                    break;
                }
            }
        }

        // This will switch is_pending_fullscreen() to false right away.
        self.with_windows_mut(|window, _| {
            if window.id() == id {
                window.request_windowed_fullscreen(!window.is_pending_windowed_fullscreen());
            }
        });
    }

    pub fn set_maximized(&mut self, id: &W::Id, maximize: bool) {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == id {
                return;
            }
        }

        for ws in self.workspaces_mut() {
            if ws.has_window(id) {
                ws.set_maximized(id, maximize);
                return;
            }
        }
    }

    pub fn toggle_maximized(&mut self, id: &W::Id) {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == id {
                return;
            }
        }

        for ws in self.workspaces_mut() {
            if ws.has_window(id) {
                ws.toggle_maximized(id);
                return;
            }
        }
    }

    pub fn view_offset_gesture_begin(
        &mut self,
        output: &Output,
        workspace_idx: Option<usize>,
        is_touchpad: bool,
    ) {
        let monitors = match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => monitors,
            MonitorSet::NoOutputs { .. } => unreachable!(),
        };

        for monitor in monitors {
            for (idx, ws) in monitor.workspaces.iter_mut().enumerate() {
                // Cancel the gesture on other workspaces.
                if &monitor.output != output
                    || idx != workspace_idx.unwrap_or(monitor.active_workspace_idx)
                {
                    ws.view_offset_gesture_end(None);
                    continue;
                }

                ws.view_offset_gesture_begin(is_touchpad);
            }
        }
    }

    pub fn view_offset_gesture_update(
        &mut self,
        delta_x: f64,
        timestamp: Duration,
        is_touchpad: bool,
    ) -> Option<Option<Output>> {
        let zoom = self.overview_zoom();
        let delta_x = delta_x / zoom;

        let monitors = match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => monitors,
            MonitorSet::NoOutputs { .. } => return None,
        };

        for monitor in monitors {
            for ws in &mut monitor.workspaces {
                if let Some(refresh) =
                    ws.view_offset_gesture_update(delta_x, timestamp, is_touchpad)
                {
                    if refresh {
                        return Some(Some(monitor.output.clone()));
                    } else {
                        return Some(None);
                    }
                }
            }
        }

        None
    }

    pub fn view_offset_gesture_end(&mut self, is_touchpad: Option<bool>) -> Option<Output> {
        let monitors = match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => monitors,
            MonitorSet::NoOutputs { .. } => return None,
        };

        for monitor in monitors {
            for ws in &mut monitor.workspaces {
                if ws.view_offset_gesture_end(is_touchpad) {
                    return Some(monitor.output.clone());
                }
            }
        }

        None
    }

    pub fn overview_gesture_begin(&mut self) {
        self.overview_open = true;

        let value = self.overview_progress.take().map_or(0., |p| p.value());
        let gesture = OverviewGesture {
            tracker: SwipeTracker::new(),
            start: value,
            value,
        };
        self.overview_progress = Some(OverviewProgress::Gesture(gesture));

        self.set_monitors_overview_state();
    }

    pub fn overview_gesture_update(&mut self, delta_y: f64, timestamp: Duration) -> Option<bool> {
        let Some(OverviewProgress::Gesture(gesture)) = &mut self.overview_progress else {
            return None;
        };

        gesture.tracker.push(delta_y, timestamp);

        let total_height = OVERVIEW_GESTURE_MOVEMENT;
        let pos = gesture.tracker.pos() / total_height;
        let new_value = gesture.start + pos;
        let new_value = OVERVIEW_GESTURE_RUBBER_BAND.clamp(0., 1., new_value);

        if gesture.value == new_value {
            return Some(false);
        }

        gesture.value = new_value;
        self.set_monitors_overview_state();

        Some(true)
    }

    pub fn overview_gesture_end(&mut self) -> bool {
        let Some(OverviewProgress::Gesture(gesture)) = &mut self.overview_progress else {
            return false;
        };

        // Take into account any idle time between the last event and now.
        let now = self.clock.now_unadjusted();
        gesture.tracker.push(0., now);

        let total_height = OVERVIEW_GESTURE_MOVEMENT;

        let mut velocity = gesture.tracker.velocity() / total_height;
        let current_pos = gesture.tracker.pos() / total_height;
        let pos = gesture.tracker.projected_end_pos() / total_height;

        let new_value = gesture.start + pos;
        let new_value = new_value.clamp(0., 1.).round();

        velocity *=
            OVERVIEW_GESTURE_RUBBER_BAND.clamp_derivative(0., 1., gesture.start + current_pos);

        self.overview_open = new_value == 1.;
        self.overview_progress = Some(OverviewProgress::Animation(Animation::new(
            self.clock.clone(),
            gesture.value,
            new_value,
            velocity,
            self.options.animations.overview_open_close.0,
        )));

        self.set_monitors_overview_state();

        true
    }

    pub fn interactive_move_begin(
        &mut self,
        window_id: W::Id,
        output: &Output,
        start_pos_within_output: Point<f64, Logical>,
    ) -> bool {
        if self.interactive_move.is_some() {
            return false;
        }

        let Some((mon, (ws, ws_geo))) = self.monitors().find_map(|mon| {
            mon.workspaces_with_render_geo()
                .find(|(ws, _)| ws.has_window(&window_id))
                .map(|rv| (mon, rv))
        }) else {
            return false;
        };

        if mon.output() != output {
            return false;
        }

        let zoom = mon.overview_zoom();

        let is_floating = ws.is_floating(&window_id);
        let (tile, tile_offset, _visible) = ws
            .tiles_with_render_positions()
            .find(|(tile, _, _)| tile.window().id() == &window_id)
            .unwrap();
        let window_offset = tile.window_loc();

        let tile_pos = ws_geo.loc + tile_offset.upscale(zoom);

        let pointer_offset_within_window =
            start_pos_within_output - tile_pos - window_offset.upscale(zoom);
        let window_size = tile.window_size().upscale(zoom);
        let pointer_ratio_within_window = (
            f64::clamp(pointer_offset_within_window.x / window_size.w, 0., 1.),
            f64::clamp(pointer_offset_within_window.y / window_size.h, 0., 1.),
        );

        self.interactive_move = Some(InteractiveMoveState::Starting {
            window_id,
            pointer_delta: Point::from((0., 0.)),
            pointer_ratio_within_window,
        });

        for mon in self.monitors_mut() {
            mon.dnd_scroll_gesture_begin();
        }

        // Lock the view for scrolling interactive move.
        if !is_floating {
            for ws in self.workspaces_mut() {
                ws.dnd_scroll_gesture_begin();
            }
        }

        true
    }

    pub fn interactive_move_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
        output: Output,
        pointer_pos_within_output: Point<f64, Logical>,
    ) -> bool {
        let Some(state) = self.interactive_move.take() else {
            return false;
        };

        match state {
            InteractiveMoveState::Starting {
                window_id,
                mut pointer_delta,
                pointer_ratio_within_window,
            } => {
                if window_id != *window {
                    self.interactive_move = Some(InteractiveMoveState::Starting {
                        window_id,
                        pointer_delta,
                        pointer_ratio_within_window,
                    });
                    return false;
                }

                let zoom = self.overview_zoom();
                let delta = delta.downscale(zoom);

                pointer_delta += delta;

                let (cx, cy) = (pointer_delta.x, pointer_delta.y);
                let sq_dist = cx * cx + cy * cy;

                let factor = RubberBand {
                    stiffness: 1.0,
                    limit: 0.5,
                }
                .band(sq_dist / INTERACTIVE_MOVE_START_THRESHOLD);

                let (is_floating, source_workspace, tile, workspace_config) = self
                    .workspaces_mut()
                    .find(|ws| ws.has_window(&window_id))
                    .map(|ws| {
                        let workspace_config = ws.layout_config().cloned().map(|c| (ws.id(), c));
                        (
                            ws.is_floating(&window_id),
                            ws.id(),
                            ws.tiles_mut()
                                .find(|tile| *tile.window().id() == window_id)
                                .unwrap(),
                            workspace_config,
                        )
                    })
                    .unwrap();
                tile.interactive_move_offset = pointer_delta.upscale(factor);

                // Put it back to be able to easily return.
                self.interactive_move = Some(InteractiveMoveState::Starting {
                    window_id: window_id.clone(),
                    pointer_delta,
                    pointer_ratio_within_window,
                });

                if !is_floating && sq_dist < INTERACTIVE_MOVE_START_THRESHOLD {
                    return true;
                }

                let output_config = self
                    .monitors()
                    .find(|mon| mon.output() == &output)
                    .and_then(|mon| mon.layout_config().cloned());

                // If the pointer is currently on the window's own output, then we can animate the
                // window movement from its current (rubberbanded and possibly moved away) position
                // to the pointer. Otherwise, we just teleport it as the layout code is not aware
                // of monitor positions.
                //
                // FIXME: when and if the layout code knows about monitor positions, this will be
                // potentially animatable.
                let mut tile_pos = None;
                if let Some((mon, (ws, ws_geo))) = self.monitors().find_map(|mon| {
                    mon.workspaces_with_render_geo()
                        .find(|(ws, _)| ws.has_window(window))
                        .map(|rv| (mon, rv))
                }) {
                    if mon.output() == &output {
                        let (_, tile_offset, _) = ws
                            .tiles_with_render_positions()
                            .find(|(tile, _, _)| tile.window().id() == window)
                            .unwrap();

                        let zoom = mon.overview_zoom();
                        tile_pos = Some((ws_geo.loc + tile_offset.upscale(zoom), zoom));
                    }
                }

                // Unset fullscreen before removing the tile. This will restore its size properly,
                // and move it to floating if needed, so we don't have to deal with that here.
                // Keep the emptied source attached until the drop has attached the tile elsewhere.
                let ws = self
                    .workspaces_mut()
                    .find(|ws| ws.has_window(&window_id))
                    .unwrap();
                ws.set_fullscreen(window, false);
                ws.set_maximized(window, false);

                let RemovedTile {
                    mut tile,
                    width,
                    is_full_width,
                    is_floating,
                    floating_working_area: _,
                } = ws.remove_tile(window, Transaction::new());

                tile.stop_move_animations();
                tile.interactive_move_offset = Point::from((0., 0.));
                tile.window().output_enter(&output);
                tile.window().set_preferred_scale_transform(
                    output.current_scale(),
                    output.current_transform(),
                );

                let view_size = output_size(&output);
                let scale = output.current_scale().fractional_scale();
                let options = Options::clone(&self.options)
                    .with_merged_layout(output_config.as_ref())
                    .with_merged_layout(workspace_config.as_ref().map(|(_, c)| c))
                    .adjusted_for_scale(scale);
                tile.update_config(view_size, scale, Rc::new(options));

                if is_floating {
                    // Unlock the view in case we locked it moving a fullscreen window that is
                    // going to unfullscreen to floating.
                    for ws in self.workspaces_mut() {
                        ws.dnd_scroll_gesture_end();
                    }
                } else {
                    // Animate to semitransparent.
                    tile.animate_alpha(
                        1.,
                        INTERACTIVE_MOVE_ALPHA,
                        self.options.animations.window_movement.0,
                    );
                    tile.hold_alpha_animation_after_done();
                }

                if tile_pos.is_none() {
                    self.focus_output(&output);
                }

                let mut data = InteractiveMoveData {
                    tile,
                    output,
                    pointer_pos_within_output,
                    width,
                    is_full_width,
                    is_floating,
                    source_workspace,
                    pointer_ratio_within_window,
                    output_config,
                    workspace_config,
                };

                if let Some((tile_pos, zoom)) = tile_pos {
                    let new_tile_pos = data.tile_render_location(zoom);
                    data.tile
                        .animate_move_from((tile_pos - new_tile_pos).downscale(zoom));
                }

                self.interactive_move = Some(InteractiveMoveState::Moving(data));
            }
            InteractiveMoveState::Moving(mut move_) => {
                if window != move_.tile.window().id() {
                    self.interactive_move = Some(InteractiveMoveState::Moving(move_));
                    return false;
                }

                let mut ws_id = None;
                if let Some(mon) = self.monitor_for_output(&output) {
                    let (insert_ws, _) = mon.insert_position(move_.pointer_pos_within_output);
                    if let InsertWorkspace::Existing(id) = insert_ws {
                        ws_id = Some(id);
                    }
                }

                // If moved over a different workspace, reset the config override.
                let mut update_config = false;
                if let Some((id, _)) = &move_.workspace_config {
                    if Some(*id) != ws_id {
                        move_.workspace_config = None;
                        update_config = true;
                    }
                }

                if output != move_.output {
                    move_.tile.window().output_leave(&move_.output);
                    move_.tile.window().output_enter(&output);
                    move_.tile.window().set_preferred_scale_transform(
                        output.current_scale(),
                        output.current_transform(),
                    );
                    move_.output = output.clone();
                    self.focus_output(&output);

                    move_.output_config = self
                        .monitor_for_output(&output)
                        .and_then(|mon| mon.layout_config().cloned());

                    update_config = true;
                }

                if update_config {
                    let view_size = output_size(&output);
                    let scale = output.current_scale().fractional_scale();
                    let options = Options::clone(&self.options)
                        .with_merged_layout(move_.output_config.as_ref())
                        .with_merged_layout(move_.workspace_config.as_ref().map(|(_, c)| c))
                        .adjusted_for_scale(scale);
                    move_.tile.update_config(view_size, scale, Rc::new(options));
                }

                move_.pointer_pos_within_output = pointer_pos_within_output;

                self.interactive_move = Some(InteractiveMoveState::Moving(move_));
            }
        }

        true
    }

    pub fn interactive_move_end(&mut self, window: &W::Id) {
        let Some(move_) = &self.interactive_move else {
            return;
        };

        let move_ = match move_ {
            InteractiveMoveState::Starting { window_id, .. } => {
                if window_id != window {
                    return;
                }

                let Some(InteractiveMoveState::Starting { window_id, .. }) =
                    self.interactive_move.take()
                else {
                    unreachable!()
                };

                for mon in self.monitors_mut() {
                    mon.dnd_scroll_gesture_end();
                }

                for ws in self.workspaces_mut() {
                    if let Some(tile) = ws.tiles_mut().find(|tile| *tile.window().id() == window_id)
                    {
                        let offset = tile.interactive_move_offset;
                        tile.interactive_move_offset = Point::from((0., 0.));
                        tile.animate_move_from(offset);
                    }

                    // Unlock the view on the workspaces, but if the moved window was active,
                    // preserve that.
                    let moved_tile_was_active =
                        ws.active_window().is_some_and(|win| *win.id() == window_id);

                    ws.dnd_scroll_gesture_end();

                    if moved_tile_was_active {
                        ws.activate_window(&window_id);
                    }
                }

                return;
            }
            InteractiveMoveState::Moving(move_) => move_,
        };

        if window != move_.tile.window().id() {
            return;
        }

        let Some(InteractiveMoveState::Moving(mut move_)) = self.interactive_move.take() else {
            unreachable!()
        };

        for mon in self.monitors_mut() {
            mon.dnd_scroll_gesture_end();
        }

        // Unlock the view on the workspaces.
        if !move_.is_floating {
            for ws in self.workspaces_mut() {
                ws.dnd_scroll_gesture_end();
            }

            // Also animate the tile back to opaque.
            move_.tile.animate_alpha(
                INTERACTIVE_MOVE_ALPHA,
                1.,
                self.options.animations.window_movement.0,
            );
        }

        // Dragging in the overview shouldn't switch the workspace and so on.
        let allow_to_activate_workspace = !self.overview_open;
        let new_workspace_identity = self.monitor_for_output(&move_.output).and_then(|monitor| {
            matches!(
                monitor.insert_position(move_.pointer_pos_within_output).0,
                InsertWorkspace::Preview(_)
            )
            .then(|| self.next_free_workspace_identity())
        });

        match &mut self.monitor_set {
            MonitorSet::Normal {
                monitors,
                active_monitor_idx,
                ..
            } => {
                let (mon, insert_ws, position, offset, zoom) =
                    if let Some(mon) = monitors.iter_mut().find(|mon| mon.output == move_.output) {
                        let zoom = mon.overview_zoom();

                        let (insert_ws, geo) = mon.insert_position(move_.pointer_pos_within_output);
                        let (position, offset) = match insert_ws {
                            InsertWorkspace::Existing(ws_id) => {
                                let ws_idx = mon.idx_of_ws(ws_id).unwrap();

                                let position = if move_.is_floating {
                                    InsertPosition::Floating
                                } else {
                                    let pos_within_workspace =
                                        (move_.pointer_pos_within_output - geo.loc).downscale(zoom);
                                    let ws = &mut mon.workspaces[ws_idx];
                                    ws.scrolling_insert_position(pos_within_workspace)
                                };

                                (position, Some(geo.loc))
                            }
                            InsertWorkspace::Preview(_) => {
                                let position = if move_.is_floating {
                                    InsertPosition::Floating
                                } else {
                                    InsertPosition::NewColumn(0)
                                };

                                (position, None)
                            }
                        };

                        (mon, insert_ws, position, offset, zoom)
                    } else {
                        let mon = &mut monitors[*active_monitor_idx];
                        let zoom = mon.overview_zoom();
                        // No point in trying to use the pointer position on the wrong output.
                        let ws = &mon.workspaces[0];
                        let ws_geo = mon.workspaces_render_geo().next().unwrap();

                        let position = if move_.is_floating {
                            InsertPosition::Floating
                        } else {
                            ws.scrolling_insert_position(Point::from((0., 0.)))
                        };

                        let insert_ws = InsertWorkspace::Existing(ws.id());
                        (mon, insert_ws, position, Some(ws_geo.loc), zoom)
                    };

                let win_id = move_.tile.window().id().clone();
                let tile_render_loc = move_.tile_render_location(zoom);

                let ws_idx = match insert_ws {
                    InsertWorkspace::Existing(ws_id) => mon.idx_of_ws(ws_id).unwrap(),
                    InsertWorkspace::Preview(preview) => {
                        let (name, number) = new_workspace_identity.unwrap();
                        let id = mon.add_sway_workspace_at(preview.insertion_index, name, number);
                        mon.sort_sway_workspaces();
                        mon.idx_of_ws(id).unwrap()
                    }
                };

                let mut displaced = None;
                match position {
                    InsertPosition::NewColumn(column_idx) => {
                        let ws_id = mon.workspaces[ws_idx].id();
                        mon.add_tile(
                            move_.tile,
                            MonitorAddWindowTarget::Workspace {
                                id: ws_id,
                                column_idx: Some(column_idx),
                            },
                            ActivateWindow::Yes,
                            allow_to_activate_workspace,
                            move_.width,
                            move_.is_full_width,
                            false,
                            None,
                        );
                    }
                    InsertPosition::SwapWith(column_idx) => {
                        // Sway swaps the dragged container with the target
                        // rather than inserting beside it
                        // (seatop_move_tiling.c:365-388). Our drag already
                        // owns a detached tile, so reinsert it next to the
                        // target and then exchange the two, which lands the
                        // same tree shape.
                        let ws_id = mon.workspaces[ws_idx].id();
                        let target_window = mon.workspaces[ws_idx]
                            .tiles()
                            .nth(column_idx)
                            .map(|tile| tile.window().id().clone());
                        let moved_window = move_.tile.window().id().clone();
                        if move_.source_workspace != ws_id {
                            if let Some(target_window) = &target_window {
                                displaced = Some(
                                    mon.workspaces[ws_idx]
                                        .remove_tile(target_window, Transaction::new()),
                                );
                            }
                        }
                        mon.add_tile(
                            move_.tile,
                            MonitorAddWindowTarget::Workspace {
                                id: ws_id,
                                column_idx: Some(column_idx),
                            },
                            ActivateWindow::Yes,
                            allow_to_activate_workspace,
                            move_.width,
                            move_.is_full_width,
                            false,
                            None,
                        );
                        if move_.source_workspace == ws_id {
                            if let Some(target_window) = target_window {
                                let workspace = &mut mon.workspaces[ws_idx];
                                if let (Some(first), Some(second)) = (
                                    workspace.tiling_node_for_window(&moved_window),
                                    workspace.tiling_node_for_window(&target_window),
                                ) {
                                    let _ = workspace.swap_tiling_nodes(first, second);
                                }
                            }
                        }
                    }
                    InsertPosition::InColumn(column_idx, tile_idx) => {
                        mon.add_tile_to_column(
                            ws_idx,
                            column_idx,
                            Some(tile_idx),
                            move_.tile,
                            true,
                            allow_to_activate_workspace,
                        );
                    }
                    InsertPosition::Floating => {
                        let mut tile = move_.tile;
                        tile.floating_pos = None;

                        match insert_ws {
                            InsertWorkspace::Existing(_) => {
                                if let Some(offset) = offset {
                                    let pos = (tile_render_loc - offset).downscale(zoom);
                                    let pos =
                                        mon.workspaces[ws_idx].floating_logical_to_size_frac(pos);
                                    tile.floating_pos = Some(pos);
                                } else {
                                    error!(
                                        "offset unset for inserting a floating tile \
                                         to existing workspace"
                                    );
                                }
                            }
                            InsertWorkspace::Preview(_) => {
                                // When putting a floating tile on a new workspace, we don't really
                                // have a good pre-existing position.
                            }
                        }

                        // Set the floating size so it takes into account any window resizing that
                        // took place during the move.
                        if let Some(size) = tile.window().expected_size() {
                            tile.floating_window_size = Some(size);
                        }

                        let ws_id = mon.workspaces[ws_idx].id();
                        mon.add_tile(
                            tile,
                            MonitorAddWindowTarget::Workspace {
                                id: ws_id,
                                column_idx: None,
                            },
                            ActivateWindow::Yes,
                            allow_to_activate_workspace,
                            move_.width,
                            move_.is_full_width,
                            true,
                            None,
                        );
                    }
                }

                let (tile, tile_offset, ws_geo) = mon
                    .workspaces_with_render_geo_mut(false)
                    .find_map(|(ws, geo)| {
                        ws.tiles_with_render_positions_mut(false)
                            .find(|(tile, _)| tile.window().id() == &win_id)
                            .map(|(tile, tile_offset)| (tile, tile_offset, geo))
                    })
                    .unwrap();
                let new_tile_render_loc = ws_geo.loc + tile_offset.upscale(zoom);

                tile.animate_move_from((tile_render_loc - new_tile_render_loc).downscale(zoom));

                // Interactive move into floating barely animates (it doesn't really move after
                // being dropped), so setting it as moving between workspaces would just cause it to
                // awkwardly sit unclipped for a moment before the animation runs out.
                if !matches!(position, InsertPosition::Floating) {
                    tile.set_anim_y_between_workspaces();
                }

                if let Some(displaced) = displaced {
                    if let Some(source) = monitors
                        .iter_mut()
                        .find(|monitor| monitor.has_ws(move_.source_workspace))
                    {
                        source.add_tile(
                            displaced.tile,
                            MonitorAddWindowTarget::Workspace {
                                id: move_.source_workspace,
                                column_idx: None,
                            },
                            ActivateWindow::No,
                            false,
                            displaced.width,
                            displaced.is_full_width,
                            displaced.is_floating,
                            None,
                        );
                    }
                }

                if let Some(source) = monitors
                    .iter_mut()
                    .find(|monitor| monitor.has_ws(move_.source_workspace))
                {
                    if source.workspace_switch.is_none() {
                        source.clean_up_workspaces();
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                if workspaces.is_empty() {
                    workspaces.push(Workspace::new_no_outputs(
                        self.clock.clone(),
                        self.options.clone(),
                    ));
                }
                let ws = &mut workspaces[0];

                // No point in trying to use the pointer position without outputs.
                ws.add_tile(
                    move_.tile,
                    WorkspaceAddWindowTarget::Auto,
                    ActivateWindow::Yes,
                    move_.width,
                    move_.is_full_width,
                    move_.is_floating,
                    None,
                );
            }
        }
    }

    pub fn interactive_move_is_moving_above_output(&self, output: &Output) -> bool {
        let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move else {
            return false;
        };

        move_.output == *output
    }

    pub fn dnd_update(&mut self, output: Output, pointer_pos_within_output: Point<f64, Logical>) {
        let begin_gesture = self.dnd.is_none();

        self.dnd = Some(DndData {
            output,
            pointer_pos_within_output,
            hold: None,
        });

        if begin_gesture {
            for mon in self.monitors_mut() {
                mon.dnd_scroll_gesture_begin();
            }

            for ws in self.workspaces_mut() {
                ws.dnd_scroll_gesture_begin();
            }
        }
    }

    pub fn dnd_end(&mut self) {
        if self.dnd.is_none() {
            return;
        }

        self.dnd = None;

        for mon in self.monitors_mut() {
            mon.dnd_scroll_gesture_end();
        }

        for ws in self.workspaces_mut() {
            ws.dnd_scroll_gesture_end();
        }
    }

    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        if ws.has_window(&window) {
                            return ws.interactive_resize_begin(window, edges);
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    if ws.has_window(&window) {
                        return ws.interactive_resize_begin(window, edges);
                    }
                }
            }
        }

        false
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return false;
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        if ws.has_window(window) {
                            return ws.interactive_resize_update(window, delta);
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    if ws.has_window(window) {
                        return ws.interactive_resize_update(window, delta);
                    }
                }
            }
        }

        false
    }

    pub fn interactive_resize_end(&mut self, window: &W::Id) {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return;
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        if ws.has_window(window) {
                            ws.interactive_resize_end(Some(window));
                            return;
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    if ws.has_window(window) {
                        ws.interactive_resize_end(Some(window));
                        return;
                    }
                }
            }
        }
    }

    pub fn move_workspace_down(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.move_workspace_down();
    }

    pub fn move_workspace_up(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.move_workspace_up();
    }

    pub fn move_workspace_to_idx(
        &mut self,
        reference: Option<(Option<Output>, usize)>,
        new_idx: usize,
    ) {
        let (monitor, old_idx) = if let Some((output, old_idx)) = reference {
            let monitor = if let Some(output) = output {
                let Some(monitor) = self.monitor_for_output_mut(&output) else {
                    return;
                };
                monitor
            } else {
                // In case a numbered workspace reference is used, assume the active monitor
                let Some(monitor) = self.active_monitor() else {
                    return;
                };
                monitor
            };

            (monitor, old_idx)
        } else {
            let Some(monitor) = self.active_monitor() else {
                return;
            };
            let index = monitor.active_workspace_idx;
            (monitor, index)
        };

        monitor.move_workspace_to_idx(old_idx, new_idx);
    }

    pub fn set_workspace_name(&mut self, name: String, reference: Option<WorkspaceReference>) {
        // ignore the request if the name is already used by another workspace
        if self.find_workspace_by_name(&name).is_some() {
            return;
        }

        let ws = if let Some(reference) = reference {
            self.find_workspace_by_ref(reference)
        } else {
            self.active_workspace_mut()
        };
        let Some(ws) = ws else {
            return;
        };

        ws.set_persistent_name(name);

        // Every monitor re-establishes its trailing placeholder, so the renamed
        // workspace's own id is no longer needed to pick one.
        if let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set {
            for monitor in monitors {
                monitor.reap_empty_workspaces();
            }
        }
    }

    pub fn unset_workspace_name(&mut self, reference: Option<WorkspaceReference>) {
        let ws = if let Some(reference) = reference {
            self.find_workspace_by_ref(reference)
        } else {
            self.active_workspace_mut()
        };
        let Some(ws) = ws else {
            return;
        };
        let id = ws.id();

        self.unname_workspace_by_id(id);
    }

    pub fn set_monitors_overview_state(&mut self) {
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return;
        };

        for mon in monitors {
            mon.overview_open = self.overview_open;
            mon.set_overview_progress(self.overview_progress.as_ref());
        }
    }

    pub fn toggle_overview(&mut self) {
        self.overview_open = !self.overview_open;

        let from = self.overview_progress.take().map_or(0., |p| p.value());
        let to = if self.overview_open { 1. } else { 0. };

        self.overview_progress = Some(OverviewProgress::Animation(Animation::new(
            self.clock.clone(),
            from,
            to,
            0.,
            self.options.animations.overview_open_close.0,
        )));

        self.set_monitors_overview_state();
    }

    pub fn open_overview(&mut self) -> bool {
        if self.overview_open {
            return false;
        }

        self.toggle_overview();
        true
    }

    pub fn close_overview(&mut self) -> bool {
        if !self.overview_open {
            return false;
        }

        self.toggle_overview();
        true
    }

    pub fn toggle_overview_to_workspace(&mut self, ws_idx: usize) {
        let config = self.options.animations.overview_open_close.0;
        if let Some(mon) = self.active_monitor() {
            mon.activate_workspace_with_anim_config(ws_idx, Some(config));
        }
        self.toggle_overview();
    }

    pub fn start_open_animation_for_window(&mut self, window: &W::Id) {
        if let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move {
            if move_.tile.window().id() == window {
                return;
            }
        }

        for ws in self.workspaces_mut() {
            if ws.start_open_animation(window) {
                return;
            }
        }
    }

    pub fn store_unmap_snapshot(
        &mut self,
        renderer: &mut GlesRenderer,
        xray: Option<&mut Xray>,
        xray_has_blocked_out_layers: bool,
        window: &W::Id,
    ) {
        let _span = tracy_client::span!("Layout::store_unmap_snapshot");

        let zoom = self.overview_zoom();

        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if move_.tile.window().id() == window {
                let pos_within_output = move_.tile_render_location(zoom);

                // Computation matches update_render_elements().
                let view_rect =
                    Rectangle::new(pos_within_output.upscale(-1.), output_size(&move_.output))
                        .downscale(zoom);
                move_.tile.update_render_elements(false, view_rect);

                move_.tile.store_unmap_snapshot_if_empty(
                    renderer,
                    xray,
                    xray_has_blocked_out_layers,
                    XrayPos::new(pos_within_output, zoom),
                );
                return;
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for (ws, geo) in mon.workspaces_with_render_geo_mut(false) {
                        if ws.has_window(window) {
                            ws.store_unmap_snapshot_if_empty(
                                renderer,
                                xray,
                                xray_has_blocked_out_layers,
                                XrayPos::new(geo.loc, zoom),
                                window,
                            );
                            return;
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    if ws.has_window(window) {
                        ws.store_unmap_snapshot_if_empty(
                            renderer,
                            xray,
                            xray_has_blocked_out_layers,
                            XrayPos::default(),
                            window,
                        );
                        return;
                    }
                }
            }
        }
    }

    pub fn clear_unmap_snapshot(&mut self, window: &W::Id) {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if move_.tile.window().id() == window {
                let _ = move_.tile.take_unmap_snapshot();
                return;
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        if ws.has_window(window) {
                            ws.clear_unmap_snapshot(window);
                            return;
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    if ws.has_window(window) {
                        ws.clear_unmap_snapshot(window);
                        return;
                    }
                }
            }
        }
    }

    pub fn start_close_animation_for_window(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &W::Id,
        blocker: TransactionBlocker,
    ) {
        let _span = tracy_client::span!("Layout::start_close_animation_for_window");

        let zoom = self.overview_zoom();

        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if move_.tile.window().id() == window {
                let Some(snapshot) = move_.tile.take_unmap_snapshot() else {
                    return;
                };
                let tile_pos = move_.tile_render_location(zoom);
                let tile_size = move_.tile.tile_size();

                let output = move_.output.clone();
                let pointer_pos_within_output = move_.pointer_pos_within_output;
                let Some(mon) = self.monitor_for_output_mut(&output) else {
                    return;
                };
                let Some((ws, ws_geo)) = mon.workspace_under(pointer_pos_within_output) else {
                    return;
                };
                let idx = mon.idx_of_ws(ws.id()).unwrap();
                let ws = &mut mon.workspaces[idx];

                let tile_pos = tile_pos - ws_geo.loc;
                ws.start_close_animation_for_tile(renderer, snapshot, tile_size, tile_pos, blocker);
                return;
            }
        }

        match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                for mon in monitors {
                    for ws in &mut mon.workspaces {
                        if ws.has_window(window) {
                            ws.start_close_animation_for_window(renderer, window, blocker);
                            return;
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    if ws.has_window(window) {
                        ws.start_close_animation_for_window(renderer, window, blocker);
                        return;
                    }
                }
            }
        }
    }

    pub fn render_interactive_move_for_output<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        output: &Output,
        push: &mut dyn FnMut(RescaleRenderElement<TileRenderElement<R>>),
    ) {
        if self.update_render_elements_time != self.clock.now() {
            error!("clock moved between updating render elements and rendering");
        }

        let Some(InteractiveMoveState::Moving(move_)) = &self.interactive_move else {
            return;
        };

        if &move_.output != output {
            return;
        }

        let scale = Scale::from(move_.output.current_scale().fractional_scale());
        let zoom = self.overview_zoom();
        let pos_in_backdrop = move_.tile_render_location(zoom);
        let xray_pos = XrayPos::new(pos_in_backdrop, zoom);

        move_
            .tile
            .render(ctx, pos_in_backdrop, xray_pos, true, &mut |elem| {
                push(RescaleRenderElement::from_element(
                    elem,
                    pos_in_backdrop.to_physical_precise_round(scale),
                    zoom,
                ));
            });
    }

    pub fn refresh(&mut self, is_active: bool) {
        let _span = tracy_client::span!("Layout::refresh");

        self.is_active = is_active;

        let mut ongoing_scrolling_dnd = self.dnd.is_some().then_some(true);

        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            let win = move_.tile.window_mut();

            win.set_active_in_column(true);
            win.set_floating(move_.is_floating);
            win.set_activated(true);

            win.set_interactive_resize(None);

            win.set_bounds(output_size(&move_.output).to_i32_round());

            win.send_pending_configure();
            win.refresh();

            ongoing_scrolling_dnd.get_or_insert(!move_.is_floating);
        } else if let Some(InteractiveMoveState::Starting { window_id, .. }) =
            &self.interactive_move
        {
            ongoing_scrolling_dnd.get_or_insert_with(|| {
                let (_, _, ws) = self
                    .workspaces()
                    .find(|(_, _, ws)| ws.has_window(window_id))
                    .unwrap();
                !ws.is_floating(window_id)
            });
        }

        match &mut self.monitor_set {
            MonitorSet::Normal {
                monitors,
                active_monitor_idx,
                ..
            } => {
                for (idx, mon) in monitors.iter_mut().enumerate() {
                    let is_active = self.is_active
                        && idx == *active_monitor_idx
                        && !matches!(self.interactive_move, Some(InteractiveMoveState::Moving(_)));

                    if ongoing_scrolling_dnd.is_some() && self.overview_open {
                        // Begin the scroll on new monitors and when opening the overview.
                        mon.dnd_scroll_gesture_begin();
                    } else if !self.overview_open {
                        mon.dnd_scroll_gesture_end();
                    }

                    for (ws_idx, ws) in mon.workspaces.iter_mut().enumerate() {
                        let is_focused = is_active && ws_idx == mon.active_workspace_idx;
                        ws.refresh(is_active, is_focused);

                        if let Some(is_scrolling) = ongoing_scrolling_dnd {
                            // Lock or unlock the view for scrolling interactive move.
                            if is_scrolling {
                                ws.dnd_scroll_gesture_begin();
                            } else {
                                ws.dnd_scroll_gesture_end();
                            }
                        } else {
                            // Cancel the view offset gesture after workspace switches, moves, etc.
                            if !self.overview_open && ws_idx != mon.active_workspace_idx {
                                ws.view_offset_gesture_end(None);
                            }
                        }
                    }
                }
            }
            MonitorSet::NoOutputs { workspaces, .. } => {
                for ws in workspaces {
                    ws.refresh(false, false);
                    ws.view_offset_gesture_end(None);
                }
            }
        }
    }

    pub fn workspaces(
        &self,
    ) -> impl Iterator<Item = (Option<&Monitor<W>>, usize, &Workspace<W>)> + '_ {
        let (iter_normal, iter_no_outputs) = match &self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                let it = monitors.iter().flat_map(|mon| {
                    mon.workspaces
                        .iter()
                        .enumerate()
                        .map(move |(idx, ws)| (Some(mon), idx, ws))
                });

                (Some(it), None)
            }
            MonitorSet::NoOutputs { workspaces } => {
                let it = workspaces
                    .iter()
                    .enumerate()
                    .map(|(idx, ws)| (None, idx, ws));

                (None, Some(it))
            }
        };

        let iter_normal = iter_normal.into_iter().flatten();
        let iter_no_outputs = iter_no_outputs.into_iter().flatten();
        iter_normal.chain(iter_no_outputs)
    }

    pub fn workspaces_mut(&mut self) -> impl Iterator<Item = &mut Workspace<W>> + '_ {
        let (iter_normal, iter_no_outputs) = match &mut self.monitor_set {
            MonitorSet::Normal { monitors, .. } => {
                let it = monitors
                    .iter_mut()
                    .flat_map(|mon| mon.workspaces.iter_mut());

                (Some(it), None)
            }
            MonitorSet::NoOutputs { workspaces } => {
                let it = workspaces.iter_mut();

                (None, Some(it))
            }
        };

        let iter_normal = iter_normal.into_iter().flatten();
        let iter_no_outputs = iter_no_outputs.into_iter().flatten();
        iter_normal.chain(iter_no_outputs)
    }

    pub fn window_center(&self, window: &W::Id) -> Option<Point<i32, Logical>> {
        self.monitors().find_map(|monitor| {
            let output_origin = monitor.output().current_location();
            monitor.workspaces.iter().find_map(|workspace| {
                workspace
                    .tiles_with_render_positions()
                    .find(|(tile, _, _)| tile.window().id() == window)
                    .map(|(tile, tile_pos, _)| {
                        let tile_rect = Rectangle::new(tile_pos, tile.tile_size());
                        output_origin + crate::utils::center_f64(tile_rect).to_i32_round()
                    })
            })
        })
    }

    pub fn windows(&self) -> impl Iterator<Item = (Option<&Monitor<W>>, &W)> {
        let moving_window = self
            .interactive_move
            .as_ref()
            .and_then(|x| x.moving())
            .map(|move_| (self.monitor_for_output(&move_.output), move_.tile.window()))
            .into_iter();

        let scratchpad = self
            .scratchpad
            .iter()
            .map(|removed| (None, removed.tile.window()));
        let rest = self
            .workspaces()
            .flat_map(|(mon, _, ws)| ws.windows().map(move |win| (mon, win)));

        moving_window.chain(scratchpad).chain(rest)
    }

    pub fn has_window(&self, window: &W::Id) -> bool {
        self.windows().any(|(_, win)| win.id() == window)
    }

    pub fn is_overview_open(&self) -> bool {
        self.overview_open
    }
}

impl<W: LayoutElement> Default for MonitorSet<W> {
    fn default() -> Self {
        Self::NoOutputs { workspaces: vec![] }
    }
}

fn compute_overview_zoom(options: &Options, overview_progress: Option<f64>) -> f64 {
    // Clamp to some sane values.
    let zoom = options.overview.zoom.clamp(0.0001, 0.75);

    if let Some(p) = overview_progress {
        (1. - p * (1. - zoom)).max(0.0001)
    } else {
        1.
    }
}

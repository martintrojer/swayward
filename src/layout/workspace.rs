use std::cmp::max;
use std::rc::Rc;
use std::time::Duration;

use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::desktop::{layer_map_for_output, Window};
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Serial, Size, Transform};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::SurfaceCachedState;
use swayward_config::utils::MergeWith as _;
use swayward_config::{CornerRadius, OutputName, PresetSize, Workspace as WorkspaceConfig};
use swayward_ipc::{ColumnDisplay, PositionChange, SizeChange, WindowLayout};

use super::floating::{apply_position_change, FloatingSpace, FloatingSpaceRenderElement};
use super::scrolling::{ColumnWidth, ScrollDirection};
use super::shadow::Shadow;
use super::tile::{Tile, TileRenderSnapshot};
use super::tiling_tree::{
    DetachedSubtree, Direction, InsertTarget, NodeId, TilingTree, TilingTreeRenderElement,
};
use super::{
    ActivateWindow, HitType, InsertPosition, InteractiveResizeData, LayoutElement, Options,
    RemovedTile, SizeFrac,
};
use crate::animation::Clock;
use crate::layout::RenderLayer;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::shadow::ShadowRenderElement;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};
use crate::render_helpers::xray::{Xray, XrayPos};
use crate::render_helpers::RenderCtx;
use crate::swayward_render_elements;
use crate::utils::id::IdCounter;
use crate::utils::transaction::{Transaction, TransactionBlocker};
use crate::utils::{
    ensure_min_max_size, ensure_min_max_size_maybe_zero, output_size, send_scale_transform,
    ResizeEdge,
};
use crate::window::ResolvedWindowRules;

#[derive(Debug)]
pub struct Workspace<W: LayoutElement> {
    /// The scrollable-tiling layout.
    tiling: TilingTree<W>,

    /// The floating layout.
    floating: FloatingSpace<W>,

    /// Whether the floating layout is active instead of the scrolling layout.
    floating_is_active: FloatingActive,

    /// The original output of this workspace.
    ///
    /// Most of the time this will be the workspace's current output, however, after an output
    /// disconnection, it may remain pointing to the disconnected output.
    pub(super) original_output: OutputId,

    /// Current output of this workspace.
    output: Option<Output>,

    /// Latest known output scale for this workspace.
    ///
    /// This should be set from the current workspace output, or, if all outputs have been
    /// disconnected, preserved until a new output is connected.
    scale: smithay::output::Scale,

    /// Latest known output transform for this workspace.
    ///
    /// This should be set from the current workspace output, or, if all outputs have been
    /// disconnected, preserved until a new output is connected.
    transform: Transform,

    /// Latest known view size for this workspace.
    ///
    /// This should be computed from the current workspace output size, or, if all outputs have
    /// been disconnected, preserved until a new output is connected.
    view_size: Size<f64, Logical>,

    /// Latest known working area for this workspace.
    ///
    /// Not rounded to physical pixels.
    ///
    /// This is similar to view size, but takes into account things like layer shell exclusive
    /// zones.
    working_area: Rectangle<f64, Logical>,

    /// This workspace's shadow in the overview.
    shadow: Shadow,

    /// This workspace's background.
    background_buffer: SolidColorBuffer,

    /// Clock for driving animations.
    pub(super) clock: Clock,

    /// Configurable properties of the layout as received from the parent monitor.
    pub(super) base_options: Rc<Options>,

    /// Configurable properties of the layout with logical sizes adjusted for the current `scale`.
    pub(super) options: Rc<Options>,

    /// Optional name of this workspace.
    pub(super) name: Option<String>,

    /// Stable sway workspace number. Named workspaces have no number.
    pub(super) number: Option<i32>,

    /// Whether this workspace came from persistent configuration.
    persistent: bool,

    /// Layout config overrides for this workspace.
    layout_config: Option<swayward_config::LayoutPart>,

    /// Gap defaults copied when this workspace was created or changed at runtime.
    gaps: f64,
    outer_gaps: swayward_config::OuterGaps,
    outer_gaps_configured: bool,

    /// Unique ID of this workspace.
    id: WorkspaceId,
}

#[derive(Debug, Clone)]
pub struct OutputId(String);

impl OutputId {
    pub fn matches(&self, output: &Output) -> bool {
        let output_name = output.user_data().get::<OutputName>().unwrap();
        output_name.matches(&self.0)
    }
}

static WORKSPACE_ID_COUNTER: IdCounter = IdCounter::new();

/// Ord follows the monotonic allocation counter, so comparing two ids compares
/// creation order. The workspace sort relies on that to break ties the way
/// sway's stable sort does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(u64);

impl WorkspaceId {
    fn next() -> WorkspaceId {
        WorkspaceId(WORKSPACE_ID_COUNTER.next())
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub fn specific(id: u64) -> Self {
        Self(id)
    }
}

swayward_render_elements! {
    WorkspaceRenderElement<R> => {
        Scrolling = TilingTreeRenderElement<R>,
        Floating = FloatingSpaceRenderElement<R>,
    }
}

#[derive(Debug)]
pub(super) struct InteractiveResize<W: LayoutElement> {
    pub window: W::Id,
    pub original_window_size: Size<f64, Logical>,
    pub data: InteractiveResizeData,
}

/// Resolved width or height in logical pixels.
#[derive(Debug, Clone, Copy)]
pub enum ResolvedSize {
    /// Size of the tile including borders.
    Tile(f64),
    /// Size of the window excluding borders.
    Window(f64),
}

/// Whether the floating space is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FloatingActive {
    /// The scrolling space is active.
    No,
    /// The scrolling space is active, but the floating space should render on top, even if the
    /// active scrolling window is fullscreen.
    ///
    /// This is necessary for focus-follows-mouse that activates but doesn't raise the window to
    /// avoid being annoying.
    NoButRaised,
    /// The floating space is active.
    Yes,
}

/// Where to put a newly added window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAddWindowTarget<'a, W: LayoutElement> {
    /// No particular preference.
    #[default]
    Auto,
    /// As a new column at this index.
    NewColumnAt(usize),
    /// Next to this existing window.
    NextTo(&'a W::Id),
}

impl OutputId {
    pub fn new(output: &Output) -> Self {
        let output_name = output.user_data().get::<OutputName>().unwrap();
        Self(output_name.format_make_model_serial_or_connector())
    }
}

impl FloatingActive {
    fn get(self) -> bool {
        self == Self::Yes
    }
}

impl<W: LayoutElement> Workspace<W> {
    pub fn new(output: Output, clock: Clock, options: Rc<Options>) -> Self {
        Self::new_with_config(output, None, clock, options)
    }

    pub fn new_with_config(
        output: Output,
        mut config: Option<WorkspaceConfig>,
        clock: Clock,
        base_options: Rc<Options>,
    ) -> Self {
        let original_output = config
            .as_ref()
            .and_then(|c| c.open_on_output.clone())
            .map(OutputId)
            .unwrap_or(OutputId::new(&output));

        let layout_config = config.as_mut().and_then(|c| c.layout.take().map(|x| x.0));

        let scale = output.current_scale();
        let options = Options::clone(&base_options).with_merged_layout(layout_config.as_ref());
        let gaps = options.layout.gaps;
        let outer_gaps = options.layout.outer_gaps;
        let outer_gaps_configured = options.layout.outer_gaps_configured;
        let options = Rc::new(options.adjusted_for_scale(scale.fractional_scale()));

        let view_size = output_size(&output);
        let output_area = compute_working_area(&output);
        let working_area = apply_outer_gaps(
            output_area,
            options.layout.outer_gaps,
            options.layout.gaps,
            options.layout.outer_gaps_configured,
        );

        let tiling = TilingTree::new(
            view_size,
            working_area,
            has_gaps_to_edge(output_area, options.layout.outer_gaps, options.layout.gaps),
            scale.fractional_scale(),
            clock.clone(),
            options.clone(),
        );

        let floating = FloatingSpace::new(
            view_size,
            working_area,
            scale.fractional_scale(),
            clock.clone(),
            options.clone(),
        );

        let shadow_config =
            compute_workspace_shadow_config(options.overview.workspace_shadow, view_size);

        Self {
            tiling,
            floating,
            floating_is_active: FloatingActive::No,
            original_output,
            scale,
            transform: output.current_transform(),
            view_size,
            working_area,
            shadow: Shadow::new(shadow_config),
            background_buffer: SolidColorBuffer::new(view_size, options.layout.background_color),
            output: Some(output),
            clock,
            base_options,
            options,
            persistent: config.is_some(),
            name: config.map(|c| c.name.0),
            number: None,
            layout_config,
            gaps,
            outer_gaps,
            outer_gaps_configured,
            id: WorkspaceId::next(),
        }
    }

    pub fn new_with_config_no_outputs(
        mut config: Option<WorkspaceConfig>,
        clock: Clock,
        base_options: Rc<Options>,
    ) -> Self {
        let original_output = OutputId(
            config
                .as_ref()
                .and_then(|c| c.open_on_output.clone())
                .unwrap_or_default(),
        );

        let layout_config = config.as_mut().and_then(|c| c.layout.take().map(|x| x.0));

        let scale = smithay::output::Scale::Integer(1);
        let options = Options::clone(&base_options).with_merged_layout(layout_config.as_ref());
        let gaps = options.layout.gaps;
        let outer_gaps = options.layout.outer_gaps;
        let outer_gaps_configured = options.layout.outer_gaps_configured;
        let options = Rc::new(options.adjusted_for_scale(scale.fractional_scale()));

        let view_size = Size::from((1280., 720.));
        let output_area = Rectangle::from_size(view_size);
        let working_area = apply_outer_gaps(
            output_area,
            options.layout.outer_gaps,
            options.layout.gaps,
            options.layout.outer_gaps_configured,
        );

        let tiling = TilingTree::new(
            view_size,
            working_area,
            has_gaps_to_edge(output_area, options.layout.outer_gaps, options.layout.gaps),
            scale.fractional_scale(),
            clock.clone(),
            options.clone(),
        );

        let floating = FloatingSpace::new(
            view_size,
            working_area,
            scale.fractional_scale(),
            clock.clone(),
            options.clone(),
        );

        let shadow_config =
            compute_workspace_shadow_config(options.overview.workspace_shadow, view_size);

        Self {
            tiling,
            floating,
            floating_is_active: FloatingActive::No,
            output: None,
            scale,
            transform: Transform::Normal,
            original_output,
            view_size,
            working_area,
            shadow: Shadow::new(shadow_config),
            background_buffer: SolidColorBuffer::new(view_size, options.layout.background_color),
            clock,
            base_options,
            options,
            persistent: config.is_some(),
            name: config.map(|c| c.name.0),
            number: None,
            layout_config,
            gaps,
            outer_gaps,
            outer_gaps_configured,
            id: WorkspaceId::next(),
        }
    }

    pub fn new_no_outputs(clock: Clock, options: Rc<Options>) -> Self {
        Self::new_with_config_no_outputs(None, clock, options)
    }

    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
    }

    pub fn sway_name(&self) -> Option<String> {
        self.name
            .clone()
            .or_else(|| self.number.map(|number| number.to_string()))
    }

    pub fn sway_display_name(&self, index: usize) -> String {
        self.sway_name().unwrap_or_else(|| (index + 1).to_string())
    }

    /// The number a client sees for this workspace.
    ///
    /// `index` is only a fallback for the initial workspace before startup has
    /// assigned its sway identity.
    pub fn sway_display_number(&self, index: usize) -> i32 {
        self.number.unwrap_or_else(|| {
            self.name.as_ref().map_or_else(
                || i32::try_from(index + 1).unwrap_or(-1),
                |name| super::sway_workspace_num(name),
            )
        })
    }

    pub fn number(&self) -> Option<i32> {
        self.number
    }

    pub fn set_sway_identity(&mut self, name: Option<String>, number: Option<i32>) {
        self.name = name;
        self.number = number;
    }

    pub fn set_persistent_name(&mut self, name: String) {
        self.name = Some(name);
        self.number = None;
        self.persistent = true;
    }

    /// Whether this workspace outlives its last window.
    ///
    /// Only a configuration declaration earns that. A runtime
    /// `rename workspace` moves the name, not the declaration, so a workspace
    /// renamed away from its configured name becomes disposable again.
    pub fn set_persistent(&mut self, persistent: bool) {
        self.persistent = persistent;
    }

    pub fn unname(&mut self) {
        self.name = None;
        self.number = None;
        self.persistent = false;
    }

    /// Whether a client can address this workspace.
    ///
    /// A sway workspace is identified by a name, a number, or both. niri had
    /// only `name`, so the inherited checks asked about that one field; adding
    /// `number` left them answering a question nobody was asking. Every caller
    /// that means "is this workspace visible to clients" must use this.
    pub fn has_sway_identity(&self) -> bool {
        self.name.is_some() || self.number.is_some()
    }

    /// Whether the configuration declares this workspace, so it outlives its
    /// last window. Implies [`Workspace::has_sway_identity`], because a
    /// persistent workspace is always created with a name.
    pub fn is_persistent(&self) -> bool {
        self.persistent
    }

    /// Whether this workspace outlives focus leaving it.
    ///
    /// Deliberately *not* [`Workspace::has_sway_identity`]. A workspace that
    /// merely holds a number is addressable but disposable: measured on sway
    /// 1.11, focusing an empty workspace 7 and switching away leaves
    /// `get_workspaces` reporting no 7 at all, while an empty workspace that
    /// still holds focus is reported. Only windows or a configured name earn
    /// survival, per `workspace_consider_destroy`,
    /// `sway/tree/workspace.c:313-330`.
    pub fn must_be_kept(&self) -> bool {
        self.has_windows() || self.persistent
    }

    pub fn scale(&self) -> smithay::output::Scale {
        self.scale
    }

    pub fn advance_animations(&mut self) {
        self.tiling.advance_animations();
        self.floating.advance_animations();
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.tiling.are_animations_ongoing() || self.floating.are_animations_ongoing()
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.tiling.are_transitions_ongoing() || self.floating.are_transitions_ongoing()
    }

    pub fn update_render_elements(&mut self, is_active: bool, layer: RenderLayer) {
        self.tiling
            .update_render_elements(is_active && !self.floating_is_active.get(), layer);

        let view_rect = Rectangle::from_size(self.view_size);
        self.floating.update_render_elements(
            is_active && self.floating_is_active.get(),
            view_rect,
            layer,
        );

        if layer.is_normal() {
            self.shadow.update_render_elements(
                self.view_size,
                true,
                CornerRadius::default(),
                self.scale.fractional_scale(),
                1.,
            );
        }
    }

    pub fn update_config(&mut self, base_options: Rc<Options>) {
        let scale = self.scale.fractional_scale();
        let mut options =
            Options::clone(&base_options).with_merged_layout(self.layout_config.as_ref());
        options.layout.gaps = self.gaps;
        options.layout.outer_gaps = self.outer_gaps;
        options.layout.outer_gaps_configured = self.outer_gaps_configured;
        let options = Rc::new(options.adjusted_for_scale(scale));
        let output_area = self
            .output
            .as_ref()
            .map(compute_working_area)
            .unwrap_or_else(|| Rectangle::from_size(self.view_size));
        let suppress_outer = match options.layout.smart_gaps {
            swayward_config::SmartGaps::Off => false,
            swayward_config::SmartGaps::On => self.tiling.visible_window_count() == 1,
            swayward_config::SmartGaps::InverseOuter => self.tiling.visible_window_count() != 1,
        };
        self.working_area = if suppress_outer {
            output_area
        } else {
            apply_outer_gaps(
                output_area,
                options.layout.outer_gaps,
                options.layout.gaps,
                options.layout.outer_gaps_configured,
            )
        };

        self.tiling.update_config(
            self.view_size,
            self.working_area,
            has_gaps_to_edge(output_area, options.layout.outer_gaps, options.layout.gaps),
            self.scale.fractional_scale(),
            options.clone(),
        );

        self.floating.update_config(
            self.view_size,
            self.working_area,
            self.scale.fractional_scale(),
            options.clone(),
        );

        let shadow_config =
            compute_workspace_shadow_config(options.overview.workspace_shadow, self.view_size);
        self.shadow.update_config(shadow_config);

        self.background_buffer
            .set_color(options.layout.background_color);

        self.base_options = base_options;
        self.options = options;
    }

    /// Adopt a per-name layout configuration as this workspace's own gaps.
    ///
    /// [`Self::update_config`] pins `self.gaps` over the merged options, so that
    /// a later change to the global defaults cannot disturb a live workspace.
    /// That pin is set from the base options at construction, so merging a
    /// layout part afterwards is not enough: the pinned values have to be
    /// re-derived from the merge. Sway does the same thing at creation time by
    /// seeding `ws->gaps_*` from the config and then overwriting from the
    /// workspace config (`sway/sway/tree/workspace.c:224-243`).
    ///
    /// Only for a freshly created workspace. Use [`Self::update_layout_config`]
    /// to re-apply configuration to an existing one.
    pub fn adopt_configured_layout(&mut self, layout_config: Option<swayward_config::LayoutPart>) {
        self.layout_config = layout_config;
        let merged =
            Options::clone(&self.base_options).with_merged_layout(self.layout_config.as_ref());
        self.gaps = merged.layout.gaps;
        self.outer_gaps = merged.layout.outer_gaps;
        self.outer_gaps_configured = merged.layout.outer_gaps_configured;
        self.update_config(self.base_options.clone());
    }

    pub fn update_layout_config(&mut self, layout_config: Option<swayward_config::LayoutPart>) {
        if self.layout_config == layout_config {
            return;
        }

        self.layout_config = layout_config;
        self.update_config(self.base_options.clone());
    }

    pub fn update_gaps(
        &mut self,
        inner: bool,
        sides: [bool; 4],
        operation: swayward_ipc::command::GapOperation,
        amount: i32,
    ) {
        let mut layout = self.layout_config.clone().unwrap_or_default();
        let apply = |value: f64| match operation {
            swayward_ipc::command::GapOperation::Set => f64::from(amount),
            swayward_ipc::command::GapOperation::Plus => value + f64::from(amount),
            swayward_ipc::command::GapOperation::Minus => value - f64::from(amount),
            swayward_ipc::command::GapOperation::Toggle => {
                if value == 0. {
                    f64::from(amount)
                } else {
                    0.
                }
            }
        };
        if inner {
            self.gaps = apply(self.gaps).max(0.);
            layout.gaps = Some(swayward_config::FloatOrInt(self.gaps));
        } else {
            let current = self.outer_gaps;
            let mut values = [current.left, current.right, current.top, current.bottom];
            for (selected, value) in sides.into_iter().zip(&mut values) {
                if selected {
                    *value = apply(*value);
                }
            }
            self.outer_gaps = swayward_config::OuterGaps {
                left: values[0],
                right: values[1],
                top: values[2],
                bottom: values[3],
            };
            self.outer_gaps_configured = true;
            layout.outer_gaps = Some(swayward_config::OuterGapsPart {
                left: Some(swayward_config::FloatOrInt(values[0])),
                right: Some(swayward_config::FloatOrInt(values[1])),
                top: Some(swayward_config::FloatOrInt(values[2])),
                bottom: Some(swayward_config::FloatOrInt(values[3])),
            });
        }
        self.update_layout_config(Some(layout));
    }

    pub fn update_shaders(&mut self) {
        self.tiling.update_shaders();
        self.floating.update_shaders();
        self.shadow.update_shaders();
    }

    pub fn windows(&self) -> impl Iterator<Item = &W> + '_ {
        self.tiles().map(Tile::window)
    }

    pub fn windows_mut(&mut self) -> impl Iterator<Item = &mut W> + '_ {
        self.tiles_mut().map(Tile::window_mut)
    }

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> + '_ {
        let scrolling = self.tiling.tiles();
        let floating = self.floating.tiles();
        scrolling.chain(floating)
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> + '_ {
        let scrolling = self.tiling.tiles_mut();
        let floating = self.floating.tiles_mut();
        scrolling.chain(floating)
    }

    pub fn is_floating(&self, id: &W::Id) -> bool {
        self.floating.has_window(id)
    }

    pub fn window_border(
        &self,
        window: &W::Id,
    ) -> Option<(swayward_ipc::command::BorderStyle, u16)> {
        self.tiles()
            .find(|tile| tile.window().id() == window)
            .map(|tile| tile.sway_border())
    }

    pub fn set_window_border(
        &mut self,
        window: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> Result<(), &'static str> {
        let changed = if self.floating.has_window(window) {
            self.floating.set_window_border(window, style, width)
        } else {
            self.tiling.set_window_border(window, style, width)
        };
        changed
            .then_some(())
            .ok_or("This window doesn't support client side decorations")
    }

    pub fn is_floating_for_ipc(&self, id: &W::Id) -> bool {
        self.floating.has_window(id)
            || self.tiling.tiles().any(|tile| {
                tile.window().id() == id
                    && tile.restore_to_floating
                    && tile.window().pending_sizing_mode().is_fullscreen()
            })
    }

    pub fn current_output(&self) -> Option<&Output> {
        self.output.as_ref()
    }

    pub fn active_window(&self) -> Option<&W> {
        if self.floating_is_active.get() {
            self.floating.active_window()
        } else {
            self.tiling.active_window()
        }
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        if self.floating_is_active.get() {
            self.floating.active_window_mut()
        } else {
            self.tiling.active_window_mut()
        }
    }

    pub fn is_active_pending_fullscreen(&self) -> bool {
        self.tiling.is_active_pending_fullscreen()
    }

    pub fn fullscreen_mode(&self) -> Option<crate::layout::tiling_tree::FullscreenMode> {
        self.tiling
            .fullscreen_node()
            .and_then(|id| self.tiling.fullscreen_mode(id))
    }

    pub fn fullscreen_contains_window(&self, window: &W::Id) -> bool {
        self.tiling.fullscreen_contains_window(window)
    }

    pub fn fullscreen_window(&self) -> Option<&W::Id> {
        self.tiling.fullscreen_window()
    }

    pub fn set_fullscreen_restore_to_floating(&mut self, window: &W::Id) {
        if let Some(tile) = self
            .tiling
            .tiles_mut()
            .find(|tile| tile.window().id() == window)
        {
            tile.restore_to_floating = true;
        }
    }

    pub fn set_window_fullscreen(
        &mut self,
        window: &W::Id,
        mode: Option<crate::layout::tiling_tree::FullscreenMode>,
    ) -> bool {
        if mode.is_some() {
            self.disable_fullscreen();
        }
        self.set_fullscreen(window, mode.is_some());
        let Some(id) = self.tiling.node_for_window(window) else {
            return false;
        };
        self.tiling.set_node_fullscreen(id, mode)
    }

    pub fn disable_fullscreen(&mut self) {
        if let Some(fullscreen) = self.tiling.fullscreen_node() {
            self.tiling.set_node_fullscreen(fullscreen, None);
        }
    }

    pub fn set_focused_fullscreen(
        &mut self,
        mode: Option<crate::layout::tiling_tree::FullscreenMode>,
    ) -> bool {
        if let Some(window) = self.active_window().map(LayoutElement::id).cloned() {
            self.set_fullscreen(&window, mode.is_some());
        }
        let Some(id) = self.tiling.focus() else {
            return false;
        };
        self.tiling.set_node_fullscreen(id, mode)
    }

    pub fn set_output(&mut self, output: Option<Output>) {
        if self.output == output {
            return;
        }

        if let Some(output) = self.output.take() {
            for win in self.windows() {
                win.output_leave(&output);
            }
        }

        self.output = output;

        if let Some(output) = &self.output {
            // Normalize original output: possibly replace connector with make/model/serial.
            if self.original_output.matches(output) {
                self.original_output = OutputId::new(output);
            }

            self.update_output_size();

            for win in self.windows() {
                self.enter_output_for_window(win);
            }
        }
    }

    fn enter_output_for_window(&self, window: &W) {
        if let Some(output) = &self.output {
            window.set_preferred_scale_transform(self.scale, self.transform);
            window.output_enter(output);
        }
    }

    pub fn update_output_size(&mut self) {
        let output = self.output.as_ref().unwrap();
        let scale = output.current_scale();
        let transform = output.current_transform();
        let view_size = output_size(output);
        let output_area = compute_working_area(output);
        let working_area = apply_outer_gaps(
            output_area,
            self.options.layout.outer_gaps,
            self.options.layout.gaps,
            self.options.layout.outer_gaps_configured,
        );
        self.set_view_size(
            scale,
            transform,
            view_size,
            working_area,
            has_gaps_to_edge(
                output_area,
                self.options.layout.outer_gaps,
                self.options.layout.gaps,
            ),
        );
    }

    fn set_view_size(
        &mut self,
        scale: smithay::output::Scale,
        transform: Transform,
        size: Size<f64, Logical>,
        working_area: Rectangle<f64, Logical>,
        gaps_to_edge: bool,
    ) {
        let scale_transform_changed = self.transform != transform
            || self.scale.integer_scale() != scale.integer_scale()
            || self.scale.fractional_scale() != scale.fractional_scale();
        if !scale_transform_changed && self.view_size == size && self.working_area == working_area {
            return;
        }

        let fractional_scale_changed = self.scale.fractional_scale() != scale.fractional_scale();

        self.scale = scale;
        self.transform = transform;
        self.view_size = size;
        self.working_area = working_area;

        if fractional_scale_changed {
            // Options need to be recomputed for the new scale.
            self.update_config(self.base_options.clone());
        } else {
            // Pass our existing options as is.
            self.tiling.update_config(
                size,
                working_area,
                gaps_to_edge,
                scale.fractional_scale(),
                self.options.clone(),
            );
            self.floating.update_config(
                size,
                working_area,
                scale.fractional_scale(),
                self.options.clone(),
            );

            let shadow_config =
                compute_workspace_shadow_config(self.options.overview.workspace_shadow, size);
            self.shadow.update_config(shadow_config);
        }

        self.background_buffer.resize(size);

        if scale_transform_changed {
            for window in self.windows() {
                window.set_preferred_scale_transform(self.scale, self.transform);
            }
        }
    }

    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn make_tile(&self, window: W) -> Tile<W> {
        Tile::new(
            window,
            self.view_size,
            self.scale.fractional_scale(),
            self.clock.clone(),
            self.options.clone(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_tile(
        &mut self,
        mut tile: Tile<W>,
        target: WorkspaceAddWindowTarget<W>,
        activate: ActivateWindow,
        width: ColumnWidth,
        is_full_width: bool,
        is_floating: bool,
        anim: Option<swayward_config::Animation>,
    ) {
        self.enter_output_for_window(tile.window());
        tile.restore_to_floating = is_floating;

        match target {
            WorkspaceAddWindowTarget::Auto => {
                // Don't steal focus from an active fullscreen window.
                let activate = activate.map_smart(|| !self.is_active_pending_fullscreen());

                // If the tile is pending maximized or fullscreen, open it in the scrolling layout
                // where it can do that.
                if is_floating && tile.window().pending_sizing_mode().is_normal() {
                    self.floating.add_tile(tile, activate);

                    if activate || self.tiling.is_empty() {
                        self.floating_is_active = FloatingActive::Yes;
                    }
                } else {
                    let _ = (width, is_full_width, anim);
                    self.tiling
                        .add_tile_with_activation(tile, InsertTarget::Focused, activate);

                    if activate {
                        self.floating_is_active = FloatingActive::No;
                    }
                }
            }
            WorkspaceAddWindowTarget::NewColumnAt(col_idx) => {
                let activate = activate.map_smart(|| false);
                let target = self
                    .tiling
                    .iter_depth_first()
                    .filter_map(|(id, node)| {
                        matches!(node, crate::layout::tiling_tree::TreeNode::Leaf { .. })
                            .then_some(id)
                    })
                    .nth(col_idx)
                    .map(InsertTarget::Node)
                    .unwrap_or(InsertTarget::Focused);
                let _ = (width, is_full_width, anim);
                self.tiling.add_tile_with_activation(tile, target, activate);

                if activate {
                    self.floating_is_active = FloatingActive::No;
                }
            }
            WorkspaceAddWindowTarget::NextTo(next_to) => {
                let activate = activate.map_smart(|| self.active_window().unwrap().id() == next_to);

                let floating_has_window = self.floating.has_window(next_to);

                if is_floating && tile.window().pending_sizing_mode().is_normal() {
                    if floating_has_window {
                        self.floating.add_tile_above(next_to, tile, activate);
                    } else {
                        let (next_to_tile, layout) = self
                            .tiling
                            .tiles_with_ipc_layouts()
                            .find(|(tile, _)| tile.window().id() == next_to)
                            .expect("NextTo parent must be in this workspace's tiling tree");
                        let layout_pos: Point<f64, Logical> =
                            layout.tile_pos_in_workspace_view.unwrap_or_default().into();

                        // Position the new tile in the center above the next_to tile. Think a
                        // dialog opening on top of a window.
                        let tile_size = tile.tile_size();
                        let pos = layout_pos
                            + (next_to_tile.tile_size().to_point() - tile_size.to_point())
                                .downscale(2.);
                        let pos = self.floating.clamp_within_working_area(pos, tile_size);
                        let pos = self.floating.logical_to_size_frac(pos);
                        tile.floating_pos = Some(pos);

                        self.floating.add_tile(tile, activate);
                    }

                    if activate || self.tiling.is_empty() {
                        self.floating_is_active = FloatingActive::Yes;
                    }
                } else if floating_has_window {
                    let _ = (width, is_full_width, anim);
                    self.tiling
                        .add_tile_with_activation(tile, InsertTarget::Focused, activate);

                    if activate {
                        self.floating_is_active = FloatingActive::No;
                    }
                } else {
                    let _ = (width, is_full_width, anim);
                    self.tiling.add_tile_right_of(next_to, tile, activate);

                    if activate {
                        self.floating_is_active = FloatingActive::No;
                    }
                }
            }
        }
    }

    pub fn add_tile_at_drop(
        &mut self,
        tile: Tile<W>,
        target: NodeId,
        edge: ResizeEdge,
        activate: bool,
    ) {
        self.enter_output_for_window(tile.window());
        self.tiling.add_tile_at_drop(tile, target, edge, activate);
        if activate {
            self.floating_is_active = FloatingActive::No;
        }
    }

    pub fn add_tiling_tile(&mut self, tile: Tile<W>, activate: bool) {
        self.enter_output_for_window(tile.window());
        self.tiling
            .add_tile_with_activation(tile, InsertTarget::Focused, activate);
        if activate {
            self.floating_is_active = FloatingActive::No;
        }
    }

    fn update_focus_floating_tiling_after_removing(&mut self, removed_from_floating: bool) {
        let floating = self
            .floating
            .tiles()
            .filter_map(|tile| {
                tile.window()
                    .focus_timestamp()
                    .map(|stamp| (stamp, tile.window().id().clone()))
            })
            .max_by_key(|(stamp, _)| *stamp);
        if let Some((_, id)) = &floating {
            self.floating.activate_window_without_raising(id);
        }
        let tiling = self.tiling.active_window().and_then(|window| {
            window
                .focus_timestamp()
                .map(|stamp| (stamp, window.id().clone()))
        });
        self.floating_is_active = match (floating, tiling) {
            // No floating window carries a focus timestamp, which happens when
            // one has never been focused. Falling straight to No breaks the
            // invariant that floating must be active when the scrolling space
            // is empty but the floating space is not, so check for that case
            // before deciding on timestamps.
            (None, _) if self.tiling.is_empty() && !self.floating.is_empty() => FloatingActive::Yes,
            (None, _) => FloatingActive::No,
            (Some(_), None) => FloatingActive::Yes,
            (Some((floating, _)), Some((tiling, _))) => match floating.cmp(&tiling) {
                std::cmp::Ordering::Greater => FloatingActive::Yes,
                std::cmp::Ordering::Less => {
                    if removed_from_floating {
                        FloatingActive::No
                    } else {
                        FloatingActive::NoButRaised
                    }
                }
                std::cmp::Ordering::Equal => {
                    if removed_from_floating {
                        FloatingActive::No
                    } else {
                        FloatingActive::Yes
                    }
                }
            },
        };
    }

    pub fn remove_tile(&mut self, id: &W::Id, transaction: Transaction) -> RemovedTile<W> {
        let mut from_floating = false;
        let removed = if self.floating.has_window(id) {
            from_floating = true;
            self.floating.remove_tile(id)
        } else {
            let tile = self.tiling.remove_tile(id, transaction).unwrap();
            let is_floating = tile.restore_to_floating;
            RemovedTile {
                tile,
                width: ColumnWidth::Proportion(0.5),
                is_full_width: false,
                is_floating,
                floating_working_area: None,
            }
        };

        if let Some(output) = &self.output {
            removed.tile.window().output_leave(output);
        }

        self.update_focus_floating_tiling_after_removing(from_floating);

        removed
    }

    pub fn detach_tiling_subtree(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, Option<NodeId>)> {
        let detached = self.tiling.detach_subtree(id)?;
        if let Some(output) = &self.output {
            detached
                .0
                .for_each_window(|window| window.output_leave(output));
        }
        self.update_focus_floating_tiling_after_removing(false);
        Some(detached)
    }

    pub fn attach_tiling_subtree(
        &mut self,
        subtree: DetachedSubtree<W>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        self.attach_tiling_subtree_at(subtree, None)
    }

    pub fn move_tiling_subtree_to_node(&mut self, source: NodeId, target: NodeId) -> bool {
        self.tiling.move_subtree_to_node(source, target)
    }

    pub fn swap_tiling_nodes(&mut self, first: NodeId, second: NodeId) -> Result<(), &'static str> {
        self.tiling.swap_nodes(first, second)
    }

    pub fn detach_tiling_subtree_for_swap(
        &mut self,
        id: NodeId,
    ) -> Option<(DetachedSubtree<W>, crate::layout::tiling_tree::DetachedSlot)> {
        let detached = self.tiling.detach_subtree_for_swap(id)?;
        if let Some(output) = &self.output {
            detached
                .0
                .for_each_window(|window| window.output_leave(output));
        }
        Some(detached)
    }

    pub fn attach_tiling_subtree_for_swap(
        &mut self,
        subtree: DetachedSubtree<W>,
        slot: crate::layout::tiling_tree::DetachedSlot,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if let Some(output) = &self.output {
            subtree.for_each_window(|window| window.output_enter(output));
        }
        self.tiling.attach_subtree_for_swap(subtree, slot)
    }

    pub fn sort_tiling_focus_by_timestamp(&mut self) {
        self.tiling.sort_focus_history_by_timestamp();
    }

    pub fn attach_tiling_subtree_at(
        &mut self,
        subtree: DetachedSubtree<W>,
        target: Option<NodeId>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        if let Some(output) = &self.output {
            subtree.for_each_window(|window| window.output_enter(output));
        }
        let fullscreen = subtree.has_fullscreen();
        if fullscreen {
            self.disable_fullscreen();
        }
        self.floating_is_active = FloatingActive::No;
        self.tiling.attach_subtree_at(subtree, target)
    }

    pub fn finish_tiling_subtree_detach(&mut self, old_parent: Option<NodeId>) {
        self.tiling.finish_subtree_detach(old_parent);
    }

    pub fn remove_active_tiling_tile(&mut self) -> Option<Tile<W>> {
        if self.floating_is_active.get() {
            return None;
        }
        let id = self.tiling.active_window()?.id().clone();
        let tile = self.tiling.remove_tile(&id, Transaction::new())?;
        if let Some(output) = &self.output {
            tile.window().output_leave(output);
        }
        self.update_focus_floating_tiling_after_removing(false);
        Some(tile)
    }

    pub fn resolve_default_width(
        &self,
        default_width: Option<Option<PresetSize>>,
        is_floating: bool,
    ) -> Option<PresetSize> {
        match default_width {
            Some(Some(width)) => Some(width),
            Some(None) => None,
            None if is_floating => None,
            None => self.options.layout.default_column_width,
        }
    }

    pub fn resolve_default_height(
        &self,
        default_height: Option<Option<PresetSize>>,
        is_floating: bool,
    ) -> Option<PresetSize> {
        match default_height {
            Some(Some(height)) => Some(height),
            Some(None) => None,
            None if is_floating => None,
            // We don't have a global default at the moment.
            None => None,
        }
    }

    pub fn new_window_size(
        &self,
        width: Option<PresetSize>,
        height: Option<PresetSize>,
        is_floating: bool,
        rules: &ResolvedWindowRules,
        (min_size, max_size): (Size<i32, Logical>, Size<i32, Logical>),
    ) -> Size<i32, Logical> {
        let mut size = if is_floating {
            self.floating.new_window_size(width, height, rules)
        } else {
            self.tiling.new_window_size(height, rules)
        };

        // If the window has a fixed size, or we're picking some fixed size, apply min and max
        // size. This is to ensure that a fixed-size window rule works on open, while still
        // allowing the window freedom to pick its default size otherwise.
        let (min_size, max_size) = rules.apply_min_max_size(min_size, max_size);
        size.w = ensure_min_max_size_maybe_zero(size.w, min_size.w, max_size.w);
        // For scrolling (where height is > 0) only ensure fixed height, since at runtime scrolling
        // will only honor fixed height currently.
        if min_size.h == max_size.h {
            size.h = ensure_min_max_size(size.h, min_size.h, max_size.h);
        } else if size.h > 0 {
            // Also always honor min height, scrolling always does.
            size.h = max(size.h, min_size.h);
        }

        size
    }

    pub fn configure_new_window(
        &self,
        window: &Window,
        width: Option<PresetSize>,
        height: Option<PresetSize>,
        is_floating: bool,
        rules: &ResolvedWindowRules,
    ) {
        window.with_surfaces(|surface, data| {
            send_scale_transform(surface, data, self.scale, self.transform);
        });

        let toplevel = window.toplevel().expect("no x11 support");
        let (min_size, max_size) = with_states(toplevel.wl_surface(), |state| {
            let mut guard = state.cached_state.get::<SurfaceCachedState>();
            let current = guard.current();
            (current.min_size, current.max_size)
        });
        toplevel.with_pending_state(|state| {
            if state.states.contains(xdg_toplevel::State::Fullscreen) {
                state.size = Some(self.view_size.to_i32_round());
            } else if state.states.contains(xdg_toplevel::State::Maximized) {
                state.size = Some(self.working_area.size.to_i32_round());
            } else {
                let size =
                    self.new_window_size(width, height, is_floating, rules, (min_size, max_size));
                state.size = Some(size);
            }

            if is_floating {
                state.bounds = Some(self.floating.new_window_toplevel_bounds(rules));
            } else {
                state.bounds = Some(self.tiling.new_window_toplevel_bounds(rules));
            }
        });
    }

    pub(super) fn resolve_scrolling_width(
        &self,
        window: &W,
        width: Option<PresetSize>,
    ) -> ColumnWidth {
        let width = width.unwrap_or_else(|| PresetSize::Fixed(window.size().w));
        match width {
            PresetSize::Fixed(fixed) => {
                let mut fixed = f64::from(fixed);

                // Add border width since ColumnWidth includes borders.
                let rules = window.rules();
                let border = self.options.layout.border.merged_with(&rules.border);
                if !border.off {
                    fixed += border.width * 2.;
                }

                ColumnWidth::Fixed(fixed)
            }
            PresetSize::Proportion(prop) => ColumnWidth::Proportion(prop),
        }
    }

    pub fn focus_parent(&mut self) -> bool {
        if self.floating_is_active.get() {
            if self.tiling.is_empty() {
                return false;
            }
            self.floating_is_active = FloatingActive::NoButRaised;
            self.tiling.focus_root();
            true
        } else {
            let changed = self.tiling.focus_parent();
            if self.tiling.root_is_focused() {
                self.floating_is_active = FloatingActive::No;
            }
            changed
        }
    }

    pub fn scroll_tab_indicator(&mut self, window: &W::Id, steps: i32) -> Option<W::Id> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.scroll_tab_indicator(window, steps))
            .flatten()
    }

    pub fn focus_next_prev_sibling(&mut self, next: bool) -> bool {
        !self.floating_is_active.get() && self.tiling.focus_next_prev_sibling(next)
    }

    pub fn focus_child(&mut self) -> bool {
        if self.is_workspace_focused() && self.floating_is_active == FloatingActive::NoButRaised {
            self.floating_is_active = FloatingActive::Yes;
            true
        } else {
            !self.floating_is_active.get() && self.tiling.focus_child()
        }
    }

    pub fn focus_tiling_node(&mut self, id: crate::layout::tiling_tree::NodeId) -> bool {
        if self.tiling.contains(id) {
            self.floating_is_active = FloatingActive::No;
            self.tiling.set_focus(id);
            true
        } else {
            false
        }
    }

    pub fn focused_tiling_node(&self) -> Option<crate::layout::tiling_tree::NodeId> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.focus())
            .flatten()
    }

    pub fn focused_container_node(&self) -> Option<crate::layout::tiling_tree::NodeId> {
        if self.floating_is_active == FloatingActive::NoButRaised {
            self.tiling.focus().filter(|id| self.tiling.is_root(*id))
        } else {
            self.focused_tiling_node()
        }
    }

    pub fn is_workspace_focused(&self) -> bool {
        !self.floating_is_active.get() && self.tiling.root_is_focused()
    }

    pub fn set_tiling_node_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        layout: crate::layout::tiling_tree::Layout,
    ) {
        self.tiling.set_layout(id, layout);
    }

    pub fn set_tiling_target_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        layout: crate::layout::tiling_tree::Layout,
    ) -> bool {
        self.tiling.set_target_layout(id, layout)
    }

    pub fn toggle_tiling_target_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        toggle: &swayward_ipc::command::LayoutToggle,
        container: bool,
    ) -> bool {
        if container {
            self.tiling.toggle_node_layout(id, toggle)
        } else {
            self.tiling.toggle_target_layout(id, toggle)
        }
    }

    pub fn restore_tiling_target_layout(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        container: bool,
    ) -> bool {
        if container {
            self.tiling.restore_node_layout(id)
        } else {
            self.tiling.restore_target_layout(id)
        }
    }

    pub fn set_tiling_node_title_format(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        format: String,
    ) -> bool {
        self.tiling.set_title_format(id, format)
    }

    pub fn tiling_node_windows(
        &self,
        id: crate::layout::tiling_tree::NodeId,
    ) -> Option<Vec<W::Id>> {
        self.tiling.contains(id).then(|| {
            self.tiling
                .windows()
                .filter(|(leaf, _)| self.tiling.contains_node(id, *leaf))
                .map(|(_, window)| window.id().clone())
                .collect()
        })
    }

    pub fn focus_from_output_direction(
        &mut self,
        direction: crate::layout::tiling_tree::Direction,
    ) -> bool {
        if self.tiling.is_empty() {
            return false;
        }
        self.floating_is_active = FloatingActive::No;
        self.tiling.focus_from_output_direction(direction)
    }

    pub fn focus_next_or_prev(&mut self, next: bool) -> Option<bool> {
        if self.floating_is_active.get() {
            return None;
        }
        Some(self.tiling.focus_next_or_prev(next))
    }

    pub fn focus_left(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_left()
        } else {
            self.tiling.focus_left()
        }
    }

    pub fn focus_left_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_left()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Left)
        }
    }

    pub fn focus_right(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_right()
        } else {
            self.tiling.focus_right()
        }
    }

    pub fn focus_right_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_right()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Right)
        }
    }

    pub fn focus_column_first(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_leftmost();
        } else {
            self.tiling.focus_column_first();
        }
    }

    pub fn focus_column_last(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_rightmost();
        } else {
            self.tiling.focus_column_last();
        }
    }

    pub fn focus_column_right_or_first(&mut self) {
        if !self.focus_right() {
            self.focus_column_first();
        }
    }

    pub fn focus_column_left_or_last(&mut self) {
        if !self.focus_left() {
            self.focus_column_last();
        }
    }

    pub fn focus_column(&mut self, index: usize) {
        if self.floating_is_active.get() {
            self.focus_tiling();
        }
        self.tiling.focus_column(index);
    }

    pub fn focus_window_in_column(&mut self, index: u8) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.focus_window_in_column(index);
    }

    pub fn focus_down(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_down()
        } else {
            self.tiling.focus_down()
        }
    }

    pub fn focus_down_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_down()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Down)
        }
    }

    pub fn focus_up(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_up()
        } else {
            self.tiling.focus_up()
        }
    }

    pub fn focus_up_without_wrap(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.focus_up()
        } else {
            self.tiling.focus_direction_without_wrap(Direction::Up)
        }
    }

    pub fn focus_down_or_left(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_down();
        } else {
            self.tiling.focus_down_or_left();
        }
    }

    pub fn focus_down_or_right(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_down();
        } else {
            self.tiling.focus_down_or_right();
        }
    }

    pub fn focus_up_or_left(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_up();
        } else {
            self.tiling.focus_up_or_left();
        }
    }

    pub fn focus_up_or_right(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_up();
        } else {
            self.tiling.focus_up_or_right();
        }
    }

    pub fn focus_window_top(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_topmost();
        } else {
            self.tiling.focus_top();
        }
    }

    pub fn focus_window_bottom(&mut self) {
        if self.floating_is_active.get() {
            self.floating.focus_bottommost();
        } else {
            self.tiling.focus_bottom();
        }
    }

    pub fn focus_window_down_or_top(&mut self) {
        if !self.focus_down() {
            self.focus_window_top();
        }
    }

    pub fn focus_window_up_or_bottom(&mut self) {
        if !self.focus_up() {
            self.focus_window_bottom();
        }
    }

    pub fn move_left(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.move_left();
            true
        } else {
            self.tiling.move_left()
        }
    }

    pub fn move_right(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.move_right();
            true
        } else {
            self.tiling.move_right()
        }
    }

    pub fn move_window_in_direction(
        &mut self,
        window: &W::Id,
        direction: Direction,
        pixels: f64,
    ) -> bool {
        if self.floating.has_window(window) {
            let (x, y) = match direction {
                Direction::Left => (-pixels, 0.),
                Direction::Right => (pixels, 0.),
                Direction::Up => (0., -pixels),
                Direction::Down => (0., pixels),
            };
            self.floating.move_window(
                Some(window),
                PositionChange::AdjustFixed(x),
                PositionChange::AdjustFixed(y),
                true,
            );
            true
        } else {
            self.tiling.move_window_direction(window, direction)
        }
    }

    pub fn move_tiling_node_in_direction(&mut self, node: NodeId, direction: Direction) -> bool {
        self.tiling.move_node_direction(node, direction)
    }

    pub fn move_column_to_first(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_column_to_first();
    }

    pub fn move_column_to_last(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_column_to_last();
    }

    pub fn move_column_to_index(&mut self, index: usize) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.move_column_to_index(index);
    }

    pub fn move_down(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.move_down();
            true
        } else {
            self.tiling.move_down()
        }
    }

    pub fn move_up(&mut self) -> bool {
        if self.floating_is_active.get() {
            self.floating.move_up();
            true
        } else {
            self.tiling.move_up()
        }
    }

    pub fn consume_or_expel_window_left(&mut self, window: Option<&W::Id>) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            return;
        }
        self.tiling.consume_or_expel_window_left(window);
    }

    pub fn consume_or_expel_window_right(&mut self, window: Option<&W::Id>) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            return;
        }
        self.tiling.consume_or_expel_window_right(window);
    }

    pub fn consume_into_column(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.consume_into_column();
    }

    pub fn expel_from_column(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.expel_from_column();
    }

    pub fn swap_window_in_direction(&mut self, direction: ScrollDirection) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.swap_window_in_direction(direction);
    }

    pub fn toggle_column_tabbed_display(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.toggle_column_tabbed_display();
    }

    pub fn set_focused_layout(
        &mut self,
        layout: crate::layout::tiling_tree::Layout,
    ) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            Vec::new()
        } else {
            self.tiling.set_focused_layout(layout)
        }
    }

    pub fn split_focused(&mut self, layout: crate::layout::tiling_tree::Layout) {
        if !self.floating_is_active.get() {
            self.tiling.split_focused(layout);
        }
    }

    pub fn flatten_focused_parent(&mut self) -> Option<(NodeId, NodeId)> {
        (!self.floating_is_active.get())
            .then(|| self.tiling.focus())
            .flatten()
            .and_then(|focus| self.tiling.flatten_parent(focus))
    }

    pub fn flatten_tiling_node_parent(&mut self, id: NodeId) -> Option<(NodeId, NodeId)> {
        self.tiling.flatten_parent(id)
    }

    pub fn split_tiling_node(
        &mut self,
        id: crate::layout::tiling_tree::NodeId,
        layout: crate::layout::tiling_tree::Layout,
    ) {
        self.tiling.split(id, layout);
    }

    pub fn toggle_tiling_node_split(&mut self, id: crate::layout::tiling_tree::NodeId) {
        self.tiling.toggle_split(id);
    }

    pub fn toggle_focused_layout(
        &mut self,
        toggle: &swayward_ipc::command::LayoutToggle,
    ) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            Vec::new()
        } else {
            self.tiling.toggle_focused_layout(toggle)
        }
    }

    pub fn restore_focused_split_layout(&mut self) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            Vec::new()
        } else {
            self.tiling.restore_focused_split_layout()
        }
    }

    pub fn toggle_focused_layout_split(&mut self) -> Vec<(NodeId, NodeId)> {
        if self.floating_is_active.get() {
            Vec::new()
        } else {
            self.tiling.toggle_focused_layout_split()
        }
    }

    pub fn toggle_focused_split(&mut self) {
        if !self.floating_is_active.get() {
            self.tiling.toggle_focused_split();
        }
    }

    pub fn set_column_display(&mut self, display: ColumnDisplay) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.set_column_display(display);
    }

    pub fn center_column(&mut self) {
        if self.floating_is_active.get() {
            self.floating.center_window(None);
        } else {
            self.tiling.center_column();
        }
    }

    pub fn center_window(&mut self, id: Option<&W::Id>) {
        if id.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            self.floating.center_window(id);
        } else {
            self.tiling.center_window(id);
        }
    }

    pub fn center_visible_columns(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.center_visible_columns();
    }

    pub fn toggle_width(&mut self, forwards: bool) {
        if self.floating_is_active.get() {
            self.floating.toggle_window_width(None, forwards);
        } else {
            self.tiling.toggle_width(forwards);
        }
    }

    pub fn toggle_full_width(&mut self) {
        if self.floating_is_active.get() {
            // Leave this unimplemented for now. For good UX, this probably needs moving the tile
            // to be against the left edge of the working area while it is full-width.
            return;
        }
        self.tiling.toggle_full_width();
    }

    pub fn set_column_width(&mut self, change: SizeChange) {
        if self.floating_is_active.get() {
            self.floating
                .set_window_width(None, change, true, self.view_size.to_i32_round());
        } else {
            self.tiling.set_window_width(None, change);
        }
    }

    pub fn set_window_width(
        &mut self,
        window: Option<&W::Id>,
        change: SizeChange,
        automatic_maximum: Size<i32, Logical>,
    ) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            self.floating
                .set_window_width(window, change, true, automatic_maximum);
        } else {
            self.tiling.set_window_width(window, change);
        }
    }

    pub fn resize_tiling_node(
        &mut self,
        node: crate::layout::tiling_tree::NodeId,
        width: bool,
        change: SizeChange,
    ) {
        self.tiling
            .resize_node_dimension_command(node, width, change);
    }

    pub fn resize_tiling_node_edge(
        &mut self,
        node: crate::layout::tiling_tree::NodeId,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> bool {
        self.tiling.resize_node_edge_command(node, edge, change)
    }

    pub fn set_tiling_node_size_sway(
        &mut self,
        node: crate::layout::tiling_tree::NodeId,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
    ) {
        self.tiling.set_node_size_sway(node, width, height);
    }

    pub fn set_window_size_sway(
        &mut self,
        window: &W::Id,
        width: Option<SizeChange>,
        height: Option<SizeChange>,
        automatic_maximum: Size<i32, Logical>,
    ) {
        if self.is_floating(window) {
            if let Some(change) = width {
                self.floating
                    .set_window_outer_width(window, change, automatic_maximum);
            }
            if let Some(change) = height {
                self.floating
                    .set_window_outer_height(window, change, automatic_maximum);
            }
        } else {
            self.tiling.set_window_size_sway(window, width, height);
        }
    }

    pub fn set_window_height(
        &mut self,
        window: Option<&W::Id>,
        change: SizeChange,
        automatic_maximum: Size<i32, Logical>,
    ) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            self.floating
                .set_window_height(window, change, true, automatic_maximum);
        } else {
            self.tiling.set_window_height(window, change);
        }
    }

    pub fn resize_window_edge(
        &mut self,
        window: Option<&W::Id>,
        edge: ResizeEdge,
        change: SizeChange,
    ) -> Option<bool> {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            Some(self.floating.resize_window_edge(window, edge, change))
        } else {
            Some(self.tiling.resize_window_edge(window, edge, change))
        }
    }

    pub fn reset_window_height(&mut self, window: Option<&W::Id>) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            return;
        }
        self.tiling.reset_window_height(window);
    }

    pub fn toggle_window_width(&mut self, window: Option<&W::Id>, forwards: bool) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            self.floating.toggle_window_width(window, forwards);
        } else {
            self.tiling.toggle_window_width(window, forwards);
        }
    }

    pub fn toggle_window_height(&mut self, window: Option<&W::Id>, forwards: bool) {
        if window.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            self.floating.toggle_window_height(window, forwards);
        } else {
            self.tiling.toggle_window_height(window, forwards);
        }
    }

    pub fn expand_column_to_available_width(&mut self) {
        if self.floating_is_active.get() {
            return;
        }
        self.tiling.expand_column_to_available_width();
    }

    pub fn set_fullscreen(&mut self, window: &W::Id, is_fullscreen: bool) {
        let mut restore_to_floating = false;
        if self.floating.has_window(window) {
            if is_fullscreen {
                restore_to_floating = true;
                self.toggle_window_floating(Some(window));
            } else {
                // Floating windows are never fullscreen, so this is an unfullscreen request for an
                // already unfullscreen window.
                return;
            }
        } else if !is_fullscreen {
            // The window is in the scrolling layout and we're requesting an unfullscreen. If it is
            // indeed fullscreen (i.e. this isn't a duplicate unfullscreen request), then we may
            // need to unfullscreen into floating.
            // When going from fullscreen to maximized, don't consider restore_to_floating yet.
            if self.tiling.is_pending_fullscreen(window)
                && !self.tiling.is_pending_maximized(window)
            {
                let tile = self
                    .tiling
                    .tiles()
                    .find(|tile| tile.window().id() == window)
                    .unwrap();
                if tile.restore_to_floating {
                    // Unfullscreen and float in one call so it has a chance to notice and request a
                    // (0, 0) size, rather than the scrolling column size.
                    self.toggle_window_floating(Some(window));
                    return;
                }
            }
        }

        // The window need not be in the tiling layout here. Moving a
        // fullscreen window between workspaces can run this after the window
        // has already left, and toggle_window_floating above is not guaranteed
        // to have placed it in tiling either. Unwrapping crashed the
        // compositor; there is simply nothing to fullscreen.
        let Some(tile) = self
            .tiling
            .tiles()
            .find(|tile| tile.window().id() == window)
        else {
            return;
        };
        let was_normal = tile.window().pending_sizing_mode().is_normal();

        self.tiling.set_fullscreen(window, is_fullscreen);

        // When going from normal to fullscreen, remember if we should unfullscreen to floating.
        let Some(tile) = self
            .tiling
            .tiles_mut()
            .find(|tile| tile.window().id() == window)
        else {
            return;
        };
        if was_normal && !tile.window().pending_sizing_mode().is_normal() {
            tile.restore_to_floating = restore_to_floating;
        }
    }

    pub fn toggle_fullscreen(&mut self, window: &W::Id) {
        let tile = self
            .tiles()
            .find(|tile| tile.window().id() == window)
            .unwrap();
        let current = tile.window().pending_sizing_mode().is_fullscreen();
        self.set_fullscreen(window, !current);
    }

    pub fn set_maximized(&mut self, window: &W::Id, maximize: bool) {
        let mut restore_to_floating = false;
        if self.floating.has_window(window) {
            if maximize {
                restore_to_floating = true;
                self.toggle_window_floating(Some(window));
            } else {
                // Floating windows are never maximized, so this is an unmaximize request for an
                // already unmaximized window.
                return;
            }
        } else if !maximize {
            // The window is in the scrolling layout and we're requesting to unmaximize. If it is
            // indeed maximized (i.e. this isn't a duplicate unmaximize request), then we may
            // need to unmaximize into floating.
            let tile = self
                .tiling
                .tiles()
                .find(|tile| tile.window().id() == window)
                .unwrap();
            // The tile cannot unmaximize into fullscreen (pending_sizing_mode() will be fullscreen
            // in that case and not maximized), so this check works.
            if tile.window().pending_sizing_mode().is_maximized() && tile.restore_to_floating {
                // Unmaximize and float in one call so it has a chance to notice and request a
                // (0, 0) size, rather than the scrolling column size.
                self.toggle_window_floating(Some(window));
                return;
            }
        }

        let tile = self
            .tiling
            .tiles()
            .find(|tile| tile.window().id() == window)
            .unwrap();
        let was_normal = tile.window().pending_sizing_mode().is_normal();

        self.tiling.set_maximized(window, maximize);

        // When going from normal to maximized, remember if we should unmaximize to floating.
        let tile = self
            .tiling
            .tiles_mut()
            .find(|tile| tile.window().id() == window)
            .unwrap();
        if was_normal && !tile.window().pending_sizing_mode().is_normal() {
            tile.restore_to_floating = restore_to_floating;
        }
    }

    pub fn toggle_maximized(&mut self, window: &W::Id) {
        // Pending maximize is compositor-side state and remains meaningful while fullscreen.
        let current = self.tiling.is_pending_maximized(window);

        self.set_maximized(window, !current);
    }

    pub(super) fn prepare_tiled_window_for_scratchpad(
        &mut self,
        id: &W::Id,
        automatic_maximum: Size<i32, Logical>,
    ) {
        let Some(tile) = self
            .tiling
            .tiles_mut()
            .find(|tile| tile.window().id() == id)
        else {
            return;
        };

        // Sway sizes a tiled window from the workspace box when it first enters
        // the scratchpad (sway/tree/container.c:913-932).
        let minimum = self.options.layout.floating_minimum_size;
        let maximum = self.options.layout.floating_maximum_size;
        let min_width = if minimum.width == -1 {
            0.
        } else if minimum.width == 0 {
            75.
        } else {
            f64::from(minimum.width)
        };
        let min_height = if minimum.height == -1 {
            0.
        } else if minimum.height == 0 {
            50.
        } else {
            f64::from(minimum.height)
        };
        let max_width = if maximum.width == -1 {
            f64::INFINITY
        } else if maximum.width == 0 {
            f64::from(automatic_maximum.w)
        } else {
            f64::from(maximum.width)
        };
        let max_height = if maximum.height == -1 {
            f64::INFINITY
        } else if maximum.height == 0 {
            f64::from(automatic_maximum.h)
        } else {
            f64::from(maximum.height)
        };
        let tile_width = (self.working_area.size.w * 0.5)
            .min(max_width)
            .max(min_width);
        let tile_height = (self.working_area.size.h * 0.75)
            .min(max_height)
            .max(min_height);
        let min_size = tile.window().min_size();
        let max_size = tile.window().max_size();
        let window_width = ensure_min_max_size(
            tile.window_width_for_tile_width(tile_width).round() as i32,
            min_size.w,
            max_size.w,
        );
        let window_height = ensure_min_max_size(
            tile.window_height_for_tile_height(tile_height).round() as i32,
            min_size.h,
            max_size.h,
        );
        tile.floating_window_size = Some(Size::from((window_width, window_height)));

        let tile_size = Size::from((
            tile.tile_width_for_window_width(f64::from(window_width)),
            tile.tile_height_for_window_height(f64::from(window_height)),
        ));
        let pos = self.working_area.loc
            + (self.working_area.size.to_point() - tile_size.to_point()).downscale(2.);
        tile.floating_pos = Some(self.floating.logical_to_size_frac(pos));
    }

    pub fn remap_floating_position(
        &self,
        tile: &mut Tile<W>,
        old_area: Option<Rectangle<f64, Logical>>,
    ) {
        self.floating.remap_stored_tile_pos(tile, old_area);
    }

    pub fn toggle_window_floating(&mut self, id: Option<&W::Id>) {
        let active_id = self.active_window().map(|win| win.id().clone());
        let target_is_active = id.is_none_or(|id| Some(id) == active_id.as_ref());
        let Some(id) = id.cloned().or(active_id) else {
            return;
        };

        let (_, render_pos, _) = self
            .tiles_with_render_positions()
            .find(|(tile, _, _)| *tile.window().id() == id)
            .unwrap();

        if self.floating.has_window(&id) {
            let removed = self.floating.remove_tile(&id);
            // FIXME: compute closest pos?
            let _ = (removed.width, removed.is_full_width);
            let rank = removed.tile.tiling_focus_rank;
            let parent = removed.tile.tiling_parent;
            if let Some(parent) = parent.filter(|parent| self.tiling.contains(*parent)) {
                self.tiling
                    .add_tile_to_existing_parent(removed.tile, parent, target_is_active);
            } else {
                self.tiling.add_tile_with_activation(
                    removed.tile,
                    InsertTarget::Focused,
                    target_is_active,
                );
            }
            if let Some(rank) = rank {
                self.tiling.restore_focus_rank(&id, rank);
            }
            if target_is_active {
                self.floating_is_active = FloatingActive::No;
            }
        } else {
            let rank = self.tiling.focus_rank_for_window(&id);
            let parent = self.tiling.non_root_parent_for_window(&id);
            let mut tile = if parent.is_some() {
                self.tiling.remove_tile_preserving_parent(&id).unwrap()
            } else {
                self.tiling.remove_tile(&id, Transaction::new()).unwrap()
            };
            tile.tiling_focus_rank = rank;
            tile.tiling_parent = parent;
            tile.stop_move_animations();

            let natural_size = tile.window().natural_size();
            if tile.floating_window_size.is_none()
                && tile.window().pending_sizing_mode().is_normal()
                && natural_size.w > 1
                && natural_size.h > 1
            {
                tile.floating_window_size = Some(natural_size);
            }

            // Come up with a default floating position close to the tile position.
            let stored_or_default = self.floating.stored_or_default_tile_pos(&tile);
            if stored_or_default.is_none() {
                let offset = Point::from((50., 50.));
                let pos = if self.tiling.is_empty() {
                    let size = tile.tile_size().to_point();
                    (self.view_size.to_point() - size).downscale(2.)
                } else {
                    self.floating
                        .clamp_within_working_area(render_pos + offset, tile.tile_size())
                };
                tile.floating_pos = Some(self.floating.logical_to_size_frac(pos));
            }

            self.floating.add_tile(tile, target_is_active);
            if target_is_active {
                self.floating_is_active = FloatingActive::Yes;
            }
        }

        let (tile, new_render_pos) = self
            .tiles_with_render_positions_mut(false)
            .find(|(tile, _)| *tile.window().id() == id)
            .unwrap();

        tile.animate_move_from(render_pos - new_render_pos);
    }

    pub fn set_window_floating(&mut self, id: Option<&W::Id>, floating: bool) {
        if id.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) == floating
        {
            return;
        }

        self.toggle_window_floating(id);
    }

    pub fn focus_floating(&mut self) {
        let recent = self
            .floating
            .tiles()
            .filter_map(|tile| {
                tile.window()
                    .focus_timestamp()
                    .map(|stamp| (stamp, tile.window().id().clone()))
            })
            .max_by_key(|(stamp, _)| *stamp)
            .map(|(_, id)| id);
        if let Some(recent) = recent {
            self.floating.activate_window_without_raising(&recent);
            self.floating_is_active = FloatingActive::Yes;
        }
    }

    pub fn focus_tiling(&mut self) {
        let recent = self
            .tiling
            .tiles()
            .filter_map(|tile| {
                tile.window()
                    .focus_timestamp()
                    .map(|stamp| (stamp, tile.window().id().clone()))
            })
            .max_by_key(|(stamp, _)| *stamp)
            .map(|(_, id)| id);
        if let Some(recent) = recent {
            self.tiling.activate_window(&recent);
            self.floating_is_active = FloatingActive::No;
        }
    }

    pub fn switch_focus_floating_tiling(&mut self) {
        if self.floating.is_empty() {
            // If floating is empty, keep focus on scrolling.
            return;
        } else if self.tiling.is_empty() {
            // If floating isn't empty but scrolling is, keep focus on floating.
            return;
        }

        self.floating_is_active = if self.floating_is_active.get() {
            FloatingActive::No
        } else {
            FloatingActive::Yes
        };
    }

    pub fn move_floating_window(
        &mut self,
        id: Option<&W::Id>,
        x: PositionChange,
        y: PositionChange,
        animate: bool,
    ) {
        if id.map_or(self.floating_is_active.get(), |id| {
            self.floating.has_window(id)
        }) {
            self.floating.move_window(id, x, y, animate);
        } else {
            // If the target tile isn't floating, set its stored floating position.
            let tile = if let Some(id) = id {
                self.tiling
                    .tiles_mut()
                    .find(|tile| tile.window().id() == id)
                    .unwrap()
            } else if let Some(tile) = self.tiling.active_tile_mut() {
                tile
            } else {
                return;
            };

            let pos = self.floating.stored_or_default_tile_pos(tile);

            // If there's no stored floating position, we can only set both components at once, not
            // adjust.
            let pos = pos.or_else(|| {
                (matches!(
                    x,
                    PositionChange::SetFixed(_) | PositionChange::SetProportion(_)
                ) && matches!(
                    y,
                    PositionChange::SetFixed(_) | PositionChange::SetProportion(_)
                ))
                .then_some(Point::default())
            });

            let Some(mut pos) = pos else {
                return;
            };

            let working_area = self.floating.working_area();
            let available_width = working_area.size.w;
            let available_height = working_area.size.h;
            let working_area_loc = working_area.loc;

            pos.x = apply_position_change(pos.x, x, available_width, working_area_loc.x);
            pos.y = apply_position_change(pos.y, y, available_height, working_area_loc.y);

            let pos = self.floating.logical_to_size_frac(pos);
            tile.floating_pos = Some(pos);
        }
    }

    pub fn has_windows(&self) -> bool {
        self.windows().next().is_some()
    }

    pub fn has_non_sticky_windows(&self) -> bool {
        !self.tiling.is_empty() || self.floating.tiles().any(|tile| !tile.is_sticky)
    }

    pub fn is_window_sticky(&self, window: &W::Id) -> bool {
        self.tiles()
            .find(|tile| tile.window().id() == window)
            .is_some_and(|tile| tile.is_sticky)
    }

    pub fn set_window_sticky(&mut self, window: &W::Id, sticky: bool) -> bool {
        let Some(tile) = self.tiles_mut().find(|tile| tile.window().id() == window) else {
            return false;
        };
        tile.is_sticky = sticky;
        true
    }

    pub fn take_sticky_tiles(&mut self) -> Vec<RemovedTile<W>> {
        let ids = self
            .floating
            .tiles()
            .filter(|tile| tile.is_sticky)
            .map(|tile| tile.window().id().clone())
            .collect::<Vec<_>>();
        ids.iter()
            .map(|id| self.remove_tile(id, Transaction::new()))
            .collect()
    }

    pub fn has_window(&self, window: &W::Id) -> bool {
        self.windows().any(|win| win.id() == window)
    }

    pub fn find_wl_surface(&self, wl_surface: &WlSurface) -> Option<&W> {
        self.windows().find(|win| win.is_wl_surface(wl_surface))
    }

    pub fn find_wl_surface_mut(&mut self, wl_surface: &WlSurface) -> Option<&mut W> {
        self.windows_mut().find(|win| win.is_wl_surface(wl_surface))
    }

    pub fn tiles_with_render_positions(
        &self,
    ) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>, bool)> {
        let scrolling = self.tiling.tiles_with_render_positions();

        let floating = self.floating.tiles_with_render_positions();
        let visible = self.is_floating_visible();
        let floating = floating.map(move |(tile, pos)| (tile, pos, visible));

        floating.chain(scrolling)
    }

    pub fn tiles_with_render_positions_mut(
        &mut self,
        round: bool,
    ) -> impl Iterator<Item = (&mut Tile<W>, Point<f64, Logical>)> {
        let scrolling = self.tiling.tiles_with_render_positions_mut(round);
        let floating = self.floating.tiles_with_render_positions_mut(round);
        floating.chain(scrolling)
    }

    pub fn contains_tiling_node(&self, id: crate::layout::tiling_tree::NodeId) -> bool {
        self.tiling.contains(id)
    }

    pub fn tiling_node_for_window(
        &self,
        window: &W::Id,
    ) -> Option<crate::layout::tiling_tree::NodeId> {
        self.tiling.node_for_window(window)
    }

    pub fn tiling_window_for_node(&self, node: NodeId) -> Option<&W> {
        self.tiling.window_for_node(node)
    }

    pub fn is_tiling_split(&self, id: crate::layout::tiling_tree::NodeId) -> bool {
        self.tiling.is_split(id)
    }

    pub fn tab_indicator_focus_target(&self, window: &W::Id) -> Option<&W> {
        self.tiling.tab_indicator_focus_target(window)
    }

    pub fn ipc_tiling_tree(&self) -> super::tiling_tree::IpcNode<W::Id> {
        let mut tree = self.tiling.ipc_tree();
        tree.retain_leaves(&|window| !self.is_floating_for_ipc(window));
        tree
    }

    pub fn ipc_decoration_rect(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        self.tiling.ipc_decoration_rect(window)
    }

    pub fn tiles_with_ipc_layouts(&self) -> impl Iterator<Item = (&Tile<W>, WindowLayout)> {
        let scrolling = self.tiling.tiles_with_ipc_layouts();
        let floating = self.floating.tiles_with_ipc_layouts();
        floating.chain(scrolling)
    }

    pub fn active_window_visual_rectangle(&self) -> Option<Rectangle<f64, Logical>> {
        if self.floating_is_active.get() {
            self.floating.active_window_visual_rectangle()
        } else {
            self.tiling.active_window_visual_rectangle()
        }
    }

    pub fn popup_target_rect(&self, window: &W::Id) -> Option<Rectangle<f64, Logical>> {
        if self.floating.has_window(window) {
            self.floating.popup_target_rect(window)
        } else {
            self.tiling.popup_target_rect(window)
        }
    }

    pub fn render_scrolling<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(WorkspaceRenderElement<R>),
    ) {
        let scrolling_focus_ring = focus_ring && !self.floating_is_active();
        self.tiling
            .render(ctx, xray_pos, scrolling_focus_ring, layer, &mut |elem| {
                push(elem.into())
            });
    }

    pub fn render_floating<R: NiriRenderer>(
        &self,
        ctx: RenderCtx<R>,
        xray_pos: XrayPos,
        focus_ring: bool,
        layer: RenderLayer,
        push: &mut dyn FnMut(WorkspaceRenderElement<R>),
    ) {
        if !self.is_floating_visible() && layer.is_normal() {
            return;
        }

        let view_rect = Rectangle::from_size(self.view_size);
        let floating_focus_ring = focus_ring && self.floating_is_active();
        self.floating.render(
            ctx,
            xray_pos,
            view_rect,
            floating_focus_ring,
            layer,
            &mut |elem| push(elem.into()),
        );
    }

    pub fn render_shadow<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        push: &mut dyn FnMut(ShadowRenderElement),
    ) {
        self.shadow.render(renderer, Point::from((0., 0.)), push);
    }

    pub fn render_background(&self) -> SolidColorRenderElement {
        SolidColorRenderElement::from_buffer(
            &self.background_buffer,
            Point::new(0., 0.),
            1.,
            Kind::Unspecified,
        )
    }

    pub fn render_above_top_layer(&self) -> bool {
        self.tiling.render_above_top_layer()
    }

    pub fn is_floating_visible(&self) -> bool {
        // If the focus is on a fullscreen scrolling window, hide the floating windows.
        matches!(
            self.floating_is_active,
            FloatingActive::Yes | FloatingActive::NoButRaised
        ) || !self.render_above_top_layer()
    }

    pub fn store_unmap_snapshot_if_empty(
        &mut self,
        renderer: &mut GlesRenderer,
        xray: Option<&mut Xray>,
        xray_has_blocked_out_layers: bool,
        xray_pos: XrayPos,
        window: &W::Id,
    ) {
        let view_size = self.view_size();
        for (tile, tile_pos) in self.tiles_with_render_positions_mut(false) {
            if tile.window().id() == window {
                let view_pos = Point::from((-tile_pos.x, -tile_pos.y));
                let view_rect = Rectangle::new(view_pos, view_size);
                tile.update_render_elements(false, view_rect);
                let xray_pos = xray_pos.offset(tile_pos);
                tile.store_unmap_snapshot_if_empty(
                    renderer,
                    xray,
                    xray_has_blocked_out_layers,
                    xray_pos,
                );
                return;
            }
        }
    }

    pub fn clear_unmap_snapshot(&mut self, window: &W::Id) {
        for tile in self.tiles_mut() {
            if tile.window().id() == window {
                let _ = tile.take_unmap_snapshot();
                return;
            }
        }
    }

    pub fn start_close_animation_for_window(
        &mut self,
        renderer: &mut GlesRenderer,
        window: &W::Id,
        blocker: TransactionBlocker,
    ) {
        if self.floating.has_window(window) {
            self.floating
                .start_close_animation_for_window(renderer, window, blocker);
        } else {
            self.tiling
                .start_close_animation_for_window(renderer, window, blocker);
        }
    }

    pub fn start_close_animation_for_tile(
        &mut self,
        renderer: &mut GlesRenderer,
        snapshot: TileRenderSnapshot,
        tile_size: Size<f64, Logical>,
        tile_pos: Point<f64, Logical>,
        blocker: TransactionBlocker,
    ) {
        self.floating
            .start_close_animation_for_tile(renderer, snapshot, tile_size, tile_pos, blocker);
    }

    pub fn start_open_animation(&mut self, id: &W::Id) -> bool {
        self.tiling.start_open_animation(id) || self.floating.start_open_animation(id)
    }

    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<(&W, HitType)> {
        // This logic is consistent with tiles_with_render_positions().
        if self.is_floating_visible() {
            if let Some(rv) = self.floating.window_under(pos) {
                return Some(rv);
            }
        }

        self.tiling.window_under(pos)
    }

    pub fn resize_edges_under(&self, pos: Point<f64, Logical>) -> Option<ResizeEdge> {
        self.tiles_with_render_positions()
            .find_map(|(tile, tile_pos, visible)| {
                // This logic should be consistent with window_under() in when it returns Some vs.
                // None.
                if !visible {
                    return None;
                }

                let pos_within_tile = pos - tile_pos;

                if tile.hit(pos_within_tile).is_some() {
                    let size = tile.tile_size().to_f64();

                    let mut edges = ResizeEdge::empty();
                    if pos_within_tile.x < size.w / 3. {
                        edges |= ResizeEdge::LEFT;
                    } else if 2. * size.w / 3. < pos_within_tile.x {
                        edges |= ResizeEdge::RIGHT;
                    }
                    if pos_within_tile.y < size.h / 3. {
                        edges |= ResizeEdge::TOP;
                    } else if 2. * size.h / 3. < pos_within_tile.y {
                        edges |= ResizeEdge::BOTTOM;
                    }
                    return Some(edges);
                }

                None
            })
    }

    pub fn descendants_added(&mut self, id: &W::Id) -> bool {
        self.floating.descendants_added(id)
    }

    pub fn update_window(&mut self, window: &W::Id, serial: Option<Serial>) {
        if !self.floating.update_window(window, serial) {
            self.tiling.update_window(window, serial);
        }
    }

    pub fn refresh(&mut self, is_active: bool, is_focused: bool) {
        self.tiling
            .refresh(is_active && !self.floating_is_active.get(), is_focused);
        self.floating
            .refresh(is_active && self.floating_is_active.get(), is_focused);
    }

    pub fn scroll_amount_to_activate(&self, window: &W::Id) -> f64 {
        if self.floating.has_window(window) {
            return 0.;
        }

        self.tiling.scroll_amount_to_activate(window)
    }

    pub fn is_urgent(&self) -> bool {
        self.windows().any(|win| win.is_urgent())
    }

    pub fn activate_window(&mut self, window: &W::Id) -> bool {
        if self.floating.activate_window(window) {
            self.floating_is_active = FloatingActive::Yes;
            true
        } else if self.tiling.activate_window(window) {
            self.floating_is_active = FloatingActive::No;
            true
        } else {
            false
        }
    }

    pub fn activate_window_without_raising(&mut self, window: &W::Id) -> bool {
        if self.floating.activate_window_without_raising(window) {
            self.floating_is_active = FloatingActive::Yes;
            true
        } else if self.tiling.activate_window(window) {
            self.floating_is_active = match self.floating_is_active {
                FloatingActive::No => FloatingActive::No,
                FloatingActive::NoButRaised => FloatingActive::NoButRaised,
                FloatingActive::Yes => FloatingActive::NoButRaised,
            };
            true
        } else {
            false
        }
    }

    pub(super) fn scrolling_insert_position(&self, pos: Point<f64, Logical>) -> InsertPosition {
        match self.tiling.tiled_drop_target(pos) {
            Some((target, edge)) if edge.is_empty() => InsertPosition::SwapWith(target),
            Some((target, edge)) => InsertPosition::InsertAt(target, edge),
            None => InsertPosition::NewColumn(self.tiling.windows().count()),
        }
    }

    pub(super) fn insert_hint_area(
        &self,
        position: InsertPosition,
    ) -> Option<Rectangle<f64, Logical>> {
        match position {
            InsertPosition::NewColumn(index) => {
                let positions = self
                    .tiling
                    .tiles_with_render_positions()
                    .collect::<Vec<_>>();
                let x = positions
                    .get(index)
                    .map(|(_, pos, _)| pos.x)
                    .or_else(|| {
                        positions
                            .last()
                            .map(|(tile, pos, _)| pos.x + tile.tile_size().w)
                    })
                    .unwrap_or(self.working_area.loc.x);
                Some(Rectangle::new(
                    Point::from((x - self.options.layout.gaps / 2., self.working_area.loc.y)),
                    Size::from((self.options.layout.gaps.max(2.), self.working_area.size.h)),
                ))
            }
            InsertPosition::InsertAt(target, edge) => {
                let mut rect = self.tiling.node_geometry(target)?;
                let thickness = rect.size.w.min(rect.size.h) * 0.3;
                if edge == ResizeEdge::LEFT {
                    rect.size.w = thickness;
                } else if edge == ResizeEdge::RIGHT {
                    rect.loc.x += rect.size.w - thickness;
                    rect.size.w = thickness;
                } else if edge == ResizeEdge::TOP {
                    rect.size.h = thickness;
                } else {
                    rect.loc.y += rect.size.h - thickness;
                    rect.size.h = thickness;
                }
                Some(rect)
            }
            InsertPosition::SwapWith(target) => self.tiling.node_geometry(target),
            InsertPosition::Floating => None,
        }
    }

    pub fn view_offset_gesture_begin(&mut self, is_touchpad: bool) {
        self.tiling.view_offset_gesture_begin(is_touchpad);
    }

    pub fn view_offset_gesture_update(
        &mut self,
        delta_x: f64,
        timestamp: Duration,
        is_touchpad: bool,
    ) -> Option<bool> {
        self.tiling
            .view_offset_gesture_update(delta_x, timestamp, is_touchpad)
    }

    pub fn view_offset_gesture_end(&mut self, is_touchpad: Option<bool>) -> bool {
        self.tiling.view_offset_gesture_end(is_touchpad)
    }

    pub fn dnd_scroll_gesture_begin(&mut self) {
        self.tiling.dnd_scroll_gesture_begin();
    }

    pub fn dnd_scroll_gesture_scroll(&mut self, pos: Point<f64, Logical>, speed: f64) -> bool {
        let config = &self.options.gestures.dnd_edge_view_scroll;
        let trigger_width = config.trigger_width;

        // This working area intentionally does not include extra struts from Options.
        let x = pos.x - self.working_area.loc.x;
        let width = self.working_area.size.w;

        let x = x.clamp(0., width);
        let trigger_width = trigger_width.clamp(0., width / 2.);

        let delta = if x < trigger_width {
            -(trigger_width - x)
        } else if width - x < trigger_width {
            trigger_width - (width - x)
        } else {
            0.
        };

        let delta = if trigger_width < 0.01 {
            // Sanity check for trigger-width 0 or small window sizes.
            0.
        } else {
            // Normalize to [0, 1].
            delta / trigger_width
        };
        let delta = delta * speed;

        self.tiling.dnd_scroll_gesture_scroll(delta)
    }

    pub fn dnd_scroll_gesture_end(&mut self) {
        self.tiling.dnd_scroll_gesture_end();
    }

    pub fn interactive_resize_begin(&mut self, window: W::Id, edges: ResizeEdge) -> bool {
        if self.floating.has_window(&window) {
            self.floating.interactive_resize_begin(window, edges)
        } else {
            self.tiling.interactive_resize_begin(window, edges)
        }
    }

    pub fn interactive_resize_update(
        &mut self,
        window: &W::Id,
        delta: Point<f64, Logical>,
    ) -> bool {
        if self.floating.has_window(window) {
            self.floating.interactive_resize_update(window, delta)
        } else {
            self.tiling.interactive_resize_update(window, delta)
        }
    }

    pub fn interactive_resize_end(&mut self, window: Option<&W::Id>) {
        if let Some(window) = window {
            if self.floating.has_window(window) {
                self.floating.interactive_resize_end(Some(window));
            } else {
                self.tiling.interactive_resize_end(Some(window));
            }
        } else {
            self.floating.interactive_resize_end(None);
            self.tiling.interactive_resize_end(None);
        }
    }

    pub fn floating_is_active(&self) -> bool {
        self.floating_is_active.get()
    }

    pub fn active_floating_is_fullscreen(&self) -> bool {
        self.tiling.active_tile().is_some_and(|tile| {
            tile.restore_to_floating && tile.window().pending_sizing_mode().is_fullscreen()
        })
    }

    pub fn floating_logical_to_size_frac(
        &self,
        logical_pos: Point<f64, Logical>,
    ) -> Point<f64, SizeFrac> {
        self.floating.logical_to_size_frac(logical_pos)
    }

    pub fn working_area(&self) -> Rectangle<f64, Logical> {
        self.working_area
    }

    pub fn layout_config(&self) -> Option<&swayward_config::LayoutPart> {
        self.layout_config.as_ref()
    }

    pub fn tiling(&self) -> &TilingTree<W> {
        &self.tiling
    }

    pub fn tiling_mut(&mut self) -> &mut TilingTree<W> {
        &mut self.tiling
    }

    pub fn floating(&self) -> &FloatingSpace<W> {
        &self.floating
    }

    #[cfg(test)]
    pub fn verify_invariants(&self, move_win_id: Option<&W::Id>) {
        use approx::assert_abs_diff_eq;

        let scale = self.scale.fractional_scale();
        assert!(scale > 0.);
        assert!(scale.is_finite());

        let mut options =
            Options::clone(&self.base_options).with_merged_layout(self.layout_config.as_ref());
        options.layout.gaps = self.gaps;
        options.layout.outer_gaps = self.outer_gaps;
        options.layout.outer_gaps_configured = self.outer_gaps_configured;
        let options = options.adjusted_for_scale(scale);
        assert_eq!(
            &*self.options, &options,
            "options must be base options adjusted for scale"
        );

        assert!(self.view_size.w > 0.);
        assert!(self.view_size.h > 0.);

        assert_eq!(self.background_buffer.size(), self.view_size);
        assert_eq!(
            self.background_buffer.color().components(),
            options.layout.background_color.to_array_unpremul(),
        );

        assert_eq!(self.view_size, self.tiling.view_size());
        assert_eq!(self.working_area, self.tiling.parent_area());
        assert_eq!(&self.clock, self.tiling.clock());
        assert!(Rc::ptr_eq(&self.options, self.tiling.options()));
        self.tiling.verify_invariants();

        assert_eq!(self.view_size, self.floating.view_size());
        assert_eq!(self.working_area, self.floating.working_area());
        assert_eq!(&self.clock, self.floating.clock());
        assert!(Rc::ptr_eq(&self.options, self.floating.options()));
        self.floating.verify_invariants();

        if self.floating.is_empty() {
            assert!(
                !self.floating_is_active.get(),
                "when floating is empty it must never be active"
            );
        } else if self.tiling.is_empty() {
            assert!(
                self.floating_is_active.get(),
                "when the tiling tree is empty but floating isn't, floating should be active"
            );
        }

        for (tile, tile_pos, visible) in self.tiles_with_render_positions() {
            if Some(tile.window().id()) != move_win_id {
                assert_eq!(tile.interactive_move_offset, Point::from((0., 0.)));
            }

            let rounded_pos = tile_pos.to_physical_precise_round(scale).to_logical(scale);

            // Tile positions must be rounded to physical pixels.
            assert_abs_diff_eq!(tile_pos.x, rounded_pos.x, epsilon = 1e-5);
            assert_abs_diff_eq!(tile_pos.y, rounded_pos.y, epsilon = 1e-5);

            if let Some(alpha) = &tile.alpha_animation {
                let anim = &alpha.anim;
                if visible {
                    assert_eq!(anim.to(), 1., "visible tiles can animate alpha only to 1");
                }

                assert!(
                    !alpha.hold_after_done,
                    "tiles in the layout cannot have held alpha animation"
                );
            }
        }
    }
}

pub(crate) fn compute_working_area(output: &Output) -> Rectangle<f64, Logical> {
    layer_map_for_output(output).non_exclusive_zone().to_f64()
}

fn has_gaps_to_edge(
    area: Rectangle<f64, Logical>,
    outer: swayward_config::OuterGaps,
    inner: f64,
) -> bool {
    apply_outer_gaps(area, outer, inner, true) != area
}

pub(super) fn apply_outer_gaps(
    mut area: Rectangle<f64, Logical>,
    outer: swayward_config::OuterGaps,
    inner: f64,
    configured: bool,
) -> Rectangle<f64, Logical> {
    if !configured {
        return area;
    }

    let mut left = (outer.left + inner).max(0.);
    let mut right = (outer.right + inner).max(0.);
    let mut top = (outer.top + inner).max(0.);
    let mut bottom = (outer.bottom + inner).max(0.);

    if area.size.w - left - right < 100. && left + right > 0. {
        let total = (area.size.w - 100.).max(0.);
        let left_fraction = left / (left + right);
        left = (left_fraction * total).trunc();
        right = total - left;
    }
    if area.size.h - top - bottom < 60. && top + bottom > 0. {
        let total = (area.size.h - 60.).max(0.);
        let top_fraction = top / (top + bottom);
        top = (top_fraction * total).trunc();
        bottom = total - top;
    }

    area.loc.x += left;
    area.loc.y += top;
    area.size.w -= left + right;
    area.size.h -= top + bottom;
    area
}

fn compute_workspace_shadow_config(
    config: swayward_config::WorkspaceShadow,
    view_size: Size<f64, Logical>,
) -> swayward_config::Shadow {
    // Gaps between workspaces are a multiple of the view height, so shadow settings should also be
    // normalized to the view height to prevent them from overlapping on lower resolutions.
    let norm = view_size.h / 1080.;

    let mut config = swayward_config::Shadow::from(config);
    config.softness *= norm;
    config.spread *= norm;
    config.offset.x.0 *= norm;
    config.offset.y.0 *= norm;

    config
}

use std::cmp::max;
use std::rc::Rc;

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::utils::{Logical, Point, Rectangle, Scale, Serial, Size};
use swayward_config::utils::MergeWith as _;
use swayward_config::{PresetSize, RelativeTo};
use swayward_ipc::{PositionChange, SizeChange, WindowLayout};

use super::closing_window::{ClosingWindow, ClosingWindowRenderElement};
use super::tile::{Tile, TileRenderElement, TileRenderSnapshot};
use super::tiling_tree::{DetachedSubtree, Direction, NodeId, TilingTree, TilingTreeRenderElement};
use super::titlebar::{self, Titlebar, TitlebarRenderer, TitlebarState};
use super::workspace::{InteractiveResize, ResolvedSize};
use super::{
    ConfigureIntent, InteractiveResizeData, LayoutElement, Options, RemovedTile, SizeFrac,
};
use crate::animation::{Animation, Clock};
use crate::layout::RenderLayer;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::RenderCtx;
use crate::swayward_render_elements;
use crate::utils::transaction::TransactionBlocker;
use crate::utils::{
    center_preferring_top_left_in_area, clamp_preferring_top_left_in_area, ensure_min_max_size,
    ensure_min_max_size_maybe_zero, ResizeEdge,
};
use crate::window::ResolvedWindowRules;

/// By how many logical pixels the directional move commands move floating windows.
pub const DIRECTIONAL_MOVE_PX: f64 = 50.;

fn remap_rect_center(
    rect: Rectangle<f64, Logical>,
    old_area: Rectangle<f64, Logical>,
    new_area: Rectangle<f64, Logical>,
) -> Rectangle<f64, Logical> {
    if old_area.size.w <= 0. || old_area.size.h <= 0. {
        return Rectangle::new(
            new_area.loc + (new_area.size.to_point() - rect.size.to_point()).downscale(2.),
            rect.size,
        );
    }
    let old_center = rect.loc + rect.size.downscale(2.);
    let relative = old_center - old_area.loc;
    let new_center = Point::from((
        new_area.loc.x + relative.x * new_area.size.w / old_area.size.w,
        new_area.loc.y + relative.y * new_area.size.h / old_area.size.h,
    ));
    Rectangle::new(new_center - rect.size.downscale(2.), rect.size)
}

/// A tile leaving the floating layer, remembering its floating size and the
/// working area its stored position is relative to.
fn removed_floating_tile<W: LayoutElement>(
    mut tile: Tile<W>,
    working_area: Rectangle<f64, Logical>,
) -> RemovedTile<W> {
    if let Some(size) = tile.window().expected_size() {
        tile.floating_window_size = Some(size);
    }
    RemovedTile {
        tile,
        is_floating: true,
        floating_working_area: Some(working_area),
    }
}

pub(super) fn apply_position_change(
    current: f64,
    change: PositionChange,
    available: f64,
    origin: f64,
) -> f64 {
    const MAX_PROPORTION: f64 = 10000.;

    match change {
        PositionChange::SetFixed(value) => value + origin,
        PositionChange::AdjustFixed(delta) => current + delta,
        PositionChange::SetProportion(proportion) if proportion.is_finite() => {
            available * (proportion / 100.).clamp(0., MAX_PROPORTION) + origin
        }
        PositionChange::AdjustProportion(delta) if delta.is_finite() => {
            let current_proportion = (current - origin) / available.max(1.);
            available * (current_proportion + delta / 100.).clamp(0., MAX_PROPORTION) + origin
        }
        PositionChange::SetProportion(_) | PositionChange::AdjustProportion(_) => current,
    }
}

/// The floating layer: single-window roots and nested container roots.
#[derive(Debug)]
pub struct FloatingLayout<W: LayoutElement> {
    /// Single-window root entries in top-to-bottom order.
    entries: Vec<FloatingEntry<W>>,

    /// Nested container roots, created by `floating enable` on a container.
    tree_entries: Vec<FloatingTreeEntry<W>>,

    /// Id of the active window.
    ///
    /// The active window is not necessarily the topmost window. Focus-follows-mouse should
    /// activate a window, but not bring it to the top, because that's very annoying.
    ///
    /// Removing the active window hands activation to another floating window, so this is
    /// `Some()` while `entries` or `tree_entries` holds a window.
    active_window_id: Option<W::Id>,

    /// Ongoing interactive resize.
    interactive_resize: Option<InteractiveResize<W>>,

    /// Windows in the closing animation.
    closing_windows: Vec<ClosingWindow>,

    /// View size for this space.
    view_size: Size<f64, Logical>,

    /// Working area for this space.
    working_area: Rectangle<f64, Logical>,

    /// Scale of the output the space is on (and rounds its sizes to).
    scale: f64,

    /// Clock for driving animations.
    clock: Clock,

    /// Configurable properties of the layout.
    options: Rc<Options>,

    titlebars: TitlebarRenderer,
}

swayward_render_elements! {
    FloatingLayoutRenderElement<R> => {
        Tile = TileRenderElement<R>,
        ClosingWindow = ClosingWindowRenderElement,
        Titlebar = crate::render_helpers::primary_gpu_texture::PrimaryGpuTextureRenderElement,
        Tree = TilingTreeRenderElement<R>,
    }
}

/// A single-window floating root.
#[derive(Debug)]
struct FloatingEntry<W: LayoutElement> {
    tile: Tile<W>,
    data: Data,
}

/// A nested container root resident in the floating layer.
#[derive(Debug)]
struct FloatingTreeEntry<W: LayoutElement> {
    tree: TilingTree<W>,
    root: NodeId,
    rect: Rectangle<f64, Logical>,
    pos: Point<f64, SizeFrac>,
    sticky: bool,
}

/// A nested floating root detached for scratchpad or workspace transfer.
#[derive(Debug)]
pub struct RemovedFloatingTree<W: LayoutElement> {
    pub(super) tree: TilingTree<W>,
    root: NodeId,
    window_ids: Vec<W::Id>,
    rect: Rectangle<f64, Logical>,
    working_area: Rectangle<f64, Logical>,
    sticky: bool,
}

impl<W: LayoutElement> RemovedFloatingTree<W> {
    pub fn ipc_tree(&self) -> super::tiling_tree::IpcNode<W::Id> {
        self.tree.ipc_tree()
    }

    pub fn is_sticky(&self) -> bool {
        self.sticky
    }

    pub fn contains_window(&self, window: &W::Id) -> bool {
        self.window_ids.contains(window)
    }

    pub fn window_ids(&self) -> &[W::Id] {
        &self.window_ids
    }

    pub fn windows(&self) -> impl Iterator<Item = &W> {
        self.tree.windows().map(|(_, window)| window)
    }

    pub(super) fn remove_window(
        &mut self,
        window: &W::Id,
        transaction: crate::utils::transaction::Transaction,
    ) -> Option<RemovedTile<W>> {
        let tile = self.tree.remove_tile(window, transaction)?;
        self.window_ids.retain(|id| id != window);
        Some(removed_floating_tile(tile, self.working_area))
    }

    pub fn into_subtree(mut self) -> Option<DetachedSubtree<W>> {
        let root = self.tree.resident_root()?;
        self.tree.set_split_sticky(root, self.sticky);
        self.tree.detach_resident_root(root)
    }
}

/// Root geometry for a floating entry.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Data {
    /// Position relative to the working area.
    pos: Point<f64, SizeFrac>,

    /// Cached position in logical coordinates.
    ///
    /// Not rounded to physical pixels.
    logical_pos: Point<f64, Logical>,

    /// Cached actual size of the tile.
    size: Size<f64, Logical>,

    /// Output size used for off-screen allowances.
    view_size: Size<f64, Logical>,

    /// Working area used for conversions.
    working_area: Rectangle<f64, Logical>,
}

impl Data {
    pub fn new<W: LayoutElement>(
        view_size: Size<f64, Logical>,
        working_area: Rectangle<f64, Logical>,
        tile: &Tile<W>,
        logical_pos: Point<f64, Logical>,
    ) -> Self {
        let mut rv = Self {
            pos: Point::default(),
            logical_pos: Point::default(),
            size: Size::default(),
            view_size,
            working_area,
        };
        rv.update(tile);
        rv.set_logical_pos(logical_pos);
        rv
    }

    pub fn scale_by_working_area(
        area: Rectangle<f64, Logical>,
        pos: Point<f64, SizeFrac>,
    ) -> Point<f64, Logical> {
        let mut logical_pos = Point::from((pos.x, pos.y));
        logical_pos.x *= area.size.w;
        logical_pos.y *= area.size.h;
        logical_pos += area.loc;
        logical_pos
    }

    pub fn logical_to_size_frac_in_working_area(
        area: Rectangle<f64, Logical>,
        logical_pos: Point<f64, Logical>,
    ) -> Point<f64, SizeFrac> {
        let pos = logical_pos - area.loc;
        let mut pos = Point::from((pos.x, pos.y));
        pos.x /= f64::max(area.size.w, 1.0);
        pos.y /= f64::max(area.size.h, 1.0);
        pos
    }

    fn recompute_logical_pos(&mut self) {
        // Sway never clamps a floating window's position. container_floating_move_to
        // translates to the requested coordinates with no bounds check
        // (`sway/sway/tree/container.c:1127-1159`), and the drag seatop feeds it raw
        // cursor coordinates (`sway/sway/input/seatop_move_floating.c:40`), so a window
        // dragged off the screen edge stays there. The only bounds-aware path centers
        // rather than clamps (`container_floating_resize_and_center`, :864-908).
        //
        // niri clamped here instead, keeping a Mutter-derived slice of every window
        // on screen. That is the opposite rule, and it silently moved windows a sway
        // client had positioned deliberately.
        self.logical_pos = Self::scale_by_working_area(self.working_area, self.pos);
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        working_area: Rectangle<f64, Logical>,
    ) {
        if self.view_size == view_size && self.working_area == working_area {
            return;
        }

        self.view_size = view_size;
        self.working_area = working_area;
        self.recompute_logical_pos();
    }

    pub fn update<W: LayoutElement>(&mut self, tile: &Tile<W>) {
        let size = tile.tile_size();
        if self.size == size {
            return;
        }

        self.size = size;
        self.recompute_logical_pos();
    }

    pub fn set_logical_pos(&mut self, logical_pos: Point<f64, Logical>) {
        self.pos = Self::logical_to_size_frac_in_working_area(self.working_area, logical_pos);

        self.recompute_logical_pos();
    }

    pub fn center(&self) -> Point<f64, Logical> {
        self.logical_pos + self.size.downscale(2.)
    }

    #[cfg(test)]
    fn verify_invariants(&self) {
        assert!(self.logical_pos.x.is_finite());
        assert!(self.logical_pos.y.is_finite());
        assert!(self.size.w.is_finite());
        assert!(self.size.h.is_finite());
        assert!(self.size.w >= 0.);
        assert!(self.size.h >= 0.);

        let mut temp = *self;
        temp.recompute_logical_pos();
        assert_eq!(
            self.logical_pos, temp.logical_pos,
            "cached logical pos must be up to date"
        );
    }
}

/// Sway's floating size constraints, from `floating_minimum_size` and
/// `floating_maximum_size`: -1 means none, 0 automatic (75x50 minimum, the
/// output layout box as maximum) and N a fixed bound
/// (floating_calculate_constraints, sway/tree/container.c:779-816). An absent
/// maximum is infinite.
pub(super) fn floating_constraints(
    minimum: swayward_config::FloatingSize,
    maximum: swayward_config::FloatingSize,
    automatic_maximum: Size<f64, Logical>,
) -> (Size<f64, Logical>, Size<f64, Logical>) {
    let min = |value: i32, automatic: f64| match value {
        -1 => 0.,
        0 => automatic,
        value => f64::from(value),
    };
    let max = |value: i32, automatic: f64| match value {
        -1 => f64::INFINITY,
        0 => automatic,
        value => f64::from(value),
    };
    (
        Size::from((min(minimum.width, 75.), min(minimum.height, 50.))),
        Size::from((
            max(maximum.width, automatic_maximum.w),
            max(maximum.height, automatic_maximum.h),
        )),
    )
}

fn constrain_floating_size(
    mut size: Size<i32, Logical>,
    minimum: swayward_config::FloatingSize,
    maximum: swayward_config::FloatingSize,
    automatic_maximum: Size<f64, Logical>,
    client_minimum: Size<i32, Logical>,
    client_maximum: Size<i32, Logical>,
) -> Size<i32, Logical> {
    let (minimum, maximum) = floating_constraints(minimum, maximum, automatic_maximum);
    // ensure_min_max_size reads a bound of 0 as none.
    let bound = |value: f64| {
        if value.is_finite() {
            value.round() as i32
        } else {
            0
        }
    };
    let minimum = Size::<i32, Logical>::from((bound(minimum.w), bound(minimum.h)));
    let maximum = Size::<i32, Logical>::from((bound(maximum.w), bound(maximum.h)));
    size.w = ensure_min_max_size(size.w, minimum.w, maximum.w);
    size.h = ensure_min_max_size(size.h, minimum.h, maximum.h);
    size.w = ensure_min_max_size(size.w, client_minimum.w, client_maximum.w);
    size.h = ensure_min_max_size(size.h, client_minimum.h, client_maximum.h);
    size
}

impl<W: LayoutElement> FloatingLayout<W> {
    pub fn new(
        view_size: Size<f64, Logical>,
        working_area: Rectangle<f64, Logical>,
        scale: f64,
        clock: Clock,
        options: Rc<Options>,
    ) -> Self {
        Self {
            entries: Vec::new(),
            tree_entries: Vec::new(),
            active_window_id: None,
            interactive_resize: None,
            closing_windows: Vec::new(),
            view_size,
            working_area,
            scale,
            clock,
            options,
            titlebars: Default::default(),
        }
    }

    pub fn update_config(
        &mut self,
        view_size: Size<f64, Logical>,
        working_area: Rectangle<f64, Logical>,
        scale: f64,
        options: Rc<Options>,
    ) {
        for (tile, data) in self
            .entries
            .iter_mut()
            .map(|entry| (&mut entry.tile, &mut entry.data))
        {
            tile.update_config(view_size, scale, options.clone());
            data.update(tile);
            data.update_config(view_size, working_area);
        }
        for entry in &mut self.tree_entries {
            entry.rect.loc = Data::scale_by_working_area(working_area, entry.pos);
            entry
                .tree
                .update_config(view_size, entry.rect, false, scale, options.clone());
        }

        self.view_size = view_size;
        self.working_area = working_area;
        self.scale = scale;
        self.options = options;
    }

    pub fn update_shaders(&mut self) {
        for entry in &mut self.entries {
            entry.tile.update_shaders();
        }
        for entry in &mut self.tree_entries {
            entry.tree.update_shaders();
        }
    }

    pub fn advance_animations(&mut self) {
        for entry in &mut self.entries {
            entry.tile.advance_animations();
        }
        for entry in &mut self.tree_entries {
            entry.tree.advance_animations();
        }

        self.closing_windows.retain_mut(|closing| {
            closing.advance_animations();
            closing.are_animations_ongoing()
        });
    }

    pub fn are_animations_ongoing(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.tile.are_animations_ongoing())
            || self
                .tree_entries
                .iter()
                .any(|entry| entry.tree.are_animations_ongoing())
            || !self.closing_windows.is_empty()
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.tile.are_transitions_ongoing())
            || self
                .tree_entries
                .iter()
                .any(|entry| entry.tree.are_transitions_ongoing())
            || !self.closing_windows.is_empty()
    }

    pub fn update_render_elements(
        &mut self,
        is_active: bool,
        view_rect: Rectangle<f64, Logical>,
        layer: RenderLayer,
    ) {
        for entry in &mut self.tree_entries {
            entry.tree.update_render_elements(is_active, layer);
        }
        let active = self.active_window_id.clone();
        for (tile, offset) in self.tiles_with_offsets_mut() {
            // Skip tiles belonging to a different render layer.
            if layer.is_normal() == tile.is_moving_between_workspaces() {
                continue;
            }

            tile.set_border_edges(ResizeEdge::all());
            tile.set_border_visible(true);
            tile.set_decorated_box(
                super::tile::DecoratedCorners::ALL,
                tile.has_sway_titlebar(),
                false,
            );
            let id = tile.window().id();
            let is_active = is_active && Some(id) == active.as_ref();

            let mut tile_view_rect = view_rect;
            tile_view_rect.loc -= offset + tile.render_offset();
            tile.update_render_elements(is_active, tile_view_rect);
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = &Tile<W>> + '_ {
        self.entries.iter().map(|entry| &entry.tile).chain(
            self.tree_entries
                .iter()
                .flat_map(|entry| entry.tree.tiles()),
        )
    }

    pub fn tiles_mut(&mut self) -> impl Iterator<Item = &mut Tile<W>> + '_ {
        self.entries.iter_mut().map(|entry| &mut entry.tile).chain(
            self.tree_entries
                .iter_mut()
                .flat_map(|entry| entry.tree.tiles_mut()),
        )
    }

    pub fn tiles_with_offsets(&self) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>)> + '_ {
        self.entries
            .iter()
            .map(|entry| (&entry.tile, entry.data.logical_pos))
    }

    pub fn tiles_with_offsets_mut(
        &mut self,
    ) -> impl Iterator<Item = (&mut Tile<W>, Point<f64, Logical>)> + '_ {
        self.entries
            .iter_mut()
            .map(|entry| (&mut entry.tile, entry.data.logical_pos))
    }

    pub fn tiles_with_render_positions(
        &self,
    ) -> impl Iterator<Item = (&Tile<W>, Point<f64, Logical>)> {
        let scale = self.scale;
        self.tiles_with_offsets().map(move |(tile, offset)| {
            let pos = offset + tile.render_offset();
            // Round to physical pixels.
            let pos = pos.to_physical_precise_round(scale).to_logical(scale);
            (tile, pos)
        })
    }

    pub fn tiles_with_render_positions_mut(
        &mut self,
        round: bool,
    ) -> impl Iterator<Item = (&mut Tile<W>, Point<f64, Logical>)> {
        let scale = self.scale;
        self.tiles_with_offsets_mut().map(move |(tile, offset)| {
            let mut pos = offset + tile.render_offset();
            // Round to physical pixels.
            if round {
                pos = pos.to_physical_precise_round(scale).to_logical(scale);
            }
            (tile, pos)
        })
    }

    pub fn ipc_decoration_rect(
        &self,
        tile: &Tile<W>,
        layout: &WindowLayout,
    ) -> Option<Rectangle<f64, Logical>> {
        let pos = layout.tile_pos_in_workspace_view?.into();
        self.titlebar_rect(tile, pos, layout.tile_size.0)
    }

    fn titlebar_rect(
        &self,
        tile: &Tile<W>,
        tile_pos: Point<f64, Logical>,
        width: f64,
    ) -> Option<Rectangle<f64, Logical>> {
        if !tile.has_sway_titlebar() {
            return None;
        }
        let height = titlebar::height(self.scale, &self.options.layout.titlebar);
        Some(Rectangle::new(
            Point::from((tile_pos.x, tile_pos.y)),
            Size::from((width, height)),
        ))
    }

    pub fn tiles_with_ipc_layouts(&self) -> impl Iterator<Item = (&Tile<W>, WindowLayout)> {
        let scale = self.scale;
        self.tiles_with_offsets().map(move |(tile, offset)| {
            // Do not include animated render offset here to avoid IPC spam.
            let pos = offset;
            // Round to physical pixels.
            let pos = pos.to_physical_precise_round(scale).to_logical(scale);

            let layout = WindowLayout {
                tile_size: tile.tile_expected_or_current_size().into(),
                window_size: tile.window().ipc_size().into(),
                tile_pos_in_workspace_view: Some(pos.into()),
                ..tile.ipc_layout_template()
            };
            (tile, layout)
        })
    }

    pub fn new_window_toplevel_bounds(&self, rules: &ResolvedWindowRules) -> Size<i32, Logical> {
        let border_config = self.options.layout.border.merged_with(&rules.border);
        compute_toplevel_bounds(border_config, self.working_area.size)
    }

    /// Returns the geometry of the active window relative to and clamped to the working area.
    ///
    /// During animations, assumes the final tile position.
    pub fn active_window_visual_rectangle(&self) -> Option<Rectangle<f64, Logical>> {
        let active_id = self.active_window_id.as_ref()?;
        if let Some(rect) = self
            .active_tree_entry()
            .and_then(|entry| entry.tree.active_window_visual_rectangle())
        {
            return Some(rect);
        }
        let (tile, offset) = self
            .tiles_with_offsets()
            .find(|(tile, _)| tile.window().id() == active_id)?;

        let window_pos = offset + tile.window_loc();
        let window_size = tile.window_size();
        let window_rect = Rectangle::new(window_pos, window_size);

        self.working_area.intersection(window_rect)
    }

    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<(&W, super::HitType)> {
        for entry in self.tree_entries.iter().rev() {
            if let Some(hit) = entry.tree.window_under(pos) {
                return Some(hit);
            }
        }
        for (tile, tile_pos) in self.tiles_with_render_positions() {
            if self
                .titlebar_rect(tile, tile_pos, tile.animated_tile_size().w)
                .is_some_and(|rect| rect.contains(pos))
            {
                return Some((
                    tile.window(),
                    super::HitType::Activate {
                        is_tab_indicator: true,
                    },
                ));
            }
            if let Some(hit) = super::HitType::hit_tile(tile, tile_pos, pos) {
                return Some(hit);
            }
        }
        None
    }

    pub fn popup_target_rect(&self, id: &W::Id) -> Option<Rectangle<f64, Logical>> {
        if let Some(rect) = self
            .tree_entries
            .iter()
            .find_map(|entry| entry.tree.popup_target_rect(id))
        {
            return Some(rect);
        }
        for (tile, pos) in self.tiles_with_offsets() {
            if tile.window().id() == id {
                // Position within the working area.
                let mut target = self.working_area;
                target.loc -= pos;
                target.loc -= tile.window_loc();

                return Some(target);
            }
        }
        None
    }

    fn idx_of(&self, id: &W::Id) -> Option<usize> {
        self.entries
            .iter()
            .map(|entry| &entry.tile)
            .position(|tile| tile.window().id() == id)
    }

    fn contains(&self, id: &W::Id) -> bool {
        self.has_window(id)
    }

    /// The index of the nested tree holding `window`, and the window's leaf node there.
    fn tree_entry_for_window(&self, window: &W::Id) -> Option<(usize, NodeId)> {
        self.tree_entries
            .iter()
            .enumerate()
            .find_map(|(idx, entry)| entry.tree.node_for_window(window).map(|node| (idx, node)))
    }

    fn tree_entry_with_window(&self, window: &W::Id) -> Option<&FloatingTreeEntry<W>> {
        let (idx, _) = self.tree_entry_for_window(window)?;
        self.tree_entries.get(idx)
    }

    fn tree_entry_with_window_mut(&mut self, window: &W::Id) -> Option<&mut FloatingTreeEntry<W>> {
        let (idx, _) = self.tree_entry_for_window(window)?;
        self.tree_entries.get_mut(idx)
    }

    /// The nested tree holding the active window.
    fn active_tree_entry(&self) -> Option<&FloatingTreeEntry<W>> {
        self.tree_entry_with_window(self.active_window_id.as_ref()?)
    }

    fn active_tree_entry_mut(&mut self) -> Option<&mut FloatingTreeEntry<W>> {
        let (idx, _) = self.tree_entry_for_window(self.active_window_id.as_ref()?)?;
        self.tree_entries.get_mut(idx)
    }

    /// Pick the next floating focus when the active entry disappears.
    ///
    /// Sway asks the seat focus stack for the inactive container
    /// (`seat_get_focus_inactive`, sway/sway/input/seat.c). Swayward stores
    /// recursive floating roots and standalone windows separately; use one
    /// fallback order everywhere so removal API choice cannot change focus.
    fn fallback_active_window(&self) -> Option<W::Id> {
        self.entries
            .first()
            .map(|entry| entry.tile.window().id().clone())
            .or_else(|| {
                self.tree_entries
                    .iter()
                    .find_map(|entry| entry.tree.active_window().map(|window| window.id().clone()))
            })
    }

    pub fn active_window(&self) -> Option<&W> {
        let id = self.active_window_id.as_ref()?;
        self.entries
            .iter()
            .map(|entry| &entry.tile)
            .find(|tile| tile.window().id() == id)
            .map(Tile::window)
            .or_else(|| {
                self.tree_entries
                    .iter()
                    .find_map(|entry| entry.tree.windows().find(|(_, window)| window.id() == id))
                    .map(|(_, window)| window)
            })
    }

    pub fn active_window_mut(&mut self) -> Option<&mut W> {
        let id = self.active_window_id.as_ref()?;
        if let Some(tile) = self
            .entries
            .iter_mut()
            .map(|entry| &mut entry.tile)
            .find(|tile| tile.window().id() == id)
        {
            return Some(tile.window_mut());
        }
        self.tree_entries.iter_mut().find_map(|entry| {
            entry
                .tree
                .tiles_mut()
                .find(|tile| tile.window().id() == id)
                .map(Tile::window_mut)
        })
    }

    pub fn has_window(&self, id: &W::Id) -> bool {
        self.entries
            .iter()
            .map(|entry| &entry.tile)
            .any(|tile| tile.window().id() == id)
            || self.tree_entry_for_window(id).is_some()
    }

    pub fn window_is_floating_root(&self, id: &W::Id) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.tile.window().id() == id)
    }

    pub fn window_is_tree_root(&self, window: &W::Id) -> bool {
        self.tree_entry_for_window(window)
            .is_some_and(|(idx, node)| self.tree_entries[idx].root == node)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.tree_entries.is_empty()
    }

    pub fn add_tile(&mut self, tile: Tile<W>, activate: bool) {
        self.add_tile_at(0, tile, activate);
    }

    /// Apply a split command to the active floating container. Sway wraps a
    /// standalone floating view in a new split container (`container_split`,
    /// sway/tree/container.c:1565-1620), so a lone leaf is promoted into a
    /// resident floating tree with its geometry kept.
    pub fn split_active(&mut self, layout: super::tiling_tree::Layout) {
        self.interactive_resize = None;
        let Some(active) = self.active_window_id.clone() else {
            return;
        };
        if let Some((idx, _)) = self.tree_entry_for_window(&active) {
            if let Some(entry) = self.tree_entries.get_mut(idx) {
                entry.tree.split_focused(layout);
            }
            return;
        }
        let Some(index) = self.idx_of(&active) else {
            return;
        };
        let FloatingEntry { tile, data } = self.entries.remove(index);
        let rect = Rectangle::new(data.logical_pos, data.size);
        let mut tree = TilingTree::new(
            self.view_size,
            rect,
            false,
            self.scale,
            self.clock.clone(),
            self.options.clone(),
        );
        tree.add_tile(tile, super::tiling_tree::InsertTarget::Focused);
        tree.split_focused(layout);
        let Some(root) = tree.parent_of_window(&active) else {
            return;
        };
        let Some((subtree, _)) = tree.detach_subtree(root) else {
            return;
        };
        self.add_tree(subtree, rect);
    }

    pub fn add_tree(
        &mut self,
        subtree: DetachedSubtree<W>,
        rect: Rectangle<f64, Logical>,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        let (tree, root, remapped) = TilingTree::from_detached_subtree(
            self.view_size,
            rect,
            self.scale,
            self.clock.clone(),
            self.options.clone(),
            subtree,
        );
        debug_assert!(!tree.is_empty());
        debug_assert!(!self
            .tree_entries
            .iter()
            .any(|entry| entry.tree.contains(root)));
        self.active_window_id = tree.active_window().map(|window| window.id().clone());
        let sticky = tree.is_split_sticky(root);
        self.tree_entries.insert(
            0,
            FloatingTreeEntry {
                tree,
                root,
                rect,
                pos: Data::logical_to_size_frac_in_working_area(self.working_area, rect.loc),
                sticky,
            },
        );
        (root, remapped)
    }

    pub fn add_removed_tree(
        &mut self,
        removed: RemovedFloatingTree<W>,
        remap_position: bool,
    ) -> (NodeId, Vec<(NodeId, NodeId)>) {
        let rect = if remap_position {
            remap_rect_center(removed.rect, removed.working_area, self.working_area)
        } else {
            removed.rect
        };
        let mut tree = removed.tree;
        tree.update_config(
            self.view_size,
            rect,
            false,
            self.scale,
            self.options.clone(),
        );
        let root = removed.root;
        self.active_window_id = tree.active_window().map(|window| window.id().clone());
        self.tree_entries.insert(
            0,
            FloatingTreeEntry {
                tree,
                root,
                rect,
                pos: Data::logical_to_size_frac_in_working_area(self.working_area, rect.loc),
                sticky: removed.sticky,
            },
        );
        (root, Vec::new())
    }

    pub fn remove_tree(&mut self, root: NodeId) -> Option<DetachedSubtree<W>> {
        self.remove_tree_for_transfer(root)?.into_subtree()
    }

    pub fn remove_tree_for_transfer(&mut self, root: NodeId) -> Option<RemovedFloatingTree<W>> {
        let index = self
            .tree_entries
            .iter()
            .position(|entry| entry.root == root)?;
        let entry = self.tree_entries.remove(index);
        let window_ids: Vec<_> = entry
            .tree
            .windows()
            .map(|(_, window)| window.id().clone())
            .collect();
        if self
            .active_window_id
            .as_ref()
            .is_some_and(|active| window_ids.contains(active))
        {
            self.active_window_id = self.fallback_active_window();
        }
        Some(RemovedFloatingTree {
            tree: entry.tree,
            root,
            window_ids,
            rect: entry.rect,
            working_area: self.working_area,
            sticky: entry.sticky,
        })
    }

    pub fn tree_rect(&self, root: NodeId) -> Option<Rectangle<f64, Logical>> {
        self.tree_entries
            .iter()
            .find(|entry| entry.root == root)
            .map(|entry| entry.rect)
    }

    pub fn tree_root_for_window(&self, window: &W::Id) -> Option<NodeId> {
        self.tree_entry_with_window(window).map(|entry| entry.root)
    }

    pub fn tree_roots(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.tree_entries.iter().map(|entry| entry.root)
    }

    pub fn transfer_window_ids(&self) -> Vec<W::Id> {
        self.entries
            .iter()
            .map(|entry| entry.tile.window().id().clone())
            .chain(self.tree_entries.iter().filter_map(|entry| {
                entry
                    .tree
                    .tiles()
                    .next()
                    .map(|tile| tile.window().id().clone())
            }))
            .collect()
    }

    pub fn ipc_trees(
        &self,
    ) -> impl Iterator<Item = (NodeId, super::tiling_tree::IpcNode<W::Id>, bool)> + '_ {
        self.tree_entries
            .iter()
            .map(|entry| (entry.root, entry.tree.ipc_tree(), entry.sticky))
    }

    pub fn tree_root_for_node(&self, node: NodeId) -> Option<NodeId> {
        self.tree_entries
            .iter()
            .find(|entry| entry.tree.contains(node))
            .map(|entry| entry.root)
    }

    pub fn focused_leaf_is_only_child_of_tree_root(&self) -> bool {
        self.active_tree_entry()
            .is_some_and(|entry| entry.tree.focused_leaf_is_only_child_of_resident_root())
    }

    pub fn fullscreen_mode(&self) -> Option<super::tiling_tree::FullscreenMode> {
        self.tree_entries.iter().find_map(|entry| {
            let fullscreen = entry.tree.fullscreen_node()?;
            entry.tree.fullscreen_mode(fullscreen)
        })
    }

    pub fn fullscreen_mode_for_window(
        &self,
        window: &W::Id,
    ) -> Option<super::tiling_tree::FullscreenMode> {
        let (idx, node) = self.tree_entry_for_window(window)?;
        let tree = &self.tree_entries[idx].tree;
        let fullscreen = tree.fullscreen_node()?;
        tree.contains_node(fullscreen, node)
            .then(|| tree.fullscreen_mode(fullscreen))
            .flatten()
    }

    pub fn fullscreen_contains_window(&self, window: &W::Id) -> bool {
        self.tree_entry_for_window(window)
            .is_some_and(|(idx, node)| {
                let tree = &self.tree_entries[idx].tree;
                tree.fullscreen_node()
                    .is_some_and(|fullscreen| tree.contains_node(fullscreen, node))
            })
    }

    pub fn fullscreen_window(&self) -> Option<&W::Id> {
        self.tree_entries
            .iter()
            .find_map(|entry| entry.tree.fullscreen_window())
    }

    pub fn disable_fullscreen(&mut self) {
        for entry in &mut self.tree_entries {
            if let Some(fullscreen) = entry.tree.fullscreen_node() {
                entry.tree.set_node_fullscreen(fullscreen, None);
                return;
            }
        }
    }

    pub fn set_window_fullscreen(
        &mut self,
        window: &W::Id,
        mode: Option<super::tiling_tree::FullscreenMode>,
    ) -> bool {
        let Some((idx, node)) = self.tree_entry_for_window(window) else {
            return false;
        };
        self.tree_entries[idx].tree.set_node_fullscreen(node, mode)
    }

    pub fn set_focused_fullscreen(
        &mut self,
        mode: Option<super::tiling_tree::FullscreenMode>,
    ) -> bool {
        let Some(entry) = self.active_tree_entry_mut() else {
            return false;
        };
        let Some(focus) = entry.tree.focus() else {
            return false;
        };
        entry.tree.set_node_fullscreen(focus, mode)
    }

    pub fn focused_container_node(&self) -> Option<NodeId> {
        let entry = self.active_tree_entry()?;
        entry.tree.focus().filter(|node| entry.tree.is_split(*node))
    }

    pub fn set_tree_window_border(
        &mut self,
        id: &W::Id,
        style: swayward_ipc::command::BorderStyle,
        width: Option<u16>,
    ) -> bool {
        self.tree_entry_with_window_mut(id)
            .is_some_and(|entry| entry.tree.set_window_border(id, style, width))
    }

    pub fn focused_tree_child(&self) -> bool {
        self.active_tree_entry()
            .and_then(|entry| entry.tree.focus().map(|focus| focus != entry.root))
            .unwrap_or(false)
    }

    pub fn focused_child_tree_mut(&mut self) -> Option<&mut TilingTree<W>> {
        self.active_tree_entry_mut()
            .filter(|entry| entry.tree.focus().is_some_and(|focus| focus != entry.root))
            .map(|entry| &mut entry.tree)
    }

    pub fn move_focused_tree_child(&mut self, direction: Direction) -> Option<bool> {
        let entry = self.active_tree_entry_mut()?;
        let focus = entry.tree.focus().filter(|focus| *focus != entry.root)?;
        Some(entry.tree.move_node_direction(focus, direction))
    }

    pub fn move_tree_window(&mut self, window: &W::Id, direction: Direction) -> Option<bool> {
        let (idx, node) = self.tree_entry_for_window(window)?;
        Some(
            self.tree_entries[idx]
                .tree
                .move_node_direction(node, direction),
        )
    }

    pub fn window_in_node(&self, node: NodeId) -> Option<&W::Id> {
        self.tree_entries.iter().find_map(|entry| {
            entry
                .tree
                .windows()
                .find(|(leaf, _)| entry.tree.contains_node(node, *leaf))
                .map(|(_, window)| window.id())
        })
    }

    pub fn focus_parent(&mut self) -> bool {
        self.active_tree_entry_mut()
            .is_some_and(|entry| entry.tree.focus_parent())
    }

    pub fn focus_child(&mut self) -> bool {
        self.active_tree_entry_mut()
            .is_some_and(|entry| entry.tree.focus_child())
    }

    pub fn tree_window_ids(&self, root: NodeId) -> Option<Vec<W::Id>> {
        self.tree(root).map(|tree| {
            tree.windows()
                .map(|(_, window)| window.id().clone())
                .collect()
        })
    }

    pub fn tree_is_sticky(&self, root: NodeId) -> bool {
        self.tree_entries
            .iter()
            .find(|entry| entry.root == root)
            .is_some_and(|entry| entry.sticky)
    }

    pub fn window_is_sticky(&self, window: &W::Id) -> bool {
        let Some((idx, node)) = self.tree_entry_for_window(window) else {
            return false;
        };
        let entry = &self.tree_entries[idx];
        if node == entry.root {
            entry.sticky
        } else {
            entry
                .tree
                .tiles()
                .find(|tile| tile.window().id() == window)
                .is_some_and(|tile| tile.is_sticky)
        }
    }

    pub fn set_window_sticky(&mut self, window: &W::Id, sticky: bool) -> bool {
        let Some((idx, node)) = self.tree_entry_for_window(window) else {
            return false;
        };
        let entry = &mut self.tree_entries[idx];
        if node == entry.root {
            entry.sticky = sticky;
            return true;
        }
        let Some(tile) = entry
            .tree
            .tiles_mut()
            .find(|tile| tile.window().id() == window)
        else {
            return false;
        };
        tile.is_sticky = sticky;
        true
    }

    pub fn swap_nodes(&mut self, first: NodeId, second: NodeId) -> Result<(), &'static str> {
        let Some(entry) = self
            .tree_entries
            .iter_mut()
            .find(|entry| entry.tree.contains(first) && entry.tree.contains(second))
        else {
            return Err("node not found");
        };
        entry.tree.swap_nodes(first, second)
    }

    pub fn set_tree_sticky(&mut self, root: NodeId, sticky: bool) -> bool {
        let Some(entry) = self
            .tree_entries
            .iter_mut()
            .find(|entry| entry.root == root)
        else {
            return false;
        };
        entry.sticky = sticky;
        entry.tree.set_split_sticky(root, sticky);
        true
    }

    pub fn tree(&self, root: NodeId) -> Option<&TilingTree<W>> {
        self.tree_entries
            .iter()
            .find(|entry| entry.root == root)
            .map(|entry| &entry.tree)
    }

    pub fn tree_mut(&mut self, root: NodeId) -> Option<&mut TilingTree<W>> {
        self.tree_entries
            .iter_mut()
            .find(|entry| entry.root == root)
            .map(|entry| &mut entry.tree)
    }

    pub fn take_sticky_trees(&mut self) -> Vec<RemovedFloatingTree<W>> {
        let roots = self
            .tree_entries
            .iter()
            .filter(|entry| entry.sticky)
            .map(|entry| entry.root)
            .collect::<Vec<_>>();
        roots
            .into_iter()
            .filter_map(|root| self.remove_tree_for_transfer(root))
            .collect()
    }

    /// Grows or shrinks a floating group root by `amount` px along `edge`, as
    /// sway's resize_adjust_floating does (sway/commands/resize.c:180-230):
    /// the size is clamped to the floating constraints, a width or height
    /// change keeps the centre, and LEFT or TOP keeps the opposite edge.
    /// Returns false when nothing changes ("Cannot resize any further").
    pub fn adjust_tree_size(
        &mut self,
        root: NodeId,
        edge: Option<ResizeEdge>,
        horizontal: bool,
        amount: i32,
        automatic_maximum: Size<f64, Logical>,
    ) -> bool {
        let Some(mut rect) = self.tree_rect(root) else {
            return false;
        };
        let (min, max) = floating_constraints(
            self.options.layout.floating_minimum_size,
            self.options.layout.floating_maximum_size,
            automatic_maximum,
        );
        let clamp_grow = |current: f64, min: f64, max: f64| {
            let grown = current + f64::from(amount);
            if grown < min {
                min - current
            } else if grown > max {
                max - current
            } else {
                f64::from(amount)
            }
        };
        let (grow_w, grow_h) = if horizontal {
            (clamp_grow(rect.size.w, min.w, max.w), 0.)
        } else {
            (0., clamp_grow(rect.size.h, min.h, max.h))
        };
        if grow_w == 0. && grow_h == 0. {
            return false;
        }
        match edge {
            None if horizontal => rect.loc.x -= (grow_w / 2.).trunc(),
            None => rect.loc.y -= (grow_h / 2.).trunc(),
            Some(edge) if edge.contains(ResizeEdge::LEFT) => rect.loc.x -= grow_w,
            Some(edge) if edge.contains(ResizeEdge::TOP) => rect.loc.y -= grow_h,
            Some(_) => {}
        }
        rect.size.w += grow_w;
        rect.size.h += grow_h;
        self.move_tree(root, rect)
    }

    /// Sets a floating group root's outer size, keeping its centre, as sway's
    /// resize_set_floating does (sway/commands/resize.c:341-401). `None`
    /// leaves that dimension unchanged.
    pub fn set_tree_size(
        &mut self,
        root: NodeId,
        width: Option<f64>,
        height: Option<f64>,
        automatic_maximum: Size<f64, Logical>,
    ) -> bool {
        let Some(mut rect) = self.tree_rect(root) else {
            return false;
        };
        let (min, max) = floating_constraints(
            self.options.layout.floating_minimum_size,
            self.options.layout.floating_maximum_size,
            automatic_maximum,
        );
        if let Some(width) = width {
            let width = width.min(max.w).max(min.w);
            rect.loc.x -= ((width - rect.size.w) / 2.).trunc();
            rect.size.w = width;
        }
        if let Some(height) = height {
            let height = height.min(max.h).max(min.h);
            rect.loc.y -= ((height - rect.size.h) / 2.).trunc();
            rect.size.h = height;
        }
        self.move_tree(root, rect)
    }

    pub fn move_tree(&mut self, root: NodeId, rect: Rectangle<f64, Logical>) -> bool {
        let Some(entry) = self
            .tree_entries
            .iter_mut()
            .find(|entry| entry.root == root)
        else {
            return false;
        };
        entry.rect = rect;
        entry.pos = Data::logical_to_size_frac_in_working_area(self.working_area, rect.loc);
        entry.tree.update_config(
            self.view_size,
            rect,
            false,
            self.scale,
            self.options.clone(),
        );
        true
    }
}

mod commands;
mod interaction;
mod rendering;

fn compute_toplevel_bounds(
    border_config: swayward_config::Border,
    working_area_size: Size<f64, Logical>,
) -> Size<i32, Logical> {
    let mut border = 0.;
    if !border_config.off {
        border = border_config.width * 2.;
    }

    Size::from((
        f64::max(working_area_size.w - border, 1.),
        f64::max(working_area_size.h - border, 1.),
    ))
    .to_i32_floor()
}

fn resolve_preset_size(preset: PresetSize, view_size: f64) -> ResolvedSize {
    match preset {
        PresetSize::Proportion(proportion) => ResolvedSize::Tile(view_size * proportion),
        PresetSize::Fixed(width) => ResolvedSize::Window(f64::from(width)),
    }
}

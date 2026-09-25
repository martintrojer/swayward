use std::cmp::min;
use std::iter::zip;
use std::rc::Rc;
use std::time::Duration;

use smithay::backend::renderer::element::utils::{
    CropRenderElement, Relocate, RelocateRenderElement, RescaleRenderElement,
};
use smithay::output::Output;
use smithay::utils::{Logical, Point, Rectangle, Size};
use swayward_config::{CornerRadius, LayoutPart};

use super::insert_hint_element::{InsertHintElement, InsertHintRenderElement};
use super::tile::Tile;
use super::tiling_tree::NodeId;
use super::workspace::{
    compute_working_area, OutputId, Workspace, WorkspaceAddWindowTarget, WorkspaceId,
    WorkspaceRenderElement,
};
use super::{compute_overview_zoom, ActivateWindow, HitType, LayoutElement, Options, TiledWidth};
use crate::animation::{Animation, Clock};
use crate::input::swipe_tracker::SwipeTracker;
use crate::layout::RenderLayer;
use crate::render_helpers::renderer::NiriRenderer;
use crate::render_helpers::shadow::ShadowRenderElement;
use crate::render_helpers::solid_color::SolidColorRenderElement;
use crate::render_helpers::xray::XrayPos;
use crate::render_helpers::RenderCtx;
use crate::swayward_render_elements;
use crate::utils::transaction::Transaction;
use crate::utils::{
    output_size, round_logical_in_physical, round_logical_in_physical_max1, ResizeEdge,
};

/// Amount of DnD edge scrolling to scroll the height of one workspace.
///
/// This constant is tied to the default dnd-edge-workspace-switch max-speed setting.
const WORKSPACE_DND_EDGE_SCROLL_MOVEMENT: f64 = 1500.;

#[derive(Debug)]
pub struct Monitor<W: LayoutElement> {
    /// Output for this monitor.
    pub(super) output: Output,
    /// Cached name of the output.
    output_name: String,
    /// Latest known scale for this output.
    scale: smithay::output::Scale,
    /// Latest known size for this output.
    view_size: Size<f64, Logical>,
    /// Latest known working area for this output.
    ///
    /// Not rounded to physical pixels.
    // FIXME: since this is used for things like DnD scrolling edges in the overview, ideally this
    // should only consider overlay and top layer-shell surfaces. However, Smithay doesn't easily
    // let you do this at the moment.
    working_area: Rectangle<f64, Logical>,
    // Must always contain at least one.
    pub(super) workspaces: Vec<Workspace<W>>,
    /// Workspace IDs in sway's output child order, separate from spatial storage.
    sway_workspace_order: Vec<WorkspaceId>,
    /// Index of the currently active workspace.
    pub(super) active_workspace_idx: usize,
    /// Workspaces ordered from most to least recently focused.
    pub(super) workspace_focus_history: Vec<WorkspaceId>,
    /// ID of the previously active workspace.
    pub(super) previous_workspace_id: Option<WorkspaceId>,
    /// Sway name of the previously active workspace, retained after cleanup.
    pub(super) previous_workspace_name: Option<String>,
    /// In-progress switch between workspaces.
    pub(super) workspace_switch: Option<WorkspaceSwitch>,
    /// Indication where an interactively-moved window is about to be placed.
    pub(super) insert_hint: Option<InsertHint>,
    /// Insert hint element for rendering.
    insert_hint_element: InsertHintElement,
    /// Location to render the insert hint element.
    insert_hint_render_loc: Option<InsertHintRenderLoc>,
    /// Whether the overview is open.
    pub(super) overview_open: bool,
    /// Progress of the overview zoom animation, 1 is fully in overview.
    overview_progress: Option<OverviewProgress>,
    /// Clock for driving animations.
    pub(super) clock: Clock,
    /// Configurable properties of the layout as received from the parent layout.
    pub(super) base_options: Rc<Options>,
    /// Configurable properties of the layout.
    pub(super) options: Rc<Options>,
    /// Layout config overrides for this monitor.
    layout_config: Option<swayward_config::LayoutPart>,
}

#[derive(Debug)]
pub enum WorkspaceSwitch {
    Animation(Animation),
    DndScroll(DndScrollGesture),
}

#[derive(Debug)]
pub struct DndScrollGesture {
    /// Index of the workspace where the gesture was started.
    center_idx: usize,
    /// Fractional workspace index where the gesture was started.
    ///
    /// Can differ from center_idx when starting a gesture in the middle between workspaces, for
    /// example by "catching" an animation.
    start_idx: f64,
    /// Current, fractional workspace index.
    pub(super) current_idx: f64,
    /// Animation for the extra offset to the current position.
    ///
    /// For example, if there's a workspace switch during a DnD scroll.
    animation: Option<Animation>,
    tracker: SwipeTracker,
    /// Whether the gesture is controlled by the touchpad.
    /// Whether the gesture is clamped to +-1 workspace around the center.
    is_clamped: bool,

    // If this gesture is for drag-and-drop scrolling, this is the last event's unadjusted
    // timestamp.
    dnd_last_event_time: Option<Duration>,
    // Time when the drag-and-drop scroll delta became non-zero, used for debouncing.
    //
    // If `None` then the scroll delta is currently zero.
    dnd_nonzero_start_time: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InsertPosition {
    NewColumn(usize),
    InsertAt(NodeId, ResizeEdge),
    /// Drop landed on the middle of a tile, which sway treats as a swap rather
    /// than an insertion.
    SwapWith(NodeId),
    Floating,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum InsertWorkspace {
    Existing(WorkspaceId),
    Preview(WorkspacePreview),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct WorkspacePreview {
    pub insertion_index: usize,
    pub geometry: Rectangle<f64, Logical>,
}

#[derive(Debug)]
pub(super) struct InsertHint {
    pub workspace: InsertWorkspace,
    pub position: InsertPosition,
    pub corner_radius: CornerRadius,
}

#[derive(Debug, Clone, Copy)]
struct InsertHintRenderLoc {
    workspace: InsertWorkspace,
    location: Point<f64, Logical>,
}

#[derive(Debug)]
pub(super) enum OverviewProgress {
    Animation(Animation),
    Value(f64),
}

/// Where to put a newly added window.
#[derive(Debug, Default, PartialEq, Eq)]
pub enum MonitorAddWindowTarget<'a, W: LayoutElement> {
    /// No particular preference.
    #[default]
    Auto,
    /// On this workspace.
    Workspace {
        /// Id of the target workspace.
        id: WorkspaceId,
        /// Override where the window will open as a new column.
        column_idx: Option<usize>,
    },
    /// Next to this existing window.
    NextTo(&'a W::Id),
}

impl<'a, W: LayoutElement> Copy for MonitorAddWindowTarget<'a, W> {}

impl<'a, W: LayoutElement> Clone for MonitorAddWindowTarget<'a, W> {
    fn clone(&self) -> Self {
        *self
    }
}

swayward_render_elements! {
    MonitorInnerRenderElement<R> => {
        Workspace = CropRenderElement<WorkspaceRenderElement<R>>,
        InsertHint = CropRenderElement<InsertHintRenderElement>,
        UncroppedInsertHint = InsertHintRenderElement,
        Shadow = ShadowRenderElement,
        SolidColor = SolidColorRenderElement,
    }
}

pub type MonitorRenderElement<R> =
    RelocateRenderElement<RescaleRenderElement<MonitorInnerRenderElement<R>>>;

impl WorkspaceSwitch {
    pub fn current_idx(&self) -> f64 {
        match self {
            WorkspaceSwitch::Animation(anim) => anim.value(),
            WorkspaceSwitch::DndScroll(gesture) => {
                gesture.current_idx + gesture.animation.as_ref().map_or(0., |anim| anim.value())
            }
        }
    }

    pub fn target_idx(&self) -> f64 {
        match self {
            WorkspaceSwitch::Animation(anim) => anim.to(),
            WorkspaceSwitch::DndScroll(gesture) => gesture.current_idx,
        }
    }

    pub fn offset(&mut self, delta: isize) {
        match self {
            WorkspaceSwitch::Animation(anim) => anim.offset(delta as f64),
            WorkspaceSwitch::DndScroll(gesture) => {
                if delta >= 0 {
                    gesture.center_idx += delta as usize;
                } else {
                    gesture.center_idx -= (-delta) as usize;
                }
                gesture.start_idx += delta as f64;
                gesture.current_idx += delta as f64;
            }
        }
    }

    fn is_animation_ongoing(&self) -> bool {
        match self {
            WorkspaceSwitch::Animation(_) => true,
            WorkspaceSwitch::DndScroll(gesture) => gesture.animation.is_some(),
        }
    }
}

impl DndScrollGesture {
    fn min_max(&self, workspace_count: usize) -> (f64, f64) {
        if self.is_clamped {
            let min = self.center_idx.saturating_sub(1) as f64;
            let max = (self.center_idx + 1).min(workspace_count - 1) as f64;
            (min, max)
        } else {
            (0., (workspace_count - 1) as f64)
        }
    }

    fn animate_from(&mut self, from: f64, clock: Clock, config: swayward_config::Animation) {
        let current = self.animation.as_ref().map_or(0., Animation::value);
        self.animation = Some(Animation::new(clock, from + current, 0., 0., config));
    }
}

impl InsertWorkspace {
    fn existing_id(self) -> Option<WorkspaceId> {
        match self {
            InsertWorkspace::Existing(id) => Some(id),
            InsertWorkspace::Preview(_) => None,
        }
    }
}

impl OverviewProgress {
    pub fn value(&self) -> f64 {
        match self {
            OverviewProgress::Animation(anim) => anim.value(),
            OverviewProgress::Value(v) => *v,
        }
    }

    pub fn clamped_value(&self) -> f64 {
        match self {
            OverviewProgress::Animation(anim) => anim.clamped_value(),
            OverviewProgress::Value(v) => *v,
        }
    }
}

impl From<&super::OverviewProgress> for OverviewProgress {
    fn from(value: &super::OverviewProgress) -> Self {
        match value {
            super::OverviewProgress::Animation(anim) => Self::Animation(anim.clone()),
            super::OverviewProgress::Gesture(gesture) => Self::Value(gesture.value),
            super::OverviewProgress::Open => Self::Value(1.),
        }
    }
}

impl<W: LayoutElement> Monitor<W> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        output: Output,
        mut workspaces: Vec<Workspace<W>>,
        ws_id_to_activate: Option<WorkspaceId>,
        initial_workspace_name: Option<String>,
        initial_workspace_number: Option<i32>,
        clock: Clock,
        base_options: Rc<Options>,
        layout_config: Option<LayoutPart>,
    ) -> Self {
        let options =
            Rc::new(Options::clone(&base_options).with_merged_layout(layout_config.as_ref()));

        let scale = output.current_scale();
        let view_size = output_size(&output);
        let working_area = compute_working_area(&output);

        // Prepare the workspaces: set output, pick the active one.
        let mut active_workspace_idx = 0;

        for (idx, ws) in workspaces.iter_mut().enumerate() {
            assert!(ws.must_be_kept());

            ws.set_output(Some(output.clone()));
            ws.update_config(options.clone());

            if ws_id_to_activate.is_some_and(|id| ws.id() == id) {
                active_workspace_idx = idx;
            }
        }

        // A monitor always needs one workspace to be active on, but only one:
        // sway creates further workspaces on demand, and niri's always-empty
        // trailing placeholder is an affordance of its scrolling strip.
        if workspaces.is_empty() {
            let mut ws = Workspace::new(output.clone(), clock.clone(), options.clone());
            if let Some(name) = initial_workspace_name {
                let (name, number) =
                    super::sway_workspace_identity(crate::command::WorkspaceTarget::Name(name))
                        .unwrap();
                ws.set_sway_identity(name, number);
            } else if let Some(number) = initial_workspace_number {
                ws.set_sway_identity(None, Some(number));
            }
            workspaces.push(ws);
        }

        let sway_workspace_order = workspaces.iter().map(Workspace::id).collect();
        let workspace_focus_history = workspaces.iter().map(Workspace::id).rev().collect();

        Self {
            output_name: output.name(),
            output,
            scale,
            view_size,
            working_area,
            workspaces,
            sway_workspace_order,
            active_workspace_idx,
            workspace_focus_history,
            previous_workspace_id: None,
            previous_workspace_name: None,
            insert_hint: None,
            insert_hint_element: InsertHintElement::new(options.layout.insert_hint),
            insert_hint_render_loc: None,
            overview_open: false,
            overview_progress: None,
            workspace_switch: None,
            clock,
            base_options,
            options,
            layout_config,
        }
    }

    pub fn into_workspaces(mut self) -> Vec<Workspace<W>> {
        self.workspaces
            .retain(|ws| ws.has_windows() || ws.is_persistent());

        for ws in &mut self.workspaces {
            ws.set_output(None);
        }

        self.workspaces
    }

    pub fn output(&self) -> &Output {
        &self.output
    }

    pub fn output_name(&self) -> &String {
        &self.output_name
    }

    pub fn active_workspace_idx(&self) -> usize {
        self.active_workspace_idx
    }

    pub fn active_workspace_ref(&self) -> &Workspace<W> {
        &self.workspaces[self.active_workspace_idx]
    }

    pub fn find_named_workspace(&self, workspace_name: &str) -> Option<&Workspace<W>> {
        self.workspaces.iter().find(|workspace| {
            workspace
                .sway_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(workspace_name))
        })
    }

    pub fn find_named_workspace_index(&self, workspace_name: &str) -> Option<usize> {
        self.workspaces.iter().position(|ws| {
            ws.name
                .as_ref()
                .is_some_and(|name| name.eq_ignore_ascii_case(workspace_name))
        })
    }

    pub fn active_workspace(&mut self) -> &mut Workspace<W> {
        &mut self.workspaces[self.active_workspace_idx]
    }

    pub fn idx_of_ws(&self, id: WorkspaceId) -> Option<usize> {
        self.workspaces.iter().position(|ws| ws.id() == id)
    }

    pub fn has_ws(&self, id: WorkspaceId) -> bool {
        self.idx_of_ws(id).is_some()
    }

    pub fn windows(&self) -> impl Iterator<Item = &W> {
        self.workspaces.iter().flat_map(|ws| ws.windows())
    }

    pub fn has_window(&self, window: &W::Id) -> bool {
        self.windows().any(|win| win.id() == window)
    }

    pub fn add_workspace_at(&mut self, idx: usize) {
        let ws = Workspace::new(
            self.output.clone(),
            self.clock.clone(),
            self.options.clone(),
        );
        self.insert_new_workspace_at(idx, ws);
    }

    /// The number a client should see for the workspace at `index`.
    pub fn sway_workspace_number(&self, index: usize) -> i32 {
        let workspace = &self.workspaces[index];
        if let Some(number) = workspace.number() {
            return number;
        }
        if workspace.name().is_some() {
            return workspace.sway_display_number(index);
        }
        // Report the lowest number no other workspace already owns while an
        // initial workspace is waiting for its sway identity.
        let taken = self
            .workspaces
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .filter_map(|(_, workspace)| {
                workspace
                    .number()
                    .or_else(|| workspace.name().map(|name| super::sway_workspace_num(name)))
            })
            .collect::<Vec<_>>();
        (1..)
            .find(|candidate| !taken.contains(candidate))
            .unwrap_or(-1)
    }

    pub fn workspaces_len(&self) -> usize {
        self.workspaces.len()
    }

    /// The name a client sees for the workspace at `index`.
    pub fn sway_workspace_name(&self, index: usize) -> String {
        let workspace = &self.workspaces[index];
        workspace
            .sway_name()
            .unwrap_or_else(|| self.sway_workspace_number(index).to_string())
    }

    pub fn sort_sway_workspaces(&mut self) {
        // Sway sorts the output's child list, not compositor spatial storage.
        // Its stable sort preserves current order among equal-ranked entries
        // (sway/sway/tree/output.c:387-405).
        let keys = self
            .workspaces
            .iter()
            .enumerate()
            .map(|(index, workspace)| {
                let key = if !workspace.has_sway_identity() {
                    (2u8, 0)
                } else if workspace.number().is_some() {
                    (0, self.sway_workspace_number(index))
                } else {
                    (1, 0)
                };
                (workspace.id(), key)
            })
            .collect::<Vec<_>>();
        self.sway_workspace_order
            .retain(|id| keys.iter().any(|(candidate, _)| candidate == id));
        self.sway_workspace_order.sort_by_key(|id| {
            keys.iter()
                .find(|(candidate, _)| candidate == id)
                .map_or((2, 0), |(_, key)| *key)
        });

        let active = self.active_workspace_ref().id();
        self.workspaces.sort_by_key(|workspace| {
            let key = keys
                .iter()
                .find(|(id, _)| *id == workspace.id())
                .map_or((2, 0), |(_, key)| *key);
            (key.0, key.1, workspace.id())
        });
        self.active_workspace_idx = self.idx_of_ws(active).unwrap();
    }

    pub(crate) fn sway_workspaces(&self) -> impl Iterator<Item = (usize, &Workspace<W>)> {
        self.sway_workspace_order.iter().filter_map(|id| {
            let index = self.idx_of_ws(*id)?;
            Some((index, &self.workspaces[index]))
        })
    }

    /// Create a workspace, applying the per-name configuration for `name` when
    /// one exists.
    ///
    /// Sway reads the workspace config at creation time
    /// (`sway/sway/tree/workspace.c:224-243`), so a name configured with its own
    /// gaps gets them however the workspace comes to exist, not only when it is
    /// created eagerly at startup.
    pub fn add_sway_workspace_at(
        &mut self,
        idx: usize,
        name: Option<String>,
        number: Option<i32>,
        layout_config: Option<swayward_config::LayoutPart>,
    ) -> WorkspaceId {
        let mut ws = Workspace::new(
            self.output.clone(),
            self.clock.clone(),
            self.options.clone(),
        );
        if layout_config.is_some() {
            ws.adopt_configured_layout(layout_config);
        }
        ws.set_sway_identity(name, number);
        let id = ws.id();
        self.insert_new_workspace_at(idx, ws);
        self.sway_workspace_order
            .retain(|candidate| *candidate != id);
        self.sway_workspace_order.push(id);
        id
    }

    fn insert_new_workspace_at(&mut self, idx: usize, ws: Workspace<W>) {
        let order_index = self
            .workspaces
            .get(idx)
            .and_then(|next| {
                self.sway_workspace_order
                    .iter()
                    .position(|id| *id == next.id())
            })
            .unwrap_or(self.sway_workspace_order.len());
        self.sway_workspace_order.insert(order_index, ws.id());
        self.workspace_focus_history.push(ws.id());
        self.workspaces.insert(idx, ws);
        if idx <= self.active_workspace_idx {
            self.active_workspace_idx += 1;
        }

        if let Some(switch) = &mut self.workspace_switch {
            if idx as f64 <= switch.target_idx() {
                switch.offset(1);
            }
        }
    }

    pub fn add_workspace_top(&mut self) {
        self.add_workspace_at(0);
    }

    pub fn add_workspace_bottom(&mut self) {
        self.add_workspace_at(self.workspaces.len());
    }

    pub fn activate_workspace(&mut self, idx: usize) {
        self.activate_workspace_with_anim_config(idx, None);
    }

    pub fn activate_workspace_with_anim_config(
        &mut self,
        idx: usize,
        config: Option<swayward_config::Animation>,
    ) {
        // FIXME: also compute and use current velocity.
        let current_idx = self.workspace_render_idx();

        if self.active_workspace_idx != idx {
            let previous = &self.workspaces[self.active_workspace_idx];
            self.previous_workspace_id = Some(previous.id());
            self.previous_workspace_name = previous.sway_name();
        }

        let prev_active_idx = self.active_workspace_idx;
        self.active_workspace_idx = idx;
        let active = self.active_workspace_ref().id();
        self.workspace_focus_history.retain(|id| *id != active);
        self.workspace_focus_history.insert(0, active);
        self.move_sticky_to_active_workspace(prev_active_idx);

        let config = config.unwrap_or(self.options.animations.workspace_switch.0);

        match &mut self.workspace_switch {
            // During a DnD scroll, we want to visually animate even if idx matches the active idx.
            Some(WorkspaceSwitch::DndScroll(gesture)) if gesture.dnd_last_event_time.is_some() => {
                gesture.center_idx = idx;

                // Adjust start_idx to make current_idx point at idx.
                let current_pos = gesture.current_idx - gesture.start_idx;
                gesture.start_idx = idx as f64 - current_pos;
                let prev_current_idx = gesture.current_idx;
                gesture.current_idx = idx as f64;

                let current_idx_delta = gesture.current_idx - prev_current_idx;
                gesture.animate_from(-current_idx_delta, self.clock.clone(), config);
            }
            _ => {
                // Don't animate if nothing changed.
                if prev_active_idx == idx {
                    return;
                }

                self.workspace_switch = Some(WorkspaceSwitch::Animation(Animation::new(
                    self.clock.clone(),
                    current_idx,
                    idx as f64,
                    0.,
                    config,
                )));
            }
        }
    }

    pub(super) fn resolve_add_window_target<'a>(
        &mut self,
        target: MonitorAddWindowTarget<'a, W>,
    ) -> (usize, WorkspaceAddWindowTarget<'a, W>) {
        match target {
            MonitorAddWindowTarget::Auto => {
                (self.active_workspace_idx, WorkspaceAddWindowTarget::Auto)
            }
            MonitorAddWindowTarget::Workspace { id, column_idx } => {
                let idx = self.idx_of_ws(id).unwrap();
                let target = if let Some(column_idx) = column_idx {
                    WorkspaceAddWindowTarget::NewColumnAt(column_idx)
                } else {
                    WorkspaceAddWindowTarget::Auto
                };
                (idx, target)
            }
            MonitorAddWindowTarget::NextTo(win_id) => {
                let idx = self
                    .workspaces
                    .iter_mut()
                    .position(|ws| ws.has_window(win_id))
                    .unwrap();
                (idx, WorkspaceAddWindowTarget::NextTo(win_id))
            }
        }
    }

    pub fn add_window(
        &mut self,
        window: W,
        target: MonitorAddWindowTarget<W>,
        activate: ActivateWindow,
        width: TiledWidth,
        is_full_width: bool,
        is_floating: bool,
    ) {
        // Currently, everything a workspace sets on a Tile is the same across all workspaces of a
        // monitor. So we can use any workspace, not necessarily the exact target workspace.
        let tile = self.workspaces[0].make_tile(window);

        self.add_tile(
            tile,
            target,
            activate,
            true,
            width,
            is_full_width,
            is_floating,
            None,
        );
    }

    pub fn add_tiling_tile(&mut self, workspace_idx: usize, tile: Tile<W>, activate: bool) {
        let workspace = &mut self.workspaces[workspace_idx];

        workspace.add_tiling_tile(tile, activate);

        // After adding a new window, workspace becomes this output's own.
        if workspace.name().is_none() {
            workspace.original_output = OutputId::new(&self.output);
        }

        if activate {
            self.activate_workspace(workspace_idx);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_tile(
        &mut self,
        tile: Tile<W>,
        target: MonitorAddWindowTarget<W>,
        activate: ActivateWindow,
        // FIXME: Refactor ActivateWindow enum to make this better.
        allow_to_activate_workspace: bool,
        width: TiledWidth,
        is_full_width: bool,
        is_floating: bool,
        anim: Option<swayward_config::Animation>,
    ) {
        let (workspace_idx, target) = self.resolve_add_window_target(target);

        let workspace = &mut self.workspaces[workspace_idx];

        workspace.add_tile(
            tile,
            target,
            activate,
            width,
            is_full_width,
            is_floating,
            anim,
        );

        // After adding a new window, workspace becomes this output's own.
        if workspace.name().is_none() {
            workspace.original_output = OutputId::new(&self.output);
        }

        if allow_to_activate_workspace && activate.map_smart(|| false) {
            self.activate_workspace(workspace_idx);
        }
    }

    pub fn add_tile_at_drop(
        &mut self,
        workspace_idx: usize,
        target: NodeId,
        edge: ResizeEdge,
        tile: Tile<W>,
        activate: bool,
        // FIXME: Refactor ActivateWindow enum to make this better.
        allow_to_activate_workspace: bool,
    ) {
        let workspace = &mut self.workspaces[workspace_idx];

        workspace.add_tile_at_drop(tile, target, edge, activate);

        if workspace.name().is_none() {
            workspace.original_output = OutputId::new(&self.output);
        }

        if allow_to_activate_workspace && activate {
            self.activate_workspace(workspace_idx);
        }
    }

    fn move_sticky_to_active_workspace(&mut self, old_idx: usize) {
        if old_idx == self.active_workspace_idx {
            return;
        }
        let sticky = self.workspaces[old_idx].take_sticky_tiles();
        let target = &mut self.workspaces[self.active_workspace_idx];
        let target_was_empty = !target.has_windows();
        for removed in sticky {
            target.add_tile(
                removed.tile,
                WorkspaceAddWindowTarget::Auto,
                ActivateWindow::No,
                removed.width,
                removed.is_full_width,
                true,
                None,
            );
        }
        if target_was_empty {
            target.focus_workspace_node();
        }
    }

    /// Applies sway's post-transition workspace destruction guards.
    ///
    /// Sway destroys only empty, output-inactive workspaces not retained by a seat
    /// (`sway/sway/tree/workspace.c:314-332`). `previous_workspace_id` is navigation
    /// history, not seat inactive focus, so it does not retain the workspace here.
    pub fn consider_destroy_workspace(&mut self, id: WorkspaceId) {
        assert!(self.workspace_switch.is_none());
        let Some(idx) = self.idx_of_ws(id) else {
            return;
        };
        if idx == self.active_workspace_idx || self.workspaces[idx].must_be_kept() {
            return;
        }
        // A monitor always keeps at least one workspace to be active on.
        if self.workspaces.len() == 1 {
            return;
        }

        let removed = self.workspaces.remove(idx);
        self.sway_workspace_order.retain(|id| *id != removed.id());
        if idx < self.active_workspace_idx {
            self.active_workspace_idx -= 1;
        }
    }

    pub fn clean_up_workspaces(&mut self) {
        let candidates = self
            .workspaces
            .iter()
            .filter(|workspace| !workspace.is_persistent())
            .map(Workspace::id)
            .collect::<Vec<_>>();
        for id in candidates {
            self.consider_destroy_workspace(id);
        }
    }

    pub fn unname_workspace(&mut self, id: WorkspaceId) -> bool {
        let Some(idx) = self.idx_of_ws(id) else {
            return false;
        };
        let ws = &mut self.workspaces[idx];

        ws.unname();

        if self.workspace_switch.is_none() {
            self.clean_up_workspaces();
        }

        true
    }

    pub fn detach_workspace(&mut self, id: WorkspaceId) -> Option<Workspace<W>> {
        let idx = self.idx_of_ws(id)?;
        let mut ws = self.workspaces.remove(idx);
        self.sway_workspace_order
            .retain(|candidate| *candidate != id);
        ws.set_output(None);

        // For monitor current workspace removal, we focus previous rather than next (<= rather
        // than <). This is different from columns and tiles, but it lets move-workspace-to-monitor
        // back and forth to preserve position.
        if idx <= self.active_workspace_idx && self.active_workspace_idx > 0 {
            self.active_workspace_idx -= 1;
        }

        self.workspace_switch = None;
        Some(ws)
    }

    pub fn attach_workspace(&mut self, mut ws: Workspace<W>, mut idx: usize, activate: bool) {
        ws.set_output(Some(self.output.clone()));
        ws.update_config(self.options.clone());

        idx = idx.min(self.workspaces.len());
        let order_index = self
            .workspaces
            .get(idx)
            .and_then(|next| {
                self.sway_workspace_order
                    .iter()
                    .position(|id| *id == next.id())
            })
            .unwrap_or(self.sway_workspace_order.len());
        self.sway_workspace_order.insert(order_index, ws.id());
        self.workspace_focus_history.push(ws.id());
        self.workspaces.insert(idx, ws);

        if idx <= self.active_workspace_idx {
            self.active_workspace_idx += 1;
        }

        if activate {
            self.workspace_switch = None;
            self.activate_workspace(idx);
        }

        self.workspace_switch = None;
        self.clean_up_workspaces();
    }

    pub fn insert_workspace(&mut self, ws: Workspace<W>, idx: usize, activate: bool) {
        self.attach_workspace(ws, idx, activate);
        self.clean_up_workspaces();
    }

    /// Destroy empty workspaces that focus has left, then re-sort.
    ///
    /// This is sway's `workspace_consider_destroy` applied across the monitor
    /// (`sway/tree/workspace.c:313-330`). It used to also append an
    /// always-empty trailing workspace, which is niri's scrolling-strip
    /// affordance rather than anything sway has.
    pub fn reap_empty_workspaces(&mut self) {
        if self.workspaces.is_empty() {
            self.add_workspace_bottom();
            self.active_workspace_idx = 0;
            return;
        }
        let active = self.active_workspace_ref().id();
        self.workspace_switch = None;

        let doomed = self
            .workspaces
            .iter()
            .enumerate()
            .filter(|(_, workspace)| !workspace.must_be_kept())
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();
        for idx in doomed.into_iter().rev() {
            if idx != self.active_workspace_idx {
                let removed = self.workspaces.remove(idx);
                self.sway_workspace_order.retain(|id| *id != removed.id());
                if idx < self.active_workspace_idx {
                    self.active_workspace_idx -= 1;
                }
            }
        }

        // A monitor always keeps at least one workspace to be active on.
        if self.workspaces.is_empty() {
            self.add_workspace_bottom();
            self.active_workspace_idx = 0;
            return;
        }
        self.sort_sway_workspaces();
        self.active_workspace_idx = self.idx_of_ws(active).unwrap_or(0);
    }

    pub fn append_workspaces(&mut self, mut workspaces: Vec<Workspace<W>>) {
        if workspaces.is_empty() {
            return;
        }

        for ws in &mut workspaces {
            ws.set_output(Some(self.output.clone()));
            ws.update_config(self.options.clone());
        }

        let active = self.active_workspace_ref().id();

        self.sway_workspace_order
            .extend(workspaces.iter().map(Workspace::id));
        self.workspaces.extend(workspaces);
        self.reap_empty_workspaces();
        self.active_workspace_idx = self.idx_of_ws(active).unwrap();

        // FIXME: if we're adding workspaces to currently invisible positions
        // (outside the workspace switch), we don't need to cancel it.
        self.workspace_switch = None;
        self.clean_up_workspaces();
    }

    pub fn move_down_or_to_workspace_down(&mut self) {
        if !self.active_workspace().move_down() {
            self.move_to_workspace_down(ActivateWindow::Smart);
        }
    }

    pub fn move_up_or_to_workspace_up(&mut self) {
        if !self.active_workspace().move_up() {
            self.move_to_workspace_up(ActivateWindow::Smart);
        }
    }

    pub fn focus_window_or_workspace_down(&mut self) {
        if !self.active_workspace().focus_down() {
            self.switch_workspace_down();
        }
    }

    pub fn focus_window_or_workspace_up(&mut self) {
        if !self.active_workspace().focus_up() {
            self.switch_workspace_up();
        }
    }

    pub fn move_tiling_subtree_to_workspace(
        &mut self,
        source_workspace: WorkspaceId,
        node: NodeId,
        target_workspace: WorkspaceId,
        preserve_empty_workspace: bool,
    ) -> Option<Vec<(NodeId, NodeId)>> {
        if source_workspace == target_workspace {
            return Some(Vec::new());
        }
        let source_idx = self.idx_of_ws(source_workspace)?;
        let target_idx = self.idx_of_ws(target_workspace)?;
        let (subtree, old_parent) = self.workspaces[source_idx].detach_tiling_subtree(node)?;
        let remapped = self.workspaces[target_idx].attach_tiling_subtree(subtree).1;
        self.workspaces[source_idx].finish_tiling_subtree_detach(old_parent);
        if !preserve_empty_workspace && self.workspace_switch.is_none() {
            self.consider_destroy_workspace(source_workspace);
        }
        Some(remapped)
    }

    pub fn move_to_workspace_up(&mut self, activate: ActivateWindow) {
        let new_idx = self.active_workspace_idx.saturating_sub(1);
        let target = self.workspaces[new_idx].id();
        self.move_to_workspace(None, target, activate);
    }

    pub fn move_to_workspace_down(&mut self, activate: ActivateWindow) {
        let new_idx = min(self.active_workspace_idx + 1, self.workspaces.len() - 1);
        let target = self.workspaces[new_idx].id();
        self.move_to_workspace(None, target, activate);
    }

    pub fn move_to_workspace(
        &mut self,
        window: Option<&W::Id>,
        target: WorkspaceId,
        activate: ActivateWindow,
    ) {
        let source_workspace_idx = if let Some(window) = window {
            self.workspaces
                .iter()
                .position(|ws| ws.has_window(window))
                .unwrap()
        } else {
            self.active_workspace_idx
        };
        let source_id = self.workspaces[source_workspace_idx].id();

        let Some(new_idx) = self.idx_of_ws(target) else {
            return;
        };
        if new_idx == source_workspace_idx {
            return;
        }
        let new_id = target;

        let activate = activate.map_smart(|| {
            window.is_none_or(|win| self.active_window().map(|win| win.id()) == Some(win))
        });

        let workspace = &mut self.workspaces[source_workspace_idx];
        let Some(window) = window.or_else(|| workspace.active_window().map(|win| win.id())) else {
            return;
        };
        let window = window.clone();

        let mut old_render_pos = workspace
            .tiles_with_render_positions()
            .find_map(|(tile, offset, _visible)| (tile.window().id() == &window).then_some(offset))
            .unwrap();

        let fullscreen = workspace.fullscreen_mode();
        let fullscreen_window = workspace.fullscreen_window().cloned();
        let transaction = Transaction::new();
        let removed = workspace.remove_tile(&window, transaction);

        // If the view is following the tile, match the animation.
        let config = if activate {
            self.options.animations.workspace_switch.0
        } else {
            self.options.animations.window_movement.0
        };

        self.add_tile(
            removed.tile,
            MonitorAddWindowTarget::Workspace {
                id: new_id,
                column_idx: None,
            },
            if activate {
                ActivateWindow::Yes
            } else {
                ActivateWindow::No
            },
            true,
            removed.width,
            removed.is_full_width,
            removed.is_floating,
            Some(config),
        );
        if let (Some(fullscreen), Some(fullscreen_window)) = (fullscreen, fullscreen_window) {
            self.workspaces[new_idx].set_window_fullscreen(&fullscreen_window, Some(fullscreen));
            self.workspaces[new_idx].set_fullscreen_restore_to_floating(&fullscreen_window);
        }

        if self.workspace_switch.is_none() {
            self.consider_destroy_workspace(source_id);
        }

        let new_idx = self.idx_of_ws(new_id).unwrap();

        // Animate vertical movement between workspaces.
        //
        // Recompute the source idx in case some workspace was removed during clean-up. If the
        // source workspace itself was removed, don't bother animating this since the removal is
        // instant anyway.
        if let Some(source_workspace_idx) = self.idx_of_ws(source_id) {
            old_render_pos.y +=
                self.workspace_size_with_gap(1.).h * (source_workspace_idx as f64 - new_idx as f64);
        }

        let (tile, new_render_pos) = self.workspaces[new_idx]
            .tiles_with_render_positions_mut(false)
            .find(|(tile, _)| tile.window().id() == &window)
            .unwrap();
        tile.animate_move_from_with_config(old_render_pos - new_render_pos, config);
        tile.set_anim_y_between_workspaces();
    }

    pub fn move_focused_to_workspace(&mut self, target: WorkspaceId, activate: bool) {
        let source_workspace = self.active_workspace_ref().id();
        if target == source_workspace {
            return;
        }

        let source_idx = self.idx_of_ws(source_workspace).unwrap();
        let target_idx = self.idx_of_ws(target).unwrap();
        let workspace = &mut self.workspaces[source_idx];
        if workspace.floating_is_active() {
            let activate = if activate {
                ActivateWindow::Smart
            } else {
                ActivateWindow::No
            };
            self.move_to_workspace(None, target, activate);
            return;
        }

        let Some(window) = workspace.active_window().map(|window| window.id().clone()) else {
            return;
        };
        let mut old_render_pos = workspace
            .tiles_with_render_positions()
            .find_map(|(tile, pos, _)| (tile.window().id() == &window).then_some(pos))
            .unwrap();
        let tile = workspace.remove_active_tiling_tile().unwrap();

        old_render_pos.y +=
            self.workspace_size_with_gap(1.).h * (source_idx as f64 - target_idx as f64);
        let config = if activate {
            self.options.animations.workspace_switch.0
        } else {
            self.options.animations.window_movement.0
        };

        self.add_tiling_tile(target_idx, tile, activate);
        if self.workspace_switch.is_none() {
            self.consider_destroy_workspace(source_workspace);
        }

        let target_idx = self.idx_of_ws(target).unwrap();
        let (tile, new_render_pos) = self.workspaces[target_idx]
            .tiles_with_render_positions_mut(false)
            .find(|(tile, _)| tile.window().id() == &window)
            .unwrap();
        tile.animate_move_from_with_config(old_render_pos - new_render_pos, config);
        tile.set_anim_y_between_workspaces();
    }

    pub fn switch_workspace_up(&mut self) {
        let new_idx = match &self.workspace_switch {
            // During a DnD scroll, select the prev apparent workspace.
            Some(WorkspaceSwitch::DndScroll(gesture)) if gesture.dnd_last_event_time.is_some() => {
                let current = gesture.current_idx;
                let new = current.ceil() - 1.;
                new.clamp(0., (self.workspaces.len() - 1) as f64) as usize
            }
            _ => self.active_workspace_idx.saturating_sub(1),
        };

        self.activate_workspace(new_idx);
    }

    pub fn switch_workspace_down(&mut self) {
        let new_idx = match &self.workspace_switch {
            // During a DnD scroll, select the next apparent workspace.
            Some(WorkspaceSwitch::DndScroll(gesture)) if gesture.dnd_last_event_time.is_some() => {
                let current = gesture.current_idx;
                let new = current.floor() + 1.;
                new.clamp(0., (self.workspaces.len() - 1) as f64) as usize
            }
            _ => min(self.active_workspace_idx + 1, self.workspaces.len() - 1),
        };

        self.activate_workspace(new_idx);
    }

    /// Selects the previous workspace, wrapping to the last one.
    ///
    /// The overview shows the whole stack at once, so stopping dead at the
    /// ends reads as a broken key rather than as an edge: with two
    /// workspaces, one of the two arrows always appears to do nothing. Sway's
    /// own `workspace prev` wraps for the same reason
    /// (Layout::relative_sway_workspace_position falls back to the maximum).
    pub fn switch_workspace_up_wrapping(&mut self) {
        if self.workspace_switch.is_some() || self.workspaces.len() < 2 {
            self.switch_workspace_up();
            return;
        }

        let new_idx = self
            .active_workspace_idx
            .checked_sub(1)
            .unwrap_or(self.workspaces.len() - 1);
        self.activate_workspace(new_idx);
    }

    /// Selects the next workspace, wrapping to the first one.
    ///
    /// See [`Self::switch_workspace_up_wrapping`].
    pub fn switch_workspace_down_wrapping(&mut self) {
        if self.workspace_switch.is_some() || self.workspaces.len() < 2 {
            self.switch_workspace_down();
            return;
        }

        let new_idx = (self.active_workspace_idx + 1) % self.workspaces.len();
        self.activate_workspace(new_idx);
    }

    pub(super) fn previous_workspace_idx(&self) -> Option<usize> {
        let id = self.previous_workspace_id?;
        self.idx_of_ws(id)
    }

    pub fn previous_workspace_id(&self) -> Option<WorkspaceId> {
        self.previous_workspace_id
    }

    pub(crate) fn workspace_focus_history(&self) -> impl Iterator<Item = WorkspaceId> + '_ {
        self.workspace_focus_history.iter().copied()
    }

    /// Re-read the cached back-and-forth name for `id` after a rename.
    pub fn refresh_previous_workspace_name(&mut self, id: WorkspaceId) {
        if self.previous_workspace_id != Some(id) {
            return;
        }
        self.previous_workspace_name = self
            .workspaces
            .iter()
            .find(|workspace| workspace.id() == id)
            .and_then(Workspace::sway_name);
    }

    pub(super) fn previous_workspace_name(&self) -> Option<&str> {
        self.previous_workspace_name.as_deref()
    }

    pub fn switch_workspace(&mut self, idx: usize) {
        self.activate_workspace(min(idx, self.workspaces.len() - 1));
    }

    pub fn switch_workspace_auto_back_and_forth(&mut self, idx: usize) {
        let idx = min(idx, self.workspaces.len() - 1);

        if idx == self.active_workspace_idx {
            if let Some(prev_idx) = self.previous_workspace_idx() {
                self.switch_workspace(prev_idx);
            }
        } else {
            self.switch_workspace(idx);
        }
    }

    pub fn switch_workspace_previous(&mut self) {
        if let Some(idx) = self.previous_workspace_idx() {
            self.switch_workspace(idx);
        }
    }

    pub fn active_window(&self) -> Option<&W> {
        self.active_workspace_ref().active_window()
    }

    pub fn advance_animations(&mut self) {
        match &mut self.workspace_switch {
            Some(WorkspaceSwitch::Animation(anim)) => {
                if anim.is_done() {
                    self.workspace_switch = None;
                    if let Some(previous) = self.previous_workspace_id {
                        self.consider_destroy_workspace(previous);
                    }
                    self.clean_up_workspaces();
                    self.reap_empty_workspaces();
                }
            }
            Some(WorkspaceSwitch::DndScroll(gesture)) => {
                // Make sure the last event time doesn't go too much out of date (for
                // monitors not under cursor), causing sudden jumps.
                //
                // This happens after any dnd_scroll_gesture_scroll() calls (in
                // Layout::advance_animations()), so it doesn't mess up the time delta there.
                if let Some(last_time) = &mut gesture.dnd_last_event_time {
                    let now = self.clock.now_unadjusted();
                    if *last_time != now {
                        *last_time = now;

                        // If last_time was already == now, then dnd_scroll_gesture_scroll() must've
                        // updated the gesture already. Therefore, when this code runs, the pointer
                        // must be outside the DnD scrolling zone.
                        gesture.dnd_nonzero_start_time = None;
                    }
                }

                if let Some(anim) = &mut gesture.animation {
                    if anim.is_done() {
                        gesture.animation = None;
                    }
                }
            }
            None => (),
        }

        for ws in &mut self.workspaces {
            ws.advance_animations();
        }
    }

    pub(super) fn are_animations_ongoing(&self) -> bool {
        self.workspace_switch
            .as_ref()
            .is_some_and(|s| s.is_animation_ongoing())
            || self.workspaces.iter().any(|ws| ws.are_animations_ongoing())
    }

    pub fn are_transitions_ongoing(&self) -> bool {
        self.workspace_switch.is_some()
            || self
                .workspaces
                .iter()
                .any(|ws| ws.are_transitions_ongoing())
    }

    pub fn update_render_elements(&mut self, is_active: bool) {
        let mut insert_hint_ws_geo = None;
        let insert_hint_ws_id = self
            .insert_hint
            .as_ref()
            .and_then(|hint| hint.workspace.existing_id());

        for ws in &mut self.workspaces {
            ws.update_render_elements(is_active, RenderLayer::MovingBetweenWorkspaces);
        }

        for (ws, geo) in self.workspaces_with_render_geo_mut(true) {
            ws.update_render_elements(is_active, RenderLayer::Normal);

            if Some(ws.id()) == insert_hint_ws_id {
                insert_hint_ws_geo = Some(geo);
            }
        }

        self.insert_hint_render_loc = None;
        if let Some(hint) = &self.insert_hint {
            match hint.workspace {
                InsertWorkspace::Existing(ws_id) => {
                    if let Some(idx) = self.idx_of_ws(ws_id) {
                        let ws = &self.workspaces[idx];
                        if let Some(mut area) = ws.insert_hint_area(hint.position) {
                            let scale = ws.scale().fractional_scale();
                            let view_size = ws.view_size();

                            // Make sure the hint is at least partially visible.
                            if matches!(hint.position, InsertPosition::NewColumn(_)) {
                                let zoom = self.overview_zoom();
                                let geo = insert_hint_ws_geo.unwrap();
                                let geo = geo.downscale(zoom);

                                area.loc.x = area.loc.x.max(-geo.loc.x - area.size.w / 2.);
                                area.loc.x =
                                    area.loc.x.min(geo.loc.x + geo.size.w - area.size.w / 2.);
                            }

                            // Round to physical pixels.
                            area = area.to_physical_precise_round(scale).to_logical(scale);

                            let view_rect = Rectangle::new(area.loc.upscale(-1.), view_size);
                            self.insert_hint_element.update_render_elements(
                                area.size,
                                view_rect,
                                hint.corner_radius,
                                scale,
                            );
                            self.insert_hint_render_loc = Some(InsertHintRenderLoc {
                                workspace: hint.workspace,
                                location: area.loc,
                            });
                        }
                    } else {
                        error!("insert hint workspace missing from monitor");
                    }
                }
                InsertWorkspace::Preview(preview) => {
                    let scale = self.scale.fractional_scale();
                    let gap = self.workspace_gap(self.overview_zoom());

                    let hint_gap = round_logical_in_physical(scale, gap * 0.1);
                    let hint_height = gap - hint_gap * 2.;

                    let next_ws_geo = preview.geometry;
                    let hint_width = round_logical_in_physical(scale, next_ws_geo.size.w * 0.75);
                    let hint_x =
                        round_logical_in_physical(scale, (next_ws_geo.size.w - hint_width) / 2.);

                    let hint_loc_diff = Point::from((-hint_x, hint_height + hint_gap));
                    let hint_loc = next_ws_geo.loc - hint_loc_diff;
                    let hint_size = Size::from((hint_width, hint_height));

                    // Sometimes the hint ends up 1 px wider than necessary and/or 1 px
                    // narrower than necessary. The values here seem correct. Might have to do with
                    // how zooming out currently doesn't round to output scale properly.

                    // Compute view rect as if we're above the next workspace (rather than below
                    // the previous one).
                    let view_rect = Rectangle::new(hint_loc_diff, next_ws_geo.size);

                    self.insert_hint_element.update_render_elements(
                        hint_size,
                        view_rect,
                        CornerRadius::default(),
                        scale,
                    );
                    self.insert_hint_render_loc = Some(InsertHintRenderLoc {
                        workspace: hint.workspace,
                        location: hint_loc,
                    });
                }
            }
        }
    }

    pub fn update_config(&mut self, base_options: Rc<Options>) {
        let options =
            Rc::new(Options::clone(&base_options).with_merged_layout(self.layout_config.as_ref()));

        for ws in &mut self.workspaces {
            ws.update_config(options.clone());
        }

        self.insert_hint_element
            .update_config(options.layout.insert_hint);

        self.base_options = base_options;
        self.options = options;
    }

    pub fn update_layout_config(
        &mut self,
        layout_config: Option<swayward_config::LayoutPart>,
    ) -> bool {
        if self.layout_config == layout_config {
            return false;
        }

        self.layout_config = layout_config;
        self.update_config(self.base_options.clone());

        true
    }

    pub fn update_shaders(&mut self) {
        for ws in &mut self.workspaces {
            ws.update_shaders();
        }

        self.insert_hint_element.update_shaders();
    }

    pub fn update_output_size(&mut self) {
        self.scale = self.output.current_scale();
        self.view_size = output_size(&self.output);
        self.working_area = compute_working_area(&self.output);

        for ws in &mut self.workspaces {
            ws.update_output_size();
        }
    }

    pub fn move_workspace_down(&mut self) {
        let new_idx = min(self.active_workspace_idx + 1, self.workspaces.len() - 1);
        if new_idx == self.active_workspace_idx {
            return;
        }

        self.workspaces.swap(self.active_workspace_idx, new_idx);

        let previous_workspace_id = self.previous_workspace_id;
        let previous_workspace_name = self.previous_workspace_name.clone();
        self.activate_workspace(new_idx);
        self.workspace_switch = None;
        self.previous_workspace_id = previous_workspace_id;
        self.previous_workspace_name = previous_workspace_name;

        self.clean_up_workspaces();
        // Swapping can leave a workspace with windows last, which the
        // invariants reject; the guards above test the pre-swap index.
        self.reap_empty_workspaces();
    }

    pub fn move_workspace_up(&mut self) {
        let new_idx = self.active_workspace_idx.saturating_sub(1);
        if new_idx == self.active_workspace_idx {
            return;
        }

        self.workspaces.swap(self.active_workspace_idx, new_idx);

        let previous_workspace_id = self.previous_workspace_id;
        let previous_workspace_name = self.previous_workspace_name.clone();
        self.activate_workspace(new_idx);
        self.workspace_switch = None;
        self.previous_workspace_id = previous_workspace_id;
        self.previous_workspace_name = previous_workspace_name;

        self.clean_up_workspaces();
        // Swapping can leave a workspace with windows last, which the
        // invariants reject; the guards above test the pre-swap index.
        self.reap_empty_workspaces();
    }

    pub fn move_workspace_to_idx(&mut self, old_idx: usize, new_idx: usize) {
        if self.workspaces.len() <= old_idx {
            return;
        }

        let new_idx = new_idx.clamp(0, self.workspaces.len() - 1);
        if old_idx == new_idx {
            return;
        }

        let ws = self.workspaces.remove(old_idx);
        self.workspaces.insert(new_idx, ws);

        // Only refocus the workspace if it was already focused
        if self.active_workspace_idx == old_idx {
            self.active_workspace_idx = new_idx;
        // If the workspace order was switched so that the current workspace moved down the
        // workspace stack, focus correctly
        } else if new_idx <= self.active_workspace_idx && old_idx > self.active_workspace_idx {
            self.active_workspace_idx += 1;
        } else if new_idx >= self.active_workspace_idx && old_idx < self.active_workspace_idx {
            self.active_workspace_idx = self.active_workspace_idx.saturating_sub(1);
        }

        self.workspace_switch = None;

        self.clean_up_workspaces();
        // Swapping can leave a workspace with windows last, which the
        // invariants reject; the guards above test the pre-swap index.
        self.reap_empty_workspaces();
    }

    /// Returns the geometry of the active window relative to and clamped to the output.
    ///
    /// During animations, assumes the final view position.
    pub fn active_window_visual_rectangle(&self) -> Option<Rectangle<f64, Logical>> {
        if self.overview_open {
            return None;
        }

        self.active_workspace_ref().active_window_visual_rectangle()
    }

    fn workspace_size(&self, zoom: f64) -> Size<f64, Logical> {
        let ws_size = self.view_size.upscale(zoom);
        let scale = self.scale.fractional_scale();
        ws_size.to_physical_precise_ceil(scale).to_logical(scale)
    }

    fn workspace_gap(&self, zoom: f64) -> f64 {
        let scale = self.scale.fractional_scale();
        let gap = self.view_size.h * 0.1 * zoom;
        round_logical_in_physical_max1(scale, gap)
    }

    fn workspace_size_with_gap(&self, zoom: f64) -> Size<f64, Logical> {
        let gap = self.workspace_gap(zoom);
        self.workspace_size(zoom) + Size::from((0., gap))
    }

    pub fn overview_zoom(&self) -> f64 {
        let progress = self.overview_progress.as_ref().map(|p| p.value());
        compute_overview_zoom(&self.options, progress)
    }

    pub(super) fn set_overview_progress(&mut self, progress: Option<&super::OverviewProgress>) {
        let prev_render_idx = self.workspace_render_idx();
        self.overview_progress = progress.map(OverviewProgress::from);
        let new_render_idx = self.workspace_render_idx();

        // If the view jumped (can happen when going from corrected to uncorrected render_idx, for
        // example when toggling the overview in the middle of an overview animation), then restart
        // the workspace switch to avoid jumps.
        if prev_render_idx != new_render_idx {
            if let Some(WorkspaceSwitch::Animation(anim)) = &mut self.workspace_switch {
                // FIXME: maintain velocity.
                *anim = anim.restarted(prev_render_idx, anim.to(), 0.);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn overview_progress_value(&self) -> Option<f64> {
        self.overview_progress.as_ref().map(|p| p.value())
    }

    pub fn workspace_render_idx(&self) -> f64 {
        // If workspace switch and overview progress are matching animations, then compute a
        // correction term to make the movement appear monotonic.
        if let (
            Some(WorkspaceSwitch::Animation(switch_anim)),
            Some(OverviewProgress::Animation(progress_anim)),
        ) = (&self.workspace_switch, &self.overview_progress)
        {
            if switch_anim.start_time() == progress_anim.start_time()
                && (switch_anim.duration().as_secs_f64() - progress_anim.duration().as_secs_f64())
                    .abs()
                    <= 0.001
            {
                #[rustfmt::skip]
                // How this was derived:
                //
                // - Assume we're animating a zoom + switch. Consider switch "from" and "to".
                //   These are render_idx values, so first workspace to second would have switch
                //   from = 0. and to = 1. regardless of the zoom level.
                //
                // - At the start, the point at "from" is at Y = 0. We're moving the point at "to"
                //   to Y = 0. We want this to be a monotonic motion in apparent coordinates (after
                //   zoom).
                //
                // - Height at the start:
                //   from_height = (size.h + gap) * from_zoom.
                //
                // - Current height:
                //   current_height = (size.h + gap) * zoom.
                //
                // - We're moving the "to" point to Y = 0:
                //   to_y = 0.
                //
                // - The initial position of the point we're moving:
                //   from_y = (to - from) * from_height.
                //
                // - We want this point to travel monotonically in apparent coordinates:
                //   current_y = from_y + (to_y - from_y) * progress,
                //   where progress is from 0 to 1, equals to the animation progress (switch and
                //   zoom are the same since they are synchronized).
                //
                // - Derive the Y of the first workspace from this:
                //   first_y = current_y - to * current_height.
                //
                // Now, let's substitute and rearrange the terms.
                //
                // - current_y = from_y + (0 - (to - from) * from_height) * progress
                // - progress = (switch_anim.value() - from) / (to - from)
                // - current_y = from_y - (to - from) * from_height * (switch_anim.value() - from) / (to - from)
                // - current_y = from_y - from_height * (switch_anim.value() - from)
                // - first_y = from_y - from_height * (switch_anim.value() - from) - to * current_height
                // - first_y = (to - from) * from_height - from_height * (switch_anim.value() - from) - to * current_height
                // - first_y = to * from_height - switch_anim.value() * from_height - to * current_height
                // - first_y = -switch_anim.value() * from_height + to * (from_height - current_height)
                let from = progress_anim.from();
                let from_zoom = compute_overview_zoom(&self.options, Some(from));
                let from_ws_height_with_gap = self.workspace_size_with_gap(from_zoom).h;

                let zoom = self.overview_zoom();
                let ws_height_with_gap = self.workspace_size_with_gap(zoom).h;

                let first_ws_y = -switch_anim.value() * from_ws_height_with_gap
                    + switch_anim.to() * (from_ws_height_with_gap - ws_height_with_gap);

                return -first_ws_y / ws_height_with_gap;
            }
        };

        if let Some(switch) = &self.workspace_switch {
            switch.current_idx()
        } else {
            self.active_workspace_idx as f64
        }
    }

    pub fn workspaces_render_geo(&self) -> impl Iterator<Item = Rectangle<f64, Logical>> {
        let scale = self.scale.fractional_scale();
        let zoom = self.overview_zoom();

        let ws_size = self.workspace_size(zoom);
        let gap = self.workspace_gap(zoom);
        let ws_height_with_gap = ws_size.h + gap;

        let static_offset = (self.view_size.to_point() - ws_size.to_point()).downscale(2.);
        let static_offset = static_offset
            .to_physical_precise_round(scale)
            .to_logical(scale);

        let first_ws_y = -self.workspace_render_idx() * ws_height_with_gap;
        let first_ws_y = round_logical_in_physical(scale, first_ws_y);

        (0..self.workspaces.len()).map(move |idx| {
            let y = first_ws_y + idx as f64 * ws_height_with_gap;
            let loc = Point::from((0., y)) + static_offset;

            // Even though all components that go into loc are rounded to physical pixels, the
            // floating point addition may lose precision. This can result for example in the
            // current workspace having y = 0.0000000000002 and thus missing pointer hits at the
            // monitor edge with y = 0. So, post-round the location too.
            let loc = loc.to_physical_precise_round(scale).to_logical(scale);

            Rectangle::new(loc, ws_size)
        })
    }

    pub fn workspaces_with_render_geo_cull(
        &self,
        cull: bool,
    ) -> impl Iterator<Item = (&Workspace<W>, Rectangle<f64, Logical>)> {
        let output_geo = Rectangle::from_size(self.view_size);

        let geo = self.workspaces_render_geo();
        zip(self.workspaces.iter(), geo)
            // Cull out workspaces outside the output.
            .filter(move |(_ws, geo)| !cull || geo.intersection(output_geo).is_some())
    }

    pub fn workspaces_with_render_geo(
        &self,
    ) -> impl Iterator<Item = (&Workspace<W>, Rectangle<f64, Logical>)> {
        self.workspaces_with_render_geo_cull(true)
    }

    pub fn workspaces_with_render_geo_idx(
        &self,
    ) -> impl Iterator<Item = ((usize, &Workspace<W>), Rectangle<f64, Logical>)> {
        let output_geo = Rectangle::from_size(self.view_size);

        let geo = self.workspaces_render_geo();
        zip(self.workspaces.iter().enumerate(), geo)
            // Cull out workspaces outside the output.
            .filter(move |(_ws, geo)| geo.intersection(output_geo).is_some())
    }

    pub fn workspaces_with_render_geo_mut(
        &mut self,
        cull: bool,
    ) -> impl Iterator<Item = (&mut Workspace<W>, Rectangle<f64, Logical>)> {
        let output_geo = Rectangle::from_size(self.view_size);

        let geo = self.workspaces_render_geo();
        zip(self.workspaces.iter_mut(), geo)
            // Cull out workspaces outside the output.
            .filter(move |(_ws, geo)| !cull || geo.intersection(output_geo).is_some())
    }

    // Render geometry versus layout geometry.
    //
    // The queries below deliberately use rendered positions, which include
    // in-flight animation offsets. They answer "what is under this pointer",
    // so they must agree with what the user can see; settled geometry here
    // would make a click during an animation select the wrong window.
    //
    // Anything answering a question about STATE must use settled geometry
    // instead: IPC replies, command targeting, focus resolution and criteria
    // matching. Reading rendered positions there reports transient values, and
    // has caused three real bugs - a floating GET_TREE rect that moved while
    // the window did not, a directional output move that picked the wrong
    // output mid workspace switch, and a dialog placed ~950px from its parent.
    // Prefer tiles_with_ipc_layouts or FloatingData::center.
    pub fn workspace_under(
        &self,
        pos_within_output: Point<f64, Logical>,
    ) -> Option<(&Workspace<W>, Rectangle<f64, Logical>)> {
        let (ws, geo) = self.workspaces_with_render_geo().find_map(|(ws, geo)| {
            // Extend width to entire output.
            let loc = Point::from((0., geo.loc.y));
            let size = Size::from((self.view_size.w, geo.size.h));
            let bounds = Rectangle::new(loc, size);

            bounds.contains(pos_within_output).then_some((ws, geo))
        })?;
        Some((ws, geo))
    }

    pub fn workspace_under_narrow(
        &self,
        pos_within_output: Point<f64, Logical>,
    ) -> Option<&Workspace<W>> {
        self.workspaces_with_render_geo()
            .find_map(|(ws, geo)| geo.contains(pos_within_output).then_some(ws))
    }

    pub fn window_under(&self, pos_within_output: Point<f64, Logical>) -> Option<(&W, HitType)> {
        let (ws, geo) = self.workspace_under(pos_within_output)?;

        if self.overview_progress.is_some() {
            let zoom = self.overview_zoom();
            let pos_within_workspace = (pos_within_output - geo.loc).downscale(zoom);
            let (win, hit) = ws.window_under(pos_within_workspace)?;
            // During the overview animation, we cannot do input hits because we cannot really
            // represent scaled windows properly.
            Some((win, hit.to_activate()))
        } else {
            let (win, hit) = ws.window_under(pos_within_output - geo.loc)?;
            Some((win, hit.offset_win_pos(geo.loc)))
        }
    }

    pub fn resize_edges_under(&self, pos_within_output: Point<f64, Logical>) -> Option<ResizeEdge> {
        if self.overview_progress.is_some() {
            return None;
        }

        let (ws, geo) = self.workspace_under(pos_within_output)?;
        ws.resize_edges_under(pos_within_output - geo.loc)
    }

    pub(super) fn insert_position(
        &self,
        pos_within_output: Point<f64, Logical>,
    ) -> (InsertWorkspace, Rectangle<f64, Logical>) {
        let mut iter = self.workspaces_with_render_geo_idx();

        // Monitors always have at least one workspace.
        let ((idx, ws), geo) = iter.next().unwrap();

        // Check if above first.
        if pos_within_output.y < geo.loc.y {
            return (
                InsertWorkspace::Preview(WorkspacePreview {
                    insertion_index: idx,
                    geometry: geo,
                }),
                geo,
            );
        }

        let contains = move |geo: Rectangle<f64, Logical>| {
            geo.loc.y <= pos_within_output.y && pos_within_output.y < geo.loc.y + geo.size.h
        };

        // Check first.
        if contains(geo) {
            return (InsertWorkspace::Existing(ws.id()), geo);
        }

        let mut last_geo = geo;
        let mut last_idx = idx;
        for ((idx, ws), geo) in iter {
            // Check gap above.
            let gap_loc = Point::from((last_geo.loc.x, last_geo.loc.y + last_geo.size.h));
            let gap_size = Size::from((geo.size.w, geo.loc.y - gap_loc.y));
            let gap_geo = Rectangle::new(gap_loc, gap_size);
            if contains(gap_geo) {
                return (
                    InsertWorkspace::Preview(WorkspacePreview {
                        insertion_index: idx,
                        geometry: geo,
                    }),
                    geo,
                );
            }

            // Check workspace itself.
            if contains(geo) {
                return (InsertWorkspace::Existing(ws.id()), geo);
            }

            last_geo = geo;
            last_idx = idx;
        }

        // Anything below previews another workspace without adding it to the collection.
        let mut preview_geo = last_geo;
        preview_geo.loc.y += preview_geo.size.h + self.workspace_gap(self.overview_zoom());
        (
            InsertWorkspace::Preview(WorkspacePreview {
                insertion_index: last_idx + 1,
                geometry: preview_geo,
            }),
            preview_geo,
        )
    }

    pub fn render_above_top_layer(&self) -> bool {
        // Render above the top layer only if the view is stationary.
        if self.workspace_switch.is_some() || self.overview_progress.is_some() {
            return false;
        }

        let ws = &self.workspaces[self.active_workspace_idx];
        ws.render_above_top_layer()
    }

    pub fn render_insert_hint_between_workspaces<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        push: &mut dyn FnMut(MonitorRenderElement<R>),
    ) {
        if self.options.layout.insert_hint.off {
            return;
        }
        let Some(render_loc) = self.insert_hint_render_loc else {
            return;
        };
        let InsertWorkspace::Preview(_) = render_loc.workspace else {
            return;
        };

        self.insert_hint_element
            .render(renderer, render_loc.location, &mut |elem| {
                let elem = MonitorInnerRenderElement::UncroppedInsertHint(elem);
                let elem = RescaleRenderElement::from_element(elem, Point::default(), 1.);
                let elem =
                    RelocateRenderElement::from_element(elem, Point::default(), Relocate::Relative);
                push(elem);
            });
    }

    pub fn render_workspaces<R: NiriRenderer>(
        &self,
        mut ctx: RenderCtx<R>,
        focus_ring: bool,
        push: &mut dyn FnMut(MonitorRenderElement<R>),
    ) {
        let _span = tracy_client::span!("Monitor::render_workspaces");

        let scale = self.scale.fractional_scale();
        // Ceil the height in physical pixels.
        let height = (self.view_size.h * scale).ceil() as i32;

        let zoom = self.overview_zoom();

        let insert_hint_render_loc = self
            .insert_hint_render_loc
            .filter(|_| !self.options.layout.insert_hint.off);

        let scale_relocate = move |geo: Rectangle<f64, Logical>, elem| {
            let elem = RescaleRenderElement::from_element(elem, Point::from((0, 0)), zoom);
            RelocateRenderElement::from_element(
                elem,
                // The offset we get from workspaces_with_render_geo() is already
                // rounded to physical pixels, but it's in the logical coordinate
                // space, so we need to convert it to physical.
                geo.loc.to_physical_precise_round(scale),
                Relocate::Relative,
            )
        };

        // Draw in passes for correct Z ordering during window movement between workspaces:
        // - floating windows moving between workspaces
        // - normal floating windows
        // - scrolling windows moving between workspaces
        // - normal scrolling windows
        for pass in 0..4 {
            // Don't cull when drawing windows moving between workspaces so that windows moving to
            // workspaces off-screen will still render.
            let cull = matches!(pass, 1 | 3);

            // Crop the elements to prevent them overflowing, currently visible during a workspace
            // switch.
            //
            // HACK: crop to infinite bounds at least horizontally where we
            // know there's no workspace joining or monitor bounds, otherwise
            // it will cut pixel shaders and mess up the coordinate space.
            // There's also a damage tracking bug which causes glitched
            // rendering for maximized GTK windows.
            //
            // FIXME: use proper bounds after fixing the Crop element.
            //
            // Also, check cull here to avoid cropping windows moving between workspaces.
            //
            // FIXME: for cull=true, it might be better visually to crop to a workspace-high region
            // anchored to the window/column as it moves between workspaces, to prevent overflowing
            // windows from appearing and disappearing.
            let crop_bounds =
                if cull && (self.workspace_switch.is_some() || self.overview_progress.is_some()) {
                    Rectangle::new(
                        Point::from((-i32::MAX / 2, 0)),
                        Size::from((i32::MAX, height)),
                    )
                } else {
                    Rectangle::new(
                        Point::from((-i32::MAX / 2, -i32::MAX / 2)),
                        Size::from((i32::MAX, i32::MAX)),
                    )
                };

            for (ws, geo) in self.workspaces_with_render_geo_cull(cull) {
                // Macro instead of closure because ws and insert hint have different elem types.
                macro_rules! push {
                    () => {{
                        &mut |elem| {
                            let elem = CropRenderElement::from_element(elem, scale, crop_bounds);
                            if let Some(elem) = elem {
                                let elem = MonitorInnerRenderElement::from(elem);
                                push(scale_relocate(geo, elem));
                            }
                        }
                    }};
                }

                let xray_pos = XrayPos::new(geo.loc, zoom);

                match pass {
                    0 => {
                        ws.render_floating(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::MovingBetweenWorkspaces,
                            push!(),
                        );
                    }
                    1 => {
                        ws.render_floating(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::Normal,
                            push!(),
                        );

                        if let Some(loc) = insert_hint_render_loc {
                            if loc.workspace == InsertWorkspace::Existing(ws.id()) {
                                self.insert_hint_element.render(
                                    ctx.renderer,
                                    loc.location,
                                    push!(),
                                );
                            }
                        }
                    }
                    2 => {
                        ws.render_scrolling(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::MovingBetweenWorkspaces,
                            push!(),
                        );
                    }
                    _ => {
                        ws.render_scrolling(
                            ctx.r(),
                            xray_pos,
                            focus_ring,
                            RenderLayer::Normal,
                            push!(),
                        );
                    }
                }
            }
        }
    }

    pub fn render_workspace_shadows<R: NiriRenderer>(
        &self,
        renderer: &mut R,
        push: &mut dyn FnMut(MonitorRenderElement<R>),
    ) {
        let Some(progress) = self.overview_progress.as_ref().map(|p| p.clamped_value()) else {
            return;
        };
        let alpha = progress.clamp(0., 1.) as f32;

        let _span = tracy_client::span!("Monitor::render_workspace_shadows");

        let scale = self.scale.fractional_scale();
        let zoom = self.overview_zoom();

        for (ws, geo) in self.workspaces_with_render_geo() {
            ws.render_shadow(renderer, &mut |elem| {
                let elem = elem.with_alpha(alpha);
                let elem = MonitorInnerRenderElement::Shadow(elem);
                let elem = RescaleRenderElement::from_element(elem, Point::from((0, 0)), zoom);
                let elem = RelocateRenderElement::from_element(
                    elem,
                    geo.loc.to_physical_precise_round(scale),
                    Relocate::Relative,
                );
                push(elem);
            });
        }
    }

    pub fn dnd_scroll_gesture_begin(&mut self) {
        if let Some(WorkspaceSwitch::DndScroll(DndScrollGesture {
            dnd_last_event_time: Some(_),
            ..
        })) = &self.workspace_switch
        {
            // Already active.
            return;
        }

        if !self.overview_open {
            // This gesture is only for the overview.
            return;
        }

        let center_idx = self.active_workspace_idx;
        let current_idx = self.workspace_render_idx();

        let gesture = DndScrollGesture {
            center_idx,
            start_idx: current_idx,
            current_idx,
            animation: None,
            tracker: SwipeTracker::new(),
            is_clamped: false,
            dnd_last_event_time: Some(self.clock.now_unadjusted()),
            dnd_nonzero_start_time: None,
        };
        self.workspace_switch = Some(WorkspaceSwitch::DndScroll(gesture));
    }

    pub fn dnd_scroll_gesture_scroll(&mut self, pos: Point<f64, Logical>, speed: f64) -> bool {
        let zoom = self.overview_zoom();

        let Some(WorkspaceSwitch::DndScroll(gesture)) = &mut self.workspace_switch else {
            return false;
        };

        let Some(last_time) = gesture.dnd_last_event_time else {
            // Not a DnD scroll.
            return false;
        };

        let config = &self.options.gestures.dnd_edge_workspace_switch;
        let trigger_height = config.trigger_height;

        // Restrict the scrolling horizontally to the strip of workspaces to avoid unwanted trigger
        // after using the hot corner or during horizontal scroll.
        let width = self.view_size.w * zoom;
        let x = pos.x - (self.view_size.w - width) / 2.;

        // Consider the working area so layer-shell docks and such don't prevent scrolling.
        let y = pos.y - self.working_area.loc.y;
        let height = self.working_area.size.h;

        let y = y.clamp(0., height);
        let trigger_height = trigger_height.clamp(0., height / 2.);

        let delta = if x < 0. || width <= x {
            // Outside the bounds horizontally.
            0.
        } else if y < trigger_height {
            -(trigger_height - y)
        } else if height - y < trigger_height {
            trigger_height - (height - y)
        } else {
            0.
        };

        let delta = if trigger_height < 0.01 {
            // Sanity check for trigger-height 0 or small window sizes.
            0.
        } else {
            // Normalize to [0, 1].
            delta / trigger_height
        };
        let delta = delta * speed;

        let now = self.clock.now_unadjusted();
        gesture.dnd_last_event_time = Some(now);

        if delta == 0. {
            // We're outside the scrolling zone.
            gesture.dnd_nonzero_start_time = None;
            return false;
        }

        let nonzero_start = *gesture.dnd_nonzero_start_time.get_or_insert(now);

        // Delay starting the gesture a bit to avoid unwanted movement when dragging across
        // monitors.
        let delay = Duration::from_millis(u64::from(config.delay_ms));
        if now.saturating_sub(nonzero_start) < delay {
            return true;
        }

        let time_delta = now.saturating_sub(last_time).as_secs_f64();

        let delta = delta * time_delta * config.max_speed;

        gesture.tracker.push(delta, now);

        let total_height = WORKSPACE_DND_EDGE_SCROLL_MOVEMENT;
        let pos = gesture.tracker.pos() / total_height;
        let unclamped = gesture.start_idx + pos;

        let (min, max) = gesture.min_max(self.workspaces.len());
        let clamped = unclamped.clamp(min, max);

        // Make sure that DnD scrolling too much outside the min/max does not "build up".
        gesture.start_idx += clamped - unclamped;
        gesture.current_idx = clamped;

        true
    }

    pub fn dnd_scroll_gesture_end(&mut self) {
        if !matches!(
            self.workspace_switch,
            Some(WorkspaceSwitch::DndScroll(DndScrollGesture {
                dnd_last_event_time: Some(_),
                ..
            }))
        ) {
            // Not a DnD scroll.
            return;
        };

        let Some(WorkspaceSwitch::DndScroll(gesture)) = &mut self.workspace_switch else {
            return;
        };
        let new_idx = gesture.current_idx.round() as usize;
        self.active_workspace_idx = new_idx;
        self.workspace_switch = Some(WorkspaceSwitch::Animation(Animation::new(
            self.clock.clone(),
            new_idx as f64,
            new_idx as f64,
            0.,
            self.options.animations.workspace_switch.0,
        )));
    }

    pub fn scale(&self) -> smithay::output::Scale {
        self.scale
    }

    pub fn view_size(&self) -> Size<f64, Logical> {
        self.view_size
    }

    pub fn working_area(&self) -> Rectangle<f64, Logical> {
        self.working_area
    }

    pub fn layout_config(&self) -> Option<&swayward_config::LayoutPart> {
        self.layout_config.as_ref()
    }

    #[cfg(test)]
    pub(super) fn verify_invariants(&self, detached_move_source: Option<WorkspaceId>) {
        use approx::assert_abs_diff_eq;

        let options =
            Options::clone(&self.base_options).with_merged_layout(self.layout_config.as_ref());
        assert_eq!(&*self.options, &options);

        assert!(
            !self.workspaces.is_empty(),
            "monitor must have at least one workspace"
        );
        assert!(self.active_workspace_idx < self.workspaces.len());

        if let Some(WorkspaceSwitch::Animation(anim)) = &self.workspace_switch {
            let before_idx = anim.from() as usize;
            let after_idx = anim.to() as usize;

            assert!(before_idx < self.workspaces.len());
            assert!(after_idx < self.workspaces.len());
        }

        // Sway has no trailing placeholder workspace. Workspaces exist only
        // when named, numbered, or holding windows; niri's always-empty last
        // workspace is an affordance of its scrolling strip, which swayward
        // replaced with i3's tree.

        // If there's no workspace switch in progress, no inactive workspace may
        // be both empty and unaddressable.
        if self.workspace_switch.is_none() {
            for (idx, ws) in self.workspaces.iter().enumerate() {
                if idx != self.active_workspace_idx {
                    // Sway destroys an empty workspace once focus leaves it,
                    // so an inactive workspace must be addressable or hold
                    // windows. Unlike niri there is no exemption for the last
                    // one: there is no trailing placeholder to exempt.
                    assert!(
                        ws.has_windows()
                            || ws.has_sway_identity()
                            || Some(ws.id()) == detached_move_source,
                        "inactive workspace must hold windows or be addressable"
                    );
                }
            }
        }

        for workspace in &self.workspaces {
            assert_eq!(self.clock, workspace.clock);

            assert_eq!(
                self.scale().integer_scale(),
                workspace.scale().integer_scale()
            );
            assert_eq!(
                self.scale().fractional_scale(),
                workspace.scale().fractional_scale()
            );
            assert_eq!(self.view_size, workspace.view_size());

            assert_eq!(
                workspace.base_options, self.options,
                "workspace options must be synchronized with monitor"
            );
        }

        let scale = self.scale().fractional_scale();
        let iter = self.workspaces_with_render_geo();
        for (_ws, ws_geo) in iter {
            let pos = ws_geo.loc;
            let rounded_pos = pos.to_physical_precise_round(scale).to_logical(scale);

            // Workspace positions must be rounded to physical pixels.
            assert_abs_diff_eq!(pos.x, rounded_pos.x, epsilon = 1e-5);
            assert_abs_diff_eq!(pos.y, rounded_pos.y, epsilon = 1e-5);
        }
    }
}

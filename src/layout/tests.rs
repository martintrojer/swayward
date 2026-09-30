use std::cell::{Cell, OnceCell, RefCell};

use proptest::prelude::*;
use proptest_derive::Arbitrary;
use smithay::output::{Mode, PhysicalProperties, Subpixel};
use smithay::utils::Rectangle;
use swayward_config::utils::Flag;
use swayward_config::workspace::WorkspaceName;
use swayward_config::{
    FloatOrInt, OutputName, Struts, TabIndicatorLength, TabIndicatorPosition, WorkspaceReference,
};

use super::tiling_tree::IpcNode;
use super::*;

mod fullscreen;

impl<W: LayoutElement> Default for Layout<W> {
    fn default() -> Self {
        Self::with_options(Clock::with_time(Duration::ZERO), Default::default())
    }
}

#[derive(Debug)]
struct TestWindowInner {
    id: usize,
    parent_id: Cell<Option<usize>>,
    bbox: Cell<Rectangle<i32, Logical>>,
    initial_bbox: Rectangle<i32, Logical>,
    requested_size: Cell<Option<Size<i32, Logical>>>,
    // Emulates the window ignoring the compositor-provided size.
    forced_size: Cell<Option<Size<i32, Logical>>>,
    min_size: Size<i32, Logical>,
    max_size: Size<i32, Logical>,
    pending_sizing_mode: Cell<SizingMode>,
    pending_activated: Cell<bool>,
    sizing_mode: Cell<SizingMode>,
    is_windowed_fullscreen: Cell<bool>,
    is_pending_windowed_fullscreen: Cell<bool>,
    animate_next_configure: Cell<bool>,
    animation_snapshot: RefCell<Option<LayoutElementRenderSnapshot>>,
    rules: ResolvedWindowRules,
    focus_timestamp: Cell<Option<Duration>>,
}

#[derive(Debug, Clone)]
struct TestWindow(Rc<TestWindowInner>);

#[derive(Debug, Clone, Arbitrary)]
struct TestWindowParams {
    #[proptest(strategy = "1..=5usize")]
    id: usize,
    #[proptest(strategy = "arbitrary_parent_id()")]
    parent_id: Option<usize>,
    is_floating: bool,
    #[proptest(strategy = "arbitrary_bbox()")]
    bbox: Rectangle<i32, Logical>,
    #[proptest(strategy = "arbitrary_min_max_size()")]
    min_max_size: (Size<i32, Logical>, Size<i32, Logical>),
    #[proptest(strategy = "prop::option::of(arbitrary_rules())")]
    rules: Option<ResolvedWindowRules>,
}

impl TestWindowParams {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            parent_id: None,
            is_floating: false,
            bbox: Rectangle::from_size(Size::from((100, 200))),
            min_max_size: Default::default(),
            rules: None,
        }
    }
}

impl TestWindow {
    fn new(params: TestWindowParams) -> Self {
        Self(Rc::new(TestWindowInner {
            id: params.id,
            parent_id: Cell::new(params.parent_id),
            bbox: Cell::new(params.bbox),
            initial_bbox: params.bbox,
            requested_size: Cell::new(None),
            forced_size: Cell::new(None),
            min_size: params.min_max_size.0,
            max_size: params.min_max_size.1,
            pending_sizing_mode: Cell::new(SizingMode::Normal),
            pending_activated: Cell::new(false),
            sizing_mode: Cell::new(SizingMode::Normal),
            is_windowed_fullscreen: Cell::new(false),
            is_pending_windowed_fullscreen: Cell::new(false),
            animate_next_configure: Cell::new(false),
            animation_snapshot: RefCell::new(None),
            rules: params.rules.unwrap_or_default(),
            focus_timestamp: Cell::new(None),
        }))
    }

    fn communicate(&self) -> bool {
        let mut changed = false;

        let size = self.0.forced_size.get().or(self.0.requested_size.get());
        if let Some(size) = size {
            assert!(size.w >= 0);
            assert!(size.h >= 0);

            let mut new_bbox = self.0.initial_bbox;
            if size.w != 0 {
                new_bbox.size.w = size.w;
            }
            if size.h != 0 {
                new_bbox.size.h = size.h;
            }

            if self.0.bbox.get() != new_bbox {
                if self.0.animate_next_configure.get() {
                    self.0.animation_snapshot.replace(Some(RenderSnapshot {
                        contents: Vec::new(),
                        contents_with_blocked_out_bg: None,
                        blocked_out_contents: Vec::new(),
                        block_out_from: None,
                        size: self.0.bbox.get().size.to_f64(),
                        texture: OnceCell::new(),
                        texture_with_blocked_out_bg: Default::default(),
                        blocked_out_texture: OnceCell::new(),
                    }));
                }

                self.0.bbox.set(new_bbox);
                changed = true;
            }
        }

        self.0.animate_next_configure.set(false);

        if self.0.sizing_mode.get() != self.0.pending_sizing_mode.get() {
            self.0.sizing_mode.set(self.0.pending_sizing_mode.get());
            changed = true;
        }

        if self.0.is_windowed_fullscreen.get() != self.0.is_pending_windowed_fullscreen.get() {
            self.0
                .is_windowed_fullscreen
                .set(self.0.is_pending_windowed_fullscreen.get());
            changed = true;
        }

        changed
    }
}

impl LayoutElement for TestWindow {
    type Id = usize;

    fn id(&self) -> &Self::Id {
        &self.0.id
    }

    fn focus_timestamp(&self) -> Option<Duration> {
        self.0.focus_timestamp.get()
    }

    fn size(&self) -> Size<i32, Logical> {
        self.0.bbox.get().size
    }

    fn buf_loc(&self) -> Point<i32, Logical> {
        (0, 0).into()
    }

    fn is_in_input_region(&self, _point: Point<f64, Logical>) -> bool {
        false
    }

    fn request_size(
        &mut self,
        size: Size<i32, Logical>,
        mode: SizingMode,
        _animate: bool,
        _transaction: Option<Transaction>,
    ) {
        if self.0.requested_size.get() != Some(size) {
            self.0.requested_size.set(Some(size));
            self.0.animate_next_configure.set(true);
        }

        self.0.pending_sizing_mode.set(mode);

        if mode.is_fullscreen() {
            self.0.is_pending_windowed_fullscreen.set(false);
        }
    }

    fn min_size(&self) -> Size<i32, Logical> {
        self.0.min_size
    }

    fn max_size(&self) -> Size<i32, Logical> {
        self.0.max_size
    }

    fn is_wl_surface(&self, _wl_surface: &WlSurface) -> bool {
        false
    }

    fn set_preferred_scale_transform(&self, _scale: output::Scale, _transform: Transform) {}

    fn has_ssd(&self) -> bool {
        false
    }

    fn output_enter(&self, _output: &Output) {}

    fn output_leave(&self, _output: &Output) {}

    fn set_offscreen_data(&self, _data: Option<OffscreenData>) {}

    fn set_activated(&mut self, active: bool) {
        self.0.pending_activated.set(active);
    }

    fn set_bounds(&self, _bounds: Size<i32, Logical>) {}

    fn is_ignoring_opacity_window_rule(&self) -> bool {
        false
    }

    fn configure_intent(&self) -> ConfigureIntent {
        ConfigureIntent::CanSend
    }

    fn send_pending_configure(&mut self) {}

    fn set_active_in_column(&mut self, _active: bool) {}

    fn set_floating(&mut self, _floating: bool) {}

    fn sizing_mode(&self) -> SizingMode {
        self.0.sizing_mode.get()
    }

    fn pending_sizing_mode(&self) -> SizingMode {
        self.0.pending_sizing_mode.get()
    }

    fn requested_size(&self) -> Option<Size<i32, Logical>> {
        self.0.requested_size.get()
    }

    fn is_windowed_fullscreen(&self) -> bool {
        self.0.is_windowed_fullscreen.get()
    }

    fn is_pending_windowed_fullscreen(&self) -> bool {
        self.0.is_pending_windowed_fullscreen.get()
    }

    fn request_windowed_fullscreen(&mut self, value: bool) {
        self.0.is_pending_windowed_fullscreen.set(value);
    }

    fn is_child_of(&self, parent: &Self) -> bool {
        self.0.parent_id.get() == Some(parent.0.id)
    }

    fn refresh(&self) {}

    fn rules(&self) -> &ResolvedWindowRules {
        &self.0.rules
    }

    fn take_animation_snapshot(&mut self) -> Option<LayoutElementRenderSnapshot> {
        self.0.animation_snapshot.take()
    }

    fn set_interactive_resize(&mut self, _data: Option<InteractiveResizeData>) {}

    fn cancel_interactive_resize(&mut self) {}

    fn on_commit(&mut self, _serial: Serial) {}

    fn interactive_resize_data(&self) -> Option<InteractiveResizeData> {
        None
    }

    fn is_urgent(&self) -> bool {
        false
    }
}

fn arbitrary_size() -> impl Strategy<Value = Size<i32, Logical>> {
    any::<(u16, u16)>().prop_map(|(w, h)| Size::from((w.max(1).into(), h.max(1).into())))
}

fn arbitrary_bbox() -> impl Strategy<Value = Rectangle<i32, Logical>> {
    any::<(i16, i16, u16, u16)>().prop_map(|(x, y, w, h)| {
        let loc: Point<i32, _> = Point::from((x.into(), y.into()));
        let size: Size<i32, _> = Size::from((w.max(1).into(), h.max(1).into()));
        Rectangle::new(loc, size)
    })
}

fn arbitrary_size_change() -> impl Strategy<Value = SizeChange> {
    prop_oneof![
        (0..).prop_map(SizeChange::SetFixed),
        (0f64..).prop_map(SizeChange::SetProportion),
        any::<i32>().prop_map(SizeChange::AdjustFixed),
        any::<f64>().prop_map(SizeChange::AdjustProportion),
        // Interactive resize can have negative values here.
        Just(SizeChange::SetFixed(-100)),
    ]
}

fn arbitrary_position_change() -> impl Strategy<Value = PositionChange> {
    // Sway parses fixed movement amounts with strtol into an int
    // (`sway/common/util.c:80-102`), and swayward's command parser narrows them
    // to i32 before constructing PositionChange. Generating arbitrary f64
    // fixed positions exercises states no command can create; values near
    // f64::MAX overflow to infinity when multiplied by output scale. Position
    // proportions must also be finite because non-finite inputs are no-ops.
    let proportion =
        any::<f64>().prop_filter("proportion must be finite", |value| value.is_finite());
    prop_oneof![
        any::<i32>().prop_map(|value| PositionChange::SetFixed(f64::from(value))),
        proportion.clone().prop_map(PositionChange::SetProportion),
        any::<i32>().prop_map(|value| PositionChange::AdjustFixed(f64::from(value))),
        proportion.prop_map(PositionChange::AdjustProportion),
    ]
}

fn arbitrary_min_max() -> impl Strategy<Value = (i32, i32)> {
    prop_oneof![
        Just((0, 0)),
        (1..65536).prop_map(|n| (n, n)),
        (1..65536).prop_map(|min| (min, 0)),
        (1..).prop_map(|max| (0, max)),
        (1..65536, 1..).prop_map(|(min, max): (i32, i32)| (min, max.max(min))),
    ]
}

fn arbitrary_min_max_size() -> impl Strategy<Value = (Size<i32, Logical>, Size<i32, Logical>)> {
    prop_oneof![
        5 => (arbitrary_min_max(), arbitrary_min_max()).prop_map(
            |((min_w, max_w), (min_h, max_h))| {
                let min_size = Size::from((min_w, min_h));
                let max_size = Size::from((max_w, max_h));
                (min_size, max_size)
            },
        ),
        1 => arbitrary_min_max().prop_map(|(w, h)| {
            let size = Size::from((w, h));
            (size, size)
        }),
    ]
}

prop_compose! {
    fn arbitrary_rules()(
        focus_ring in arbitrary_focus_ring(),
        border in arbitrary_border(),
    ) -> ResolvedWindowRules {
        ResolvedWindowRules {
            focus_ring,
            border,
            ..ResolvedWindowRules::default()
        }
    }
}

fn arbitrary_view_offset_gesture_delta() -> impl Strategy<Value = f64> {
    prop_oneof![(-10f64..10f64), (-50000f64..50000f64),]
}

fn arbitrary_resize_edge() -> impl Strategy<Value = ResizeEdge> {
    prop_oneof![
        Just(ResizeEdge::RIGHT),
        Just(ResizeEdge::BOTTOM),
        Just(ResizeEdge::LEFT),
        Just(ResizeEdge::TOP),
        Just(ResizeEdge::BOTTOM_RIGHT),
        Just(ResizeEdge::BOTTOM_LEFT),
        Just(ResizeEdge::TOP_RIGHT),
        Just(ResizeEdge::TOP_LEFT),
        Just(ResizeEdge::empty()),
    ]
}

fn arbitrary_scale() -> impl Strategy<Value = f64> {
    prop_oneof![Just(1.), Just(1.5), Just(2.),]
}

fn arbitrary_msec_delta() -> impl Strategy<Value = i32> {
    prop_oneof![
        1 => Just(-1000),
        2 => Just(-10),
        1 => Just(0),
        2 => Just(10),
        6 => Just(1000),
    ]
}

fn arbitrary_parent_id() -> impl Strategy<Value = Option<usize>> {
    prop_oneof![
        5 => Just(None),
        1 => prop::option::of(1..=5usize),
    ]
}

fn arbitrary_tiling_display() -> impl Strategy<Value = ColumnDisplay> {
    prop_oneof![Just(ColumnDisplay::Normal), Just(ColumnDisplay::Tabbed)]
}

fn arbitrary_default_orientation() -> impl Strategy<Value = swayward_config::DefaultOrientation> {
    prop_oneof![
        Just(swayward_config::DefaultOrientation::Horizontal),
        Just(swayward_config::DefaultOrientation::Vertical),
        Just(swayward_config::DefaultOrientation::Auto),
    ]
}

fn arbitrary_tree_layout() -> impl Strategy<Value = tiling_tree::Layout> {
    prop_oneof![
        Just(tiling_tree::Layout::SplitH),
        Just(tiling_tree::Layout::SplitV),
        Just(tiling_tree::Layout::Tabbed),
        Just(tiling_tree::Layout::Stacked),
    ]
}

fn arbitrary_tree_direction() -> impl Strategy<Value = tiling_tree::Direction> {
    prop_oneof![
        Just(tiling_tree::Direction::Left),
        Just(tiling_tree::Direction::Right),
        Just(tiling_tree::Direction::Up),
        Just(tiling_tree::Direction::Down),
    ]
}

mod operations;

use operations::Op;

#[track_caller]
fn check_ops_on_layout(layout: &mut Layout<TestWindow>, ops: impl IntoIterator<Item = Op>) {
    for op in ops {
        op.apply(layout);
        layout.verify_invariants();
    }
}

#[track_caller]
fn check_ops(ops: impl IntoIterator<Item = Op>) -> Layout<TestWindow> {
    let mut layout = Layout::default();
    check_ops_on_layout(&mut layout, ops);
    layout
}

#[track_caller]
fn check_ops_with_options(
    options: Options,
    ops: impl IntoIterator<Item = Op>,
) -> Layout<TestWindow> {
    let mut layout = Layout::with_options(Clock::with_time(Duration::ZERO), options);
    check_ops_on_layout(&mut layout, ops);
    layout
}

#[test]
fn operations_dont_panic() {
    if std::env::var_os("RUN_SLOW_TESTS").is_none() {
        eprintln!("ignoring slow test");
        return;
    }

    let every_op = [
        Op::AddOutput(0),
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::RemoveOutput(0),
        Op::RemoveOutput(1),
        Op::RemoveOutput(2),
        Op::FocusOutput(0),
        Op::FocusOutput(1),
        Op::FocusOutput(2),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: Some(1),
            layout_config: None,
        },
        Op::UnnameWorkspace { ws_name: 1 },
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(2),
            next_to_id: 1,
        },
        Op::AddWindowToNamedWorkspace {
            params: TestWindowParams::new(3),
            ws_name: 1,
        },
        Op::CloseWindow(0),
        Op::CloseWindow(1),
        Op::CloseWindow(2),
        Op::FullscreenWindow(1),
        Op::FullscreenWindow(2),
        Op::FullscreenWindow(3),
        Op::MaximizeWindowToEdges { id: Some(1) },
        Op::MaximizeWindowToEdges { id: Some(2) },
        Op::MaximizeWindowToEdges { id: Some(3) },
        Op::FocusLeft,
        Op::FocusRight,
        Op::FocusRightOrFirstRootChild,
        Op::FocusLeftOrLastRootChild,
        Op::FocusWindowOrMonitorUp(0),
        Op::FocusWindowOrMonitorDown(1),
        Op::FocusLeftOrMonitorLeft(0),
        Op::FocusRightOrMonitorRight(1),
        Op::FocusWindowUp,
        Op::FocusUpOrLeft,
        Op::FocusUpOrRight,
        Op::FocusWindowOrWorkspaceUp,
        Op::FocusWindowDown,
        Op::FocusDownOrLeft,
        Op::FocusDownOrRight,
        Op::FocusWindowOrWorkspaceDown,
        Op::MoveLeft,
        Op::MoveRight,
        Op::MoveLeftOrToMonitorLeft(0),
        Op::MoveRightOrToMonitorRight(1),
        Op::NestFocusedWindow,
        Op::UnnestFocusedWindow,
        Op::FocusWorkspaceDown,
        Op::FocusWorkspaceUp,
        Op::FocusWorkspace(1),
        Op::FocusWorkspace(2),
        Op::MoveWindowToWorkspaceDown(true),
        Op::MoveWindowToWorkspaceUp(true),
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 1,
        },
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 2,
        },
        Op::MoveFocusedToWorkspaceDown(true),
        Op::MoveFocusedToWorkspaceUp(true),
        Op::MoveFocusedToWorkspace(1, true),
        Op::MoveFocusedToWorkspace(2, true),
        Op::MoveWindowDown,
        Op::MoveWindowDownOrToWorkspaceDown,
        Op::MoveWindowUp,
        Op::MoveWindowUpOrToWorkspaceUp,
        Op::MoveWindowInDirection(tiling_tree::Direction::Left),
        Op::MoveWindowInDirection(tiling_tree::Direction::Right),
        Op::MoveWindowInDirection(tiling_tree::Direction::Up),
        Op::MoveWindowInDirection(tiling_tree::Direction::Down),
        Op::SplitFocused(tiling_tree::Layout::SplitH),
        Op::SplitFocused(tiling_tree::Layout::SplitV),
        Op::SetFocusedLayout(tiling_tree::Layout::Tabbed),
        Op::SetFocusedLayout(tiling_tree::Layout::Stacked),
        Op::FocusParent,
        Op::FocusChild,
        Op::NestOrUnnestWindowLeft { id: None },
        Op::NestOrUnnestWindowRight { id: None },
        Op::MoveWorkspaceToOutput(1),
        Op::ToggleFocusedTabbedDisplay,
    ];

    for third in &every_op {
        for second in &every_op {
            for first in &every_op {
                // eprintln!("{first:?}, {second:?}, {third:?}");

                let mut layout = Layout::default();
                first.clone().apply(&mut layout);
                layout.verify_invariants();
                second.clone().apply(&mut layout);
                layout.verify_invariants();
                third.clone().apply(&mut layout);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn operations_from_starting_state_dont_panic() {
    if std::env::var_os("RUN_SLOW_TESTS").is_none() {
        eprintln!("ignoring slow test");
        return;
    }

    // Running every op from an empty state doesn't get us to all the interesting states. So,
    // also run it from a manually-created starting state with more things going on to exercise
    // more code paths.
    let setup_ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowToWorkspaceDown(true),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::FocusLeft,
        Op::NestFocusedWindow,
        Op::AddWindow {
            params: TestWindowParams::new(4),
        },
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(5),
        },
        Op::MoveWindowToOutput {
            window_id: None,
            output_id: 2,
            target_ws_idx: None,
        },
        Op::FocusOutput(1),
        Op::Communicate(1),
        Op::Communicate(2),
        Op::Communicate(3),
        Op::Communicate(4),
        Op::Communicate(5),
    ];

    let every_op = [
        Op::AddOutput(0),
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::RemoveOutput(0),
        Op::RemoveOutput(1),
        Op::RemoveOutput(2),
        Op::FocusOutput(0),
        Op::FocusOutput(1),
        Op::FocusOutput(2),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: Some(1),
            layout_config: None,
        },
        Op::UnnameWorkspace { ws_name: 1 },
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(6),
            next_to_id: 0,
        },
        Op::AddWindowNextTo {
            params: TestWindowParams::new(7),
            next_to_id: 1,
        },
        Op::AddWindowToNamedWorkspace {
            params: TestWindowParams::new(5),
            ws_name: 1,
        },
        Op::CloseWindow(0),
        Op::CloseWindow(1),
        Op::CloseWindow(2),
        Op::FullscreenWindow(1),
        Op::FullscreenWindow(2),
        Op::FullscreenWindow(3),
        Op::MaximizeWindowToEdges { id: Some(1) },
        Op::MaximizeWindowToEdges { id: Some(2) },
        Op::MaximizeWindowToEdges { id: Some(3) },
        Op::SetFullscreenWindow {
            window: 1,
            is_fullscreen: false,
        },
        Op::SetFullscreenWindow {
            window: 1,
            is_fullscreen: true,
        },
        Op::SetFullscreenWindow {
            window: 2,
            is_fullscreen: false,
        },
        Op::SetFullscreenWindow {
            window: 2,
            is_fullscreen: true,
        },
        Op::FocusLeft,
        Op::FocusRight,
        Op::FocusRightOrFirstRootChild,
        Op::FocusLeftOrLastRootChild,
        Op::FocusWindowOrMonitorUp(0),
        Op::FocusWindowOrMonitorDown(1),
        Op::FocusLeftOrMonitorLeft(0),
        Op::FocusRightOrMonitorRight(1),
        Op::FocusWindowUp,
        Op::FocusUpOrLeft,
        Op::FocusUpOrRight,
        Op::FocusWindowOrWorkspaceUp,
        Op::FocusWindowDown,
        Op::FocusDownOrLeft,
        Op::FocusDownOrRight,
        Op::FocusWindowOrWorkspaceDown,
        Op::MoveLeft,
        Op::MoveRight,
        Op::MoveLeftOrToMonitorLeft(0),
        Op::MoveRightOrToMonitorRight(1),
        Op::NestFocusedWindow,
        Op::UnnestFocusedWindow,
        Op::FocusWorkspaceDown,
        Op::FocusWorkspaceUp,
        Op::FocusWorkspace(1),
        Op::FocusWorkspace(2),
        Op::FocusWorkspace(3),
        Op::MoveWindowToWorkspaceDown(true),
        Op::MoveWindowToWorkspaceUp(true),
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 1,
        },
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 2,
        },
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 3,
        },
        Op::MoveFocusedToWorkspaceDown(true),
        Op::MoveFocusedToWorkspaceUp(true),
        Op::MoveFocusedToWorkspace(1, true),
        Op::MoveFocusedToWorkspace(2, true),
        Op::MoveFocusedToWorkspace(3, true),
        Op::MoveWindowDown,
        Op::MoveWindowDownOrToWorkspaceDown,
        Op::MoveWindowUp,
        Op::MoveWindowUpOrToWorkspaceUp,
        Op::MoveWindowInDirection(tiling_tree::Direction::Left),
        Op::MoveWindowInDirection(tiling_tree::Direction::Right),
        Op::MoveWindowInDirection(tiling_tree::Direction::Up),
        Op::MoveWindowInDirection(tiling_tree::Direction::Down),
        Op::SplitFocused(tiling_tree::Layout::SplitH),
        Op::SplitFocused(tiling_tree::Layout::SplitV),
        Op::SetFocusedLayout(tiling_tree::Layout::Tabbed),
        Op::SetFocusedLayout(tiling_tree::Layout::Stacked),
        Op::FocusParent,
        Op::FocusChild,
        Op::NestOrUnnestWindowLeft { id: None },
        Op::NestOrUnnestWindowRight { id: None },
        Op::ToggleFocusedTabbedDisplay,
    ];

    for third in &every_op {
        for second in &every_op {
            for first in &every_op {
                // eprintln!("{first:?}, {second:?}, {third:?}");

                let mut layout = Layout::default();
                for op in &setup_ops {
                    op.clone().apply(&mut layout);
                }

                let mut layout = Layout::default();
                first.clone().apply(&mut layout);
                layout.verify_invariants();
                second.clone().apply(&mut layout);
                layout.verify_invariants();
                third.clone().apply(&mut layout);
                layout.verify_invariants();
            }
        }
    }
}

#[test]
fn primary_active_workspace_idx_not_updated_on_output_add() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::FocusOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(2),
        Op::FocusWorkspace(3),
        Op::AddOutput(2),
    ];

    check_ops(ops);
}

#[test]
fn window_closed_on_previous_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusWorkspaceDown,
        Op::CloseWindow(0),
    ];

    check_ops(ops);
}

#[test]
fn removing_active_output_focuses_its_evacuated_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    // Sway keeps focus on the evacuated non-empty workspace rather than the
    // surviving output's previously active empty workspace.
    assert_eq!(monitors[0].active_workspace_idx, 0);
}

#[test]
fn removing_active_output_reaps_the_empty_workspace_that_loses_focus() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(4),
        Op::MoveWorkspaceToMonitor {
            ws_name: None,
            output_id: 4,
        },
        Op::RemoveOutput(4),
    ]);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };
    assert_eq!(monitors[0].workspaces.len(), 1);
    assert!(monitors[0].active_workspace_ref().has_window(&1));
}

#[test]
fn move_down_creates_named_destination_before_moving_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);

    layout.move_to_workspace_down(true);

    let workspace = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .map(|(_, _, workspace)| workspace)
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn move_column_down_creates_named_destination_before_detaching() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);

    layout.move_focused_to_workspace_down(true);

    let workspace = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .map(|(_, _, workspace)| workspace)
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn workspace_focus_history_tracks_every_visited_workspace() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for name in ["93", "92", "94", "96", "foo"] {
        layout
            .activate_sway_workspace(crate::command::WorkspaceTarget::Name(name.into()))
            .unwrap();
    }

    let monitor = layout.active_monitor_ref().unwrap();
    let names = monitor
        .workspace_focus_history
        .iter()
        .map(|id| {
            monitor
                .workspaces
                .iter()
                .find(|workspace| workspace.id() == *id)
                .and_then(Workspace::sway_name)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["foo", "96", "94", "92", "93", "1"]);
}

#[test]
fn move_focused_to_output_names_destination_before_detaching() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let output = layout
        .outputs()
        .find(|output| output.name() == "output2")
        .unwrap()
        .clone();

    layout.move_focused_to_output(&output, None, true);

    let workspace = layout
        .monitor_for_output(&output)
        .unwrap()
        .workspaces
        .iter()
        .find(|workspace| workspace.has_window(&0))
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn sway_move_sorts_new_destination_before_moving_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("10".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);

    layout
        .move_window_to_sway_workspace(&0, crate::command::WorkspaceTarget::Name("2".into()), false)
        .unwrap();

    let monitor = layout.active_monitor_ref().unwrap();
    let names = monitor
        .workspaces
        .iter()
        .filter_map(Workspace::sway_name)
        .collect::<Vec<_>>();
    assert_eq!(names, ["1", "2", "10"]);
    assert!(monitor
        .workspaces
        .iter()
        .find(|workspace| workspace.has_window(&0))
        .is_some_and(|workspace| workspace.sway_name().as_deref() == Some("2")));
}

#[test]
fn move_to_output_names_destination_before_moving_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let output = layout
        .outputs()
        .find(|output| output.name() == "output2")
        .unwrap()
        .clone();

    layout.move_to_output(Some(&0), &output, None, ActivateWindow::No);

    let workspace = layout
        .monitor_for_output(&output)
        .unwrap()
        .workspaces
        .iter()
        .find(|workspace| workspace.has_window(&0))
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn move_to_workspace_by_idx_does_not_leave_empty_workspaces() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(1),
        Op::MoveWindowToWorkspace {
            window_id: Some(0),
            workspace_idx: 2,
        },
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert!(monitors[0].workspaces[1].has_windows());
}

#[test]
fn empty_workspaces_dont_move_back_to_original_output() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
        Op::FocusWorkspace(1),
        Op::CloseWindow(1),
        Op::AddOutput(1),
    ];

    check_ops(ops);
}

#[test]
fn empty_named_workspace_is_destroyed_with_its_output() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
    ];

    let layout = check_ops(ops);
    assert!(layout.workspaces().all(|(_, _, ws)| ws.name().is_none()));
}

#[test]
fn large_negative_height_change() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::AdjustProportion(-1e129),
        },
    ];

    let mut options = Options::default();
    options.layout.border.off = false;
    options.layout.border.width = 1.;

    check_ops_with_options(options, ops);
}

#[test]
fn floating_normal_border_has_titlebar_above_squared_client() {
    let radius = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    let config = Config::load_default();
    let mut layout = check_ops_with_options(
        Options {
            layout: config.layout,
            ..Default::default()
        },
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams {
                    is_floating: true,
                    rules: Some(ResolvedWindowRules {
                        geometry_corner_radius: Some(radius),
                        clip_to_geometry: Some(true),
                        ..Default::default()
                    }),
                    ..TestWindowParams::new(1)
                },
            },
        ],
    );
    layout.update_render_elements(None);

    let workspace = layout.active_workspace().unwrap();
    let floating = workspace.floating();
    let (tile, ipc) = floating.tiles_with_ipc_layouts().next().unwrap();
    assert!(tile.has_sway_titlebar());
    assert_eq!(tile.geometry_corner_radius(), radius);
    let expected_outer = tile.geometry_corner_radius();
    assert_eq!(
        tile.decoration_corner_radii(),
        (
            Some(expected_outer),
            swayward_config::CornerRadius {
                top_left: 0.,
                top_right: 0.,
                ..expected_outer
            }
        )
    );
    let titlebar = floating.ipc_decoration_rect(tile, &ipc).unwrap();
    let tile_pos: Point<f64, Logical> = ipc.tile_pos_in_workspace_view.unwrap().into();
    assert_eq!(titlebar.loc.y, tile_pos.y);
    assert_eq!(titlebar.loc.x, tile_pos.x);
    assert_eq!(titlebar.size.w, ipc.tile_size.0);
    let (window, hit) = workspace
        .window_under(titlebar.loc + titlebar.size.downscale(2.).to_point())
        .unwrap();
    assert_eq!(window.id(), &1);
    assert_eq!(
        hit,
        HitType::Activate {
            is_tab_indicator: true
        }
    );
}

#[test]
fn outer_gaps_change_tiled_and_floating_workspace_geometry() {
    let mut options = Options::default();
    options.layout.border.off = true;
    options.layout.gaps = 0.;
    options.layout.outer_gaps = swayward_config::OuterGaps {
        left: 30.,
        right: 10.,
        top: 20.,
        bottom: 0.,
    };
    options.layout.outer_gaps_configured = true;
    let mut floating = TestWindowParams::new(2);
    floating.is_floating = true;
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow { params: floating },
        ],
    );

    let workspace = layout.active_workspace().unwrap();
    assert_eq!(
        workspace.working_area(),
        Rectangle::new((30., 20.).into(), (1240., 700.).into())
    );
    let tiled = workspace
        .tiling()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &1)
        .unwrap()
        .1;
    assert_eq!(tiled.tile_pos_in_workspace_view, Some((30., 20.)));
    let floating = workspace
        .floating()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &2)
        .unwrap()
        .1;
    assert_eq!(floating.tile_pos_in_workspace_view, Some((600., 270.)));
}

#[test]
fn smart_borders_no_gaps_uses_resolved_workspace_gaps() {
    for ((inner, outer), expected) in [
        ((0., 5.), ResizeEdge::all()),
        ((10., -2.), ResizeEdge::all()),
        ((10., -10.), ResizeEdge::empty()),
    ] {
        let mut options = Options::default();
        options.layout.border.off = false;
        options.layout.gaps = inner;
        options.layout.outer_gaps = swayward_config::OuterGaps::all(outer);
        options.layout.outer_gaps_configured = true;
        options.layout.smart_borders = swayward_config::SmartBorders::NoGaps;
        let layout = check_ops_with_options(
            options,
            [
                Op::AddOutput(1),
                Op::AddWindow {
                    params: TestWindowParams::new(1),
                },
            ],
        );

        let node = layout.active_workspace().unwrap().ipc_tiling_tree();
        let IpcNode::Split { children, .. } = node else {
            panic!("root must be a split")
        };
        let IpcNode::Leaf { border_edges, .. } = children[0] else {
            panic!("window must be a leaf")
        };
        assert_eq!(border_edges, expected, "outer gap {outer}");
    }
}

#[test]
fn negative_outer_gaps_add_to_inner_gaps() {
    let mut options = Options::default();
    options.layout.gaps = 10.;
    options.layout.outer_gaps = swayward_config::OuterGaps::all(-2.);
    options.layout.outer_gaps_configured = true;
    let layout = check_ops_with_options(options, [Op::AddOutput(1)]);

    assert_eq!(
        layout.active_workspace().unwrap().working_area(),
        Rectangle::new((8., 8.).into(), (1264., 704.).into())
    );
}

#[test]
fn outer_gaps_use_sways_proportional_minimum_size_clamp() {
    let area = workspace::apply_outer_gaps(
        Rectangle::from_size(Size::from((150., 100.))),
        swayward_config::OuterGaps {
            left: 100.,
            right: 300.,
            top: 20.,
            bottom: 60.,
        },
        0.,
        true,
    );

    assert_eq!(area, Rectangle::new((12., 10.).into(), (100., 60.).into()));
}

#[test]
fn moving_subtree_to_node_cleans_source_after_attachment() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("source".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let (source, node) = layout.tiling_target_for_window(&0).unwrap();
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    let (target, target_node) = layout.tiling_target_for_window(&1).unwrap();
    layout.active_monitor().unwrap().workspace_switch = None;

    layout
        .move_tiling_subtree_to_node(source, node, target, target_node)
        .unwrap();

    assert_eq!(layout.window_workspace_id(&0), Some(target));
    assert!(layout.find_workspace_by_id(source).is_none());
}

#[test]
fn sticky_window_does_not_keep_focus_on_an_empty_workspace_switch() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams {
            is_floating: true,
            ..TestWindowParams::new(0)
        },
    }
    .apply(&mut layout);
    assert!(layout.set_window_sticky(&0, "enable"));

    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("2".into()))
        .unwrap();

    assert!(layout.active_workspace().unwrap().active_window().is_none());
    assert_eq!(
        layout.window_workspace_id(&0),
        Some(layout.active_workspace().unwrap().id())
    );
}

#[test]
fn making_window_sticky_moves_before_cleaning_source_workspace() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("source".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams {
            is_floating: true,
            ..TestWindowParams::new(0)
        },
    }
    .apply(&mut layout);
    let source = layout.window_workspace_id(&0).unwrap();
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();
    layout.active_monitor().unwrap().workspace_switch = None;

    assert!(layout.set_window_sticky(&0, "enable"));

    assert!(layout.window_workspace_id(&0).is_some());
    assert!(layout.find_workspace_by_id(source).is_none());
}

#[test]
fn sticky_floating_tree_follows_workspace_focus() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=2 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }
    let source = layout.active_workspace().unwrap().id();
    let workspace = layout.active_workspace_mut().unwrap();
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let focused = workspace.tiling().node_for_window(&1).unwrap();
    workspace.tiling_mut().set_focus(focused);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.finish_tiling_subtree_detach(old_parent);
    workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );
    assert!(layout.set_window_sticky(&1, "enable"));

    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();

    let target = layout.active_workspace().unwrap();
    assert_eq!(target.floating_tree_root_for_window(&1), Some(root));
    assert_eq!(target.floating_tree_root_for_window(&2), Some(root));
    assert_eq!(target.floating().tree(root).unwrap().focus(), Some(focused));
    assert_ne!(target.id(), source);
}

#[test]
fn tiled_window_restores_natural_size_when_first_floated() {
    let mut options = Options::default();
    options.layout.border.off = true;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams {
                    bbox: Rectangle::from_size(Size::from((400, 150))),
                    ..TestWindowParams::new(1)
                },
            },
        ],
    );

    layout.toggle_window_floating(Some(&1));

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((400, 150))));
    let (_, ipc) = layout
        .active_workspace()
        .unwrap()
        .floating()
        .tiles_with_ipc_layouts()
        .next()
        .unwrap();
    assert_eq!(ipc.window_size, (400, 150));
}

#[test]
fn tiled_window_gets_sway_default_size_when_first_moved_to_scratchpad() {
    let mut options = Options::default();
    options.layout.border.off = true;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ],
    );

    layout.move_to_scratchpad(Some(&1));
    layout.show_scratchpad(Some(&1));

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((640, 540))));
    let workspace = layout.active_workspace().unwrap();
    let (_, pos) = workspace
        .floating()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &1)
        .unwrap();
    assert_eq!(pos.tile_pos_in_workspace_view, Some((320., 90.)));
}

// sway/tree/container.c:990-994: every return to tiling removes the
// container from the scratchpad, including a drag toggled to tiling.
#[test]
fn toggling_a_dragged_scratchpad_window_to_tiling_removes_it_from_the_scratchpad() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ]);
    layout.move_to_scratchpad(Some(&1));
    layout.show_scratchpad(Some(&1));
    assert!(layout.is_scratchpad_window(&1));

    check_ops_on_layout(
        &mut layout,
        [
            Op::InteractiveMoveBegin {
                window: 1,
                output_idx: 1,
                px: 0.0,
                py: 0.0,
            },
            Op::InteractiveMoveUpdate {
                window: 1,
                dx: 100.0,
                dy: 100.0,
                output_idx: 1,
                px: 0.0,
                py: 0.0,
            },
            Op::ToggleWindowFloating { id: None },
        ],
    );
    assert!(layout.is_scratchpad_window(&1), "still mid-drag");

    check_ops_on_layout(&mut layout, [Op::InteractiveMoveEnd { window: 1 }]);
    assert!(!layout.is_scratchpad_window(&1));
    assert!(layout.scratchpad_is_empty());
}

#[test]
fn scratchpad_default_size_honors_client_size_hints() {
    let mut options = Options::default();
    options.layout.border.off = true;
    let mut layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams {
                    min_max_size: (Size::from((700, 100)), Size::from((800, 400))),
                    ..TestWindowParams::new(1)
                },
            },
        ],
    );

    layout.move_to_scratchpad(Some(&1));
    layout.show_scratchpad(Some(&1));

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((700, 400))));
    let workspace = layout.active_workspace().unwrap();
    let (_, layout) = workspace
        .floating()
        .tiles_with_ipc_layouts()
        .find(|(tile, _)| tile.window().id() == &1)
        .unwrap();
    assert_eq!(layout.tile_pos_in_workspace_view, Some((290., 160.)));
}

#[test]
fn configured_floating_constraints_clamp_resize_requests() {
    let mut options = Options::default();
    options.layout.border.off = true;
    options.layout.floating_minimum_size = swayward_config::FloatingSize {
        width: 60,
        height: 50,
    };
    options.layout.floating_maximum_size = swayward_config::FloatingSize {
        width: 100,
        height: 90,
    };
    let mut params = TestWindowParams::new(1);
    params.is_floating = true;
    let layout = check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow { params },
            Op::SetWindowWidth {
                id: None,
                change: SizeChange::SetFixed(200),
            },
            Op::SetWindowHeight {
                id: None,
                change: SizeChange::SetFixed(10),
            },
        ],
    );

    let window = layout
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1;
    assert_eq!(window.0.requested_size.get(), Some(Size::from((100, 50))));
}

#[test]
fn large_max_size() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                min_max_size: (Size::from((0, 0)), Size::from((i32::MAX, i32::MAX))),
                ..TestWindowParams::new(1)
            },
        },
    ];

    let mut options = Options::default();
    options.layout.border.off = false;
    options.layout.border.width = 1.;

    let layout = check_ops_with_options(options, ops);
    let window = layout.windows().next().unwrap().1;
    assert!(
        window.0.requested_size.get().unwrap().w < i32::MAX,
        "layout must request a finite usable width"
    );
}

#[test]
fn inactive_empty_configured_workspace_is_destroyed_after_focus_changes() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout.set_workspace_name("source".into(), None);
    let source = layout.active_workspace().unwrap().id();

    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();
    Op::CompleteAnimations.apply(&mut layout);

    assert!(layout.find_workspace_by_id(source).is_none());
}

#[test]
fn workspace_cleanup_during_switch() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::CloseWindow(1),
    ];

    let layout = check_ops(ops);
    assert_eq!(layout.windows().count(), 0);
    assert_eq!(layout.workspaces().count(), 1);
}

#[test]
fn workspace_transfer_during_switch() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::RemoveOutput(1),
        Op::FocusWorkspaceDown,
        Op::FocusWorkspaceDown,
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);
    assert_eq!(layout.outputs().count(), 2);
    assert_eq!(layout.windows().count(), 2);
    assert!(layout.windows().any(|(_, window)| window.id() == &1));
    assert!(layout.windows().any(|(_, window)| window.id() == &2));
}

#[test]
fn workspace_transfer_during_switch_from_last() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(2),
        Op::RemoveOutput(1),
        Op::FocusWorkspaceUp,
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);
    assert_eq!(layout.outputs().count(), 2);
    assert_eq!(layout.windows().count(), 1);
    assert!(layout.windows().any(|(_, window)| window.id() == &1));
}

#[test]
fn workspace_transfer_during_switch_gets_cleaned_up() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(1),
        Op::AddOutput(2),
        Op::MoveFocusedToWorkspaceDown(true),
        Op::MoveFocusedToWorkspaceDown(true),
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);
    assert_eq!(layout.outputs().count(), 2);
    assert_eq!(layout.windows().count(), 1);
    assert!(layout.windows().any(|(_, window)| window.id() == &1));
}

#[test]
fn moving_the_only_workspace_replaces_it_before_reparenting() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    Op::FocusOutput(2).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::FocusOutput(1).apply(&mut layout);

    let MonitorSet::Normal {
        monitors,
        active_monitor_idx,
        ..
    } = &mut layout.monitor_set
    else {
        unreachable!()
    };
    *active_monitor_idx = 0;
    monitors[0].workspaces[0].set_sway_identity(None, Some(2));
    monitors[0].add_workspace_at(1);
    monitors[1].workspaces[0].set_sway_identity(None, Some(3));
    let source = monitors[0].workspaces[0].id();
    let destination = monitors[1].output.clone();

    assert!(layout.move_workspace_to_output_by_id(source, None, &destination));

    let MonitorSet::Normal {
        monitors,
        active_monitor_idx,
        ..
    } = &layout.monitor_set
    else {
        unreachable!()
    };
    assert_eq!(*active_monitor_idx, 1);
    assert_eq!(
        monitors[0]
            .workspaces
            .iter()
            .filter_map(Workspace::sway_name)
            .collect::<Vec<_>>(),
        ["1"]
    );
    assert_eq!(
        monitors[1]
            .workspaces
            .iter()
            .filter_map(Workspace::sway_name)
            .collect::<Vec<_>>(),
        ["2", "3"]
    );
    assert_eq!(
        monitors[1].workspaces[monitors[1].active_workspace_idx].id(),
        source
    );
}

#[test]
fn move_to_named_target_index_preserves_addressable_workspace() {
    // Fuzzer seed: a numbered-but-unnamed workspace is addressable and must
    // survive a targeted move even though it has no windows.
    let layout = check_ops([
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: None,
            layout_config: None,
        },
        Op::AddOutput(4),
        Op::MoveWindowToOutput {
            window_id: None,
            output_id: 4,
            target_ws_idx: Some(1),
        },
    ]);
    assert!(layout
        .workspaces()
        .any(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("ws1")));
}

#[test]
fn unname_then_implicit_rename_preserves_workspace_invariants() {
    // CI shrank `random_operations_dont_panic` to this six-op sequence. Both
    // the move and rename use the implicit target (`None`), and the same output
    // is added twice.
    let layout = check_ops([
        Op::UnnameWorkspace { ws_name: 1 },
        Op::AddOutput(1),
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::MoveWindowDownOrToWorkspaceDown,
        Op::MoveWindowToWorkspace {
            window_id: None,
            workspace_idx: 0,
        },
        Op::SetWorkspaceName {
            new_ws_name: 1,
            ws_name: None,
        },
    ]);
    assert_eq!(
        layout.active_workspace().unwrap().sway_name().as_deref(),
        Some("ws1")
    );
    assert!(layout.active_workspace().unwrap().has_window(&1));
}

#[test]
fn mapping_a_window_does_not_create_a_ghost_workspace() {
    // sway creates a workspace on demand and destroys it when it empties
    // (`sway/tree/workspace.c:313-330`). Mapping a window onto the only
    // workspace must not append niri's trailing scrolling-strip placeholder,
    // which a bar and `workspace next` would both show as a ghost.
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
    ]);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };
    assert_eq!(monitors[0].workspaces.len(), 1);
    assert!(monitors[0].workspaces[0].has_windows());
}

#[test]
fn move_workspace_to_output() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::FocusOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::MoveWorkspaceToOutput(2),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal {
        monitors,
        active_monitor_idx,
        ..
    } = layout.monitor_set
    else {
        unreachable!()
    };

    assert_eq!(active_monitor_idx, 1);
    assert_eq!(monitors[0].workspaces.len(), 1);
    assert!(!monitors[0].workspaces[0].has_windows());
    assert_eq!(monitors[1].active_workspace_idx, 0);
    // Just the moved workspace. There is no trailing placeholder: that was
    // niri's scrolling-strip affordance, and sway creates workspaces on
    // demand instead.
    assert_eq!(monitors[1].workspaces.len(), 1);
    assert!(monitors[1].workspaces[0].has_windows());
}

#[test]
fn removing_all_outputs_preserves_empty_named_workspaces() {
    let ops = [
        Op::AddOutput(1),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: None,
            layout_config: None,
        },
        Op::AddNamedWorkspace {
            ws_name: 2,
            output_name: None,
            layout_config: None,
        },
        Op::RemoveOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::NoOutputs { workspaces } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(workspaces.len(), 2);
}

#[test]
fn non_finite_floating_proportions_are_noops() {
    let mut layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
    ]);

    let before = layout
        .active_workspace()
        .unwrap()
        .active_window_visual_rectangle();
    for change in [
        PositionChange::SetProportion(f64::NAN),
        PositionChange::AdjustProportion(f64::NAN),
        PositionChange::SetProportion(f64::INFINITY),
        PositionChange::AdjustProportion(f64::NEG_INFINITY),
    ] {
        layout.move_floating_window(Some(&1), change, PositionChange::AdjustFixed(0.), false);
        assert_eq!(
            layout
                .active_workspace()
                .unwrap()
                .active_window_visual_rectangle(),
            before
        );
        layout.verify_invariants();
    }

    let stored_position = |invalid_change| {
        let mut layout = check_ops([
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
        ]);
        layout.move_floating_window(
            Some(&1),
            PositionChange::SetFixed(100.),
            PositionChange::SetFixed(200.),
            false,
        );
        if let Some((x, y)) = invalid_change {
            layout.move_floating_window(Some(&1), x, y, false);
        }
        layout.toggle_window_floating(Some(&1));
        layout.verify_invariants();
        layout
            .active_workspace()
            .unwrap()
            .active_window_visual_rectangle()
    };
    let control = stored_position(None);
    assert_eq!(
        stored_position(Some((
            PositionChange::SetProportion(f64::NAN),
            PositionChange::AdjustProportion(f64::NAN),
        ))),
        control
    );
}

#[test]
fn config_change_updates_cached_sizes() {
    let mut config = Config::default();
    let border = &mut config.layout.border;
    border.off = false;
    border.width = 2.;

    let mut layout = Layout::new(Clock::default(), &config);

    Op::AddWindow {
        params: TestWindowParams {
            bbox: Rectangle::from_size(Size::from((1280, 200))),
            ..TestWindowParams::new(1)
        },
    }
    .apply(&mut layout);

    config.layout.border.width = 4.;
    layout.update_config(&config);

    layout.verify_invariants();
}

#[test]
fn preset_height_change_removes_preset() {
    let mut config = Config::default();
    config.layout.preset_window_heights = vec![PresetSize::Fixed(1), PresetSize::Fixed(2)];

    let mut layout = Layout::new(Clock::default(), &config);

    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::NestOrUnnestWindowLeft { id: None },
        Op::SwitchPresetWindowHeight { id: None },
        Op::SwitchPresetWindowHeight { id: None },
    ];
    for op in ops {
        op.apply(&mut layout);
    }

    // Leave only one.
    config.layout.preset_window_heights = vec![PresetSize::Fixed(1)];

    layout.update_config(&config);

    layout.verify_invariants();
}

#[test]
fn set_window_height_recomputes_to_auto() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::NestOrUnnestWindowLeft { id: None },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::NestOrUnnestWindowLeft { id: None },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(100),
        },
        Op::FocusWindowUp,
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(200),
        },
    ];

    check_ops(ops);
}

#[test]
fn one_window_in_column_becomes_weight_1() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::NestOrUnnestWindowLeft { id: None },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::NestOrUnnestWindowLeft { id: None },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(100),
        },
        Op::Communicate(2),
        Op::FocusWindowUp,
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(200),
        },
        Op::Communicate(1),
        Op::CloseWindow(0),
        Op::CloseWindow(1),
    ];

    check_ops(ops);
}

#[test]
fn fixed_height_takes_max_non_auto_into_account() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::SetWindowHeight {
            id: Some(0),
            change: SizeChange::SetFixed(704),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::NestOrUnnestWindowLeft { id: None },
    ];

    let options = Options {
        layout: swayward_config::Layout {
            border: swayward_config::Border {
                off: false,
                width: 4.,
                ..Default::default()
            },
            gaps: 0.,
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn start_interactive_move_then_remove_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::CloseWindow(0),
    ];

    check_ops(ops);
}

#[test]
fn moving_popup_target_ignores_tile_animation_offset() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();

    assert!(layout.interactive_move_begin(0, &output, Point::from((50., 100.))));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        output,
        Point::from((500., 500.)),
    ));

    let InteractiveMoveState::Moving(move_) = layout.interactive_move.as_ref().unwrap() else {
        panic!("window must be moving");
    };
    assert_ne!(move_.tile.render_offset().y, 0.);
    let pointer_offset_y = move_.tile.window_size().h * move_.pointer_ratio_within_window.1;
    let stable_tile_y =
        move_.pointer_pos_within_output.y - pointer_offset_y - move_.tile.window_loc().y;
    let target = layout.popup_target_rect(&0);
    assert_eq!(target.loc.y, -stable_tile_y - move_.tile.window_loc().y);
}

#[test]
fn interactive_move_keeps_source_until_drop_is_attached() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let source = layout.window_workspace_id(&0).unwrap();
    Op::FocusWorkspaceDown.apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();

    assert!(layout.interactive_move_begin(0, &output, Point::default()));
    assert!(layout.interactive_move_update(&0, Point::from((1000., 0.)), output, Point::default(),));

    assert!(layout.find_workspace_by_id(source).is_some());
    layout.interactive_move_end(&0);
    assert!(layout.window_workspace_id(&0).is_some());
}

#[test]
fn interactive_drop_creates_named_destination_before_inserting_window() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();

    assert!(layout.interactive_move_begin(0, &output, Point::default()));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        output,
        Point::from((0., 10000.)),
    ));
    layout.interactive_move_end(&0);

    let workspace = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .map(|(_, _, workspace)| workspace)
        .unwrap();
    assert_eq!(workspace.sway_name().as_deref(), Some("2"));
}

#[test]
fn drag_over_creation_slot_uses_preview_then_materializes_workspace() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let output = layout.outputs().next().unwrap().clone();
    let monitor = layout.monitor_for_output(&output).unwrap();
    let last = monitor.workspaces.last().unwrap().id();
    let last_geo = monitor.workspaces_render_geo().last().unwrap();
    // The creation slot is the empty region below the last workspace. There is
    // no trailing placeholder workspace to drop onto.
    let pointer = last_geo.loc + Point::from((last_geo.size.w / 2., last_geo.size.h * 2.));
    let workspace_count = monitor.workspaces.len();
    let (target, preview_geo) = monitor.insert_position(pointer);
    let InsertWorkspace::Preview(preview) = target else {
        panic!("creation slot must be represented by preview state");
    };
    assert_eq!(preview.insertion_index, workspace_count);
    assert_eq!(preview_geo, preview.geometry);

    assert!(layout.interactive_move_begin(0, &output, Point::default()));
    assert!(layout.interactive_move_update(&0, Point::from((1000., 0.)), output, pointer,));
    layout.update_insert_hint(None);

    let monitor = layout.active_monitor_ref().unwrap();
    assert_eq!(monitor.workspaces.len(), workspace_count);
    assert!(matches!(
        monitor.insert_hint.as_ref().map(|hint| hint.workspace),
        Some(InsertWorkspace::Preview(_))
    ));

    layout.interactive_move_end(&0);
    let (_, _, destination) = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&0))
        .unwrap();
    assert_ne!(destination.id(), last);
    assert!(destination.has_sway_identity());
}

#[test]
fn interactive_move_onto_empty_output() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::AddOutput(2),
        Op::InteractiveMoveUpdate {
            window: 0,
            dx: 1000.,
            dy: 0.,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveEnd { window: 0 },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_onto_empty_output_ewaf() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::AddOutput(2),
        Op::InteractiveMoveUpdate {
            window: 0,
            dx: 1000.,
            dy: 0.,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveEnd { window: 0 },
    ];

    let options = Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn interactive_move_onto_last_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::InteractiveMoveBegin {
            window: 0,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveUpdate {
            window: 0,
            dx: 1000.,
            dy: 0.,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::FocusWorkspaceDown,
        Op::AdvanceAnimations { msec_delta: 1000 },
        Op::InteractiveMoveEnd { window: 0 },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_onto_first_empty_workspace() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::InteractiveMoveBegin {
            window: 1,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::InteractiveMoveUpdate {
            window: 1,
            dx: 1000.,
            dy: 0.,
            output_idx: 1,
            px: 0.,
            py: 0.,
        },
        Op::FocusWorkspaceUp,
        Op::AdvanceAnimations { msec_delta: 1000 },
        Op::InteractiveMoveEnd { window: 1 },
    ];
    let options = Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn output_active_workspace_is_preserved() {
    // Two workspaces need two names now: without niri's trailing placeholder,
    // `workspace next` on a single workspace has nowhere to go.
    let ops = [
        Op::AddOutput(1),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: Some(1),
            layout_config: None,
        },
        Op::AddNamedWorkspace {
            ws_name: 2,
            output_name: Some(1),
            layout_config: None,
        },
        Op::FocusWorkspace(0),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::RemoveOutput(1),
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(monitors[0].active_workspace_idx, 1);
}

#[test]
fn output_active_workspace_is_preserved_with_other_outputs() {
    let ops = [
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: Some(1),
            layout_config: None,
        },
        Op::AddNamedWorkspace {
            ws_name: 2,
            output_name: Some(1),
            layout_config: None,
        },
        Op::FocusWorkspace(0),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::RemoveOutput(1),
        Op::AddOutput(1),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert_eq!(monitors[1].active_workspace_idx, 1);
}

#[test]
fn named_workspace_to_output() {
    let ops = [
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: None,
            layout_config: None,
        },
        Op::AddOutput(1),
        Op::MoveWorkspaceToOutput(1),
        Op::FocusWorkspaceUp,
    ];
    check_ops(ops);
}

#[test]
fn named_workspace_uses_first_available_sway_output_assignment() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);

    layout.ensure_named_workspace(&WorkspaceConfig {
        name: WorkspaceName("assigned".into()),
        sway_output_assignment: Some(vec!["missing".into(), "output2".into(), "output1".into()]),
        open_on_output: None,
        layout: None,
    });

    let (monitor, _, _) = layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.name().is_some_and(|name| name == "assigned"))
        .unwrap();
    assert_eq!(monitor.unwrap().output_name(), "output2");
}

#[test]
fn move_window_to_different_output() {
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::MoveWorkspaceToOutput(2),
    ];
    let options = Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn floating_tree_entry_routes_geometry_focus_hit_testing_and_lifecycle() {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut workspace = Workspace::new(
        output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for id in 1..=2 {
        let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            TiledWidth::Proportion(0.5),
            false,
            false,
            None,
        );
    }
    let first = workspace.tiling().node_for_window(&1).unwrap();
    workspace
        .tiling_mut()
        .set_layout(first, tiling_tree::Layout::SplitV);
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    workspace.tiling_mut().set_focus(first);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.finish_tiling_subtree_detach(old_parent);
    let rect = Rectangle::new((100., 120.).into(), (600., 450.).into());

    let (root, remapped) = workspace.floating_mut().add_tree(subtree, rect);
    assert!(remapped.is_empty());
    assert_eq!(workspace.floating().tree(root).unwrap().parent_area(), rect);
    assert_eq!(
        workspace.floating().tree(root).unwrap().geometry(first),
        Some(Rectangle::new((100., 120.).into(), (300., 450.).into()))
    );
    assert!(workspace
        .floating_mut()
        .tree_mut(root)
        .unwrap()
        .focus_parent());
    let parent = workspace.floating().tree(root).unwrap().focus().unwrap();
    assert_ne!(parent, first);
    assert!(workspace
        .floating()
        .tree(root)
        .unwrap()
        .contains_node(root, parent));

    let detached = workspace.floating_mut().remove_tree(root).unwrap();
    let (restored, remapped) = workspace.attach_tiling_subtree(detached);
    assert_eq!(restored, root);
    assert!(remapped.is_empty());
    assert_eq!(workspace.tiling().windows().count(), 2);
    workspace.verify_invariants(None);
}

#[test]
fn moving_the_only_child_of_a_floating_group_keeps_the_root_position() {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut workspace = Workspace::new(
        output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for id in 1..=2 {
        let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            TiledWidth::Proportion(0.5),
            false,
            false,
            None,
        );
    }
    let second = workspace.tiling().node_for_window(&2).unwrap();
    workspace
        .tiling_mut()
        .split(second, tiling_tree::Layout::SplitV);
    workspace.tiling_mut().focus_parent();
    let group = workspace.tiling().focus().unwrap();
    workspace.set_container_floating(group, true).unwrap();
    workspace.focus_child();

    let root = workspace.floating_tree_root_for_window(&2).unwrap();
    let before = workspace.floating().tree_rect(root).unwrap();
    assert!(workspace
        .floating()
        .focused_leaf_is_only_child_of_tree_root());

    workspace.move_window_in_direction(&2, tiling_tree::Direction::Right, 10.);

    assert_eq!(workspace.floating().tree_rect(root), Some(before));
    assert_eq!(workspace.tiling().windows().count(), 1);
    workspace.verify_invariants(None);
}

#[test]
fn removing_a_floating_tree_leaf_uses_the_resident_tree() {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut workspace = Workspace::new(
        output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for id in 1..=2 {
        let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            TiledWidth::Proportion(0.5),
            false,
            false,
            None,
        );
    }
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.finish_tiling_subtree_detach(old_parent);
    workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );

    let removed = workspace.remove_tile(&1, Transaction::new());

    assert_eq!(removed.tile.window().id(), &1);
    assert!(workspace.floating().has_window(&2));
    assert!(!workspace.floating().has_window(&1));
    workspace.verify_invariants(None);
}

#[test]
fn floating_tree_root_tracks_output_geometry_changes() {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut workspace = Workspace::new(
        output.clone(),
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for id in 1..=2 {
        let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            TiledWidth::Proportion(0.5),
            false,
            false,
            None,
        );
    }
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.finish_tiling_subtree_detach(old_parent);
    let (root, _) = workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );

    workspace.floating_mut().update_config(
        (2560., 1440.).into(),
        Rectangle::from_size((2560., 1440.).into()),
        1.,
        Rc::new(Options::default()),
    );

    assert_eq!(
        workspace.floating().tree_rect(root),
        Some(Rectangle::new((200., 240.).into(), (600., 450.).into()))
    );
    workspace.floating().verify_invariants();
}

#[test]
fn fullscreen_targets_a_node_inside_a_floating_tree() {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut workspace = Workspace::new(
        output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for id in 1..=2 {
        let tile = workspace.make_tile(TestWindow::new(TestWindowParams::new(id)));
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            TiledWidth::Proportion(0.5),
            false,
            false,
            None,
        );
    }
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.finish_tiling_subtree_detach(old_parent);
    workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );
    workspace.activate_window(&1);
    workspace.floating_mut().focus_parent();

    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Workspace)));
    assert_eq!(
        workspace.floating().tree(root).unwrap().fullscreen_node(),
        Some(root)
    );
    assert_eq!(
        workspace.fullscreen_mode(),
        Some(tiling_tree::FullscreenMode::Workspace)
    );
    assert!(workspace.fullscreen_contains_window(&1));
    assert_eq!(workspace.fullscreen_window(), Some(&1));

    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Global)));
    assert_eq!(
        workspace.fullscreen_mode(),
        Some(tiling_tree::FullscreenMode::Global)
    );
    workspace.disable_fullscreen();
    assert_eq!(workspace.fullscreen_mode(), None);

    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Workspace)));
    assert!(workspace.set_focused_fullscreen(None));
    workspace.floating_mut().focus_child();
    assert!(workspace.set_focused_fullscreen(Some(tiling_tree::FullscreenMode::Workspace)));
    assert_eq!(
        workspace.floating().tree(root).unwrap().fullscreen_node(),
        workspace.floating().tree(root).unwrap().node_for_window(&1)
    );
}

#[test]
fn floating_tree_root_survives_workspace_and_output_moves() {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut source = Workspace::new(
        output.clone(),
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for id in 1..=2 {
        let tile = source.make_tile(TestWindow::new(TestWindowParams::new(id)));
        source.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            TiledWidth::Proportion(0.5),
            false,
            false,
            None,
        );
    }
    let first = source.tiling().node_for_window(&1).unwrap();
    source.tiling_mut().focus_root();
    let root = source.tiling().focus().unwrap();
    source.tiling_mut().set_focus(first);
    let (subtree, old_parent) = source.detach_tiling_subtree(root).unwrap();
    source.finish_tiling_subtree_detach(old_parent);
    let old_rect = Rectangle::new((100., 120.).into(), (600., 450.).into());
    let (root, _) = source.floating_mut().add_tree(subtree, old_rect);
    source
        .floating_mut()
        .tree_mut(root)
        .unwrap()
        .set_fullscreen(&1, true);
    source.floating_mut().set_tree_sticky(root, true);
    let node_ids = source
        .floating()
        .tree(root)
        .unwrap()
        .iter_depth_first()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    let removed = source.remove_floating_tree(root).unwrap();
    assert!(source.floating().is_empty());

    let target_output = Output::new(
        "target".into(),
        PhysicalProperties {
            size: Size::from((2560, 1440)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    target_output.change_current_state(
        Some(Mode {
            size: Size::from((2560, 1440)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    target_output.user_data().insert_if_missing(|| OutputName {
        connector: "target".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut target = Workspace::new(
        target_output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    let restored = target.add_floating_tree(removed, true);
    let tree = target.floating().tree(restored).unwrap();

    assert_eq!(restored, root);
    assert_eq!(tree.focus(), Some(first));
    assert_eq!(
        tree.iter_depth_first()
            .map(|(id, _)| id)
            .collect::<Vec<_>>(),
        node_ids
    );
    assert_eq!(tree.fullscreen_node(), tree.node_for_window(&1));
    assert!(target.floating().tree_is_sticky(restored));
    assert_eq!(
        target.floating().tree_rect(restored),
        Some(Rectangle::new((500., 465.).into(), (600., 450.).into()))
    );
    target.verify_invariants(None);
}

#[test]
fn directional_focus_descends_into_a_floating_tree() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=2 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }
    let workspace = layout.active_workspace_mut().unwrap();
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let first = workspace.tiling().node_for_window(&1).unwrap();
    workspace.tiling_mut().set_focus(first);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.finish_tiling_subtree_detach(old_parent);
    workspace.floating_mut().add_tree(
        subtree,
        Rectangle::new((100., 120.).into(), (600., 450.).into()),
    );

    assert!(workspace.floating_mut().focus_right());
    assert_eq!(workspace.floating().active_window().unwrap().id(), &2);
}

#[test]
fn floating_tree_scratchpad_moves_the_whole_root() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in 1..=2 {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }
    let workspace = layout.active_workspace_mut().unwrap();
    workspace.tiling_mut().focus_root();
    let root = workspace.tiling().focus().unwrap();
    let focused = workspace.tiling().node_for_window(&1).unwrap();
    workspace.tiling_mut().set_focus(focused);
    let (subtree, old_parent) = workspace.detach_tiling_subtree(root).unwrap();
    workspace.finish_tiling_subtree_detach(old_parent);
    let rect = Rectangle::new((100., 120.).into(), (600., 450.).into());
    let (root, _) = workspace.floating_mut().add_tree(subtree, rect);

    layout.move_to_scratchpad(Some(&1));
    assert!(layout.is_scratchpad_hidden(&1));
    assert!(layout.is_scratchpad_hidden(&2));
    assert_eq!(layout.scratchpad_windows().count(), 2);

    assert_eq!(layout.show_scratchpad(Some(&2)), Some(1));
    let workspace = layout.active_workspace().unwrap();
    assert_eq!(workspace.floating_tree_root_for_window(&1), Some(root));
    assert_eq!(workspace.floating_tree_root_for_window(&2), Some(root));
    assert_eq!(
        workspace.floating().tree(root).unwrap().focus(),
        Some(focused)
    );
    layout.verify_invariants();
}

#[test]
fn mixed_layer_selection_filters_one_global_focus_order() {
    let output = Output::new(
        "output".into(),
        PhysicalProperties {
            size: Size::from((1280, 720)),
            subpixel: Subpixel::Unknown,
            make: String::new(),
            model: String::new(),
            serial_number: String::new(),
        },
    );
    output.change_current_state(
        Some(Mode {
            size: Size::from((1280, 720)),
            refresh: 60000,
        }),
        None,
        None,
        None,
    );
    output.user_data().insert_if_missing(|| OutputName {
        connector: "output".into(),
        make: None,
        model: None,
        serial: None,
    });
    let mut workspace = Workspace::new(
        output,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );
    for (id, timestamp) in [(1, 4), (2, 3), (3, 2), (4, 1)] {
        let window = TestWindow::new(TestWindowParams::new(id));
        window
            .0
            .focus_timestamp
            .set(Some(Duration::from_secs(timestamp)));
        let tile = workspace.make_tile(window);
        workspace.add_tile(
            tile,
            WorkspaceAddWindowTarget::Auto,
            ActivateWindow::Yes,
            TiledWidth::Proportion(0.5),
            false,
            false,
            None,
        );
        if id >= 3 {
            workspace.toggle_window_floating(Some(&id));
        }
    }

    workspace.activate_window(&1);
    for expected in [1, 2, 3, 4] {
        assert_eq!(workspace.active_window().unwrap().id(), &expected);
        workspace.remove_tile(&expected, Transaction::new());
    }
}

#[test]
fn close_window_empty_ws_above_first() {
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(1),
        Op::CloseWindow(1),
    ];
    let options = Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn add_and_remove_output() {
    let ops = [
        Op::AddOutput(2),
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::RemoveOutput(2),
    ];
    let options = Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn switch_ewaf_on() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ];

    let mut layout = check_ops(ops);
    layout.update_options(Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    });
    layout.verify_invariants();
}

#[test]
fn switch_ewaf_off() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
    ];

    let options = Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    };
    let mut layout = check_ops_with_options(options, ops);
    layout.update_options(Options::default());
    layout.verify_invariants();
}

#[test]
fn interactive_move_drop_on_other_output_during_animation() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::InteractiveMoveBegin {
            window: 3,
            output_idx: 3,
            px: 0.0,
            py: 0.0,
        },
        Op::FocusWorkspaceDown,
        Op::AddOutput(4),
        Op::InteractiveMoveUpdate {
            window: 3,
            dx: 0.0,
            dy: 8300.68619826683,
            output_idx: 4,
            px: 0.0,
            py: 0.0,
        },
        Op::RemoveOutput(4),
        Op::InteractiveMoveEnd { window: 3 },
    ];
    check_ops(ops);
}

#[test]
fn add_window_next_to_only_interactively_moved_without_outputs() {
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(1),
        Op::InteractiveMoveBegin {
            window: 2,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 3586.692842955048,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::RemoveOutput(1),
        // We have no outputs, and the only existing window is interactively moved, meaning there
        // are no workspaces either.
        Op::AddWindowNextTo {
            params: TestWindowParams::new(3),
            next_to_id: 2,
        },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_toggle_floating_ends_dnd_gesture() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::InteractiveMoveBegin {
            window: 2,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 3586.692842955048,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::Refresh { is_active: false },
        Op::ToggleWindowFloating { id: None },
        Op::InteractiveMoveEnd { window: 2 },
    ];

    check_ops(ops);
}

#[test]
fn interactive_move_from_workspace_with_layout_config() {
    let ops = [
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: Some(2),
            layout_config: Some(Box::new(swayward_config::LayoutPart {
                border: Some(swayward_config::BorderRule {
                    on: true,
                    ..Default::default()
                }),
                ..Default::default()
            })),
        },
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::InteractiveMoveBegin {
            window: 2,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 3586.692842955048,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        // Now remove and add the output. It will have the same workspace.
        Op::RemoveOutput(1),
        Op::AddOutput(1),
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 0.0,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
        // Now move onto a different workspace.
        Op::FocusWorkspaceDown,
        Op::CompleteAnimations,
        Op::InteractiveMoveUpdate {
            window: 2,
            dx: 0.0,
            dy: 0.0,
            output_idx: 1,
            px: 0.0,
            py: 0.0,
        },
    ];

    check_ops(ops);
}

#[test]
fn set_width_fixed_negative() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleWindowFloating { id: Some(3) },
        Op::SetFocusedWidth(SizeChange::SetFixed(-100)),
    ];
    check_ops(ops);
}

#[test]
fn set_height_fixed_negative() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleWindowFloating { id: Some(3) },
        Op::SetWindowHeight {
            id: None,
            change: SizeChange::SetFixed(-100),
        },
    ];
    check_ops(ops);
}

#[test]
fn interactive_resize_to_negative() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleWindowFloating { id: Some(3) },
        Op::InteractiveResizeBegin {
            window: 3,
            edges: ResizeEdge::BOTTOM_RIGHT,
        },
        Op::InteractiveResizeUpdate {
            window: 3,
            dx: -10000.,
            dy: -10000.,
        },
    ];
    check_ops(ops);
}

#[test]
fn windows_on_other_workspaces_remain_activated() {
    let ops = [
        Op::AddOutput(3),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::FocusWorkspaceDown,
        Op::Refresh { is_active: true },
    ];

    let layout = check_ops(ops);
    let (_, win) = layout.windows().next().unwrap();
    assert!(win.0.pending_activated.get());
}

#[test]
fn stacking_add_parent_brings_up_child() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(1),
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
    ];

    check_ops(ops);
}

#[test]
fn stacking_add_parent_brings_up_descendants() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(2),
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(0),
                ..TestWindowParams::new(1)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(2)
            },
        },
    ];

    check_ops(ops);
}

#[test]
fn stacking_activate_brings_up_descendants() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(0),
                ..TestWindowParams::new(1)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                parent_id: Some(1),
                ..TestWindowParams::new(2)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(3)
            },
        },
        Op::FocusWindow(0),
    ];

    check_ops(ops);
}

#[test]
fn stacking_set_parent_brings_up_child() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(0)
            },
        },
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
        Op::SetParent {
            id: 0,
            new_parent_id: Some(1),
        },
    ];

    check_ops(ops);
}

#[test]
fn move_window_to_workspace_with_different_active_output() {
    let ops = [
        Op::AddOutput(0),
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusOutput(1),
        Op::MoveWindowToWorkspace {
            window_id: Some(0),
            workspace_idx: 2,
        },
    ];

    check_ops(ops);
}

#[test]
fn set_first_workspace_name() {
    let ops = [
        Op::AddOutput(0),
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
    ];

    check_ops(ops);
}

#[test]
fn set_first_workspace_name_ewaf() {
    let ops = [
        Op::AddOutput(0),
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
    ];

    let options = Options {
        layout: swayward_config::Layout {
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn set_last_workspace_name() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusWorkspaceDown,
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
    ];

    check_ops(ops);
}

#[test]
fn move_workspace_to_same_monitor_doesnt_reorder() {
    let ops = [
        Op::AddOutput(0),
        Op::SetWorkspaceName {
            new_ws_name: 0,
            ws_name: None,
        },
        Op::AddNamedWorkspace {
            ws_name: 1,
            output_name: Some(0),
            layout_config: None,
        },
        Op::FocusWorkspace(0),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::FocusWorkspaceDown,
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::MoveWorkspaceToMonitor {
            ws_name: Some(0),
            output_id: 0,
        },
    ];

    let layout = check_ops(ops);
    let counts: Vec<_> = layout
        .workspaces()
        .map(|(_, _, ws)| ws.windows().count())
        .collect();
    assert_eq!(counts, &[1, 2]);
}

#[test]
fn removing_window_above_preserves_focused_window() {
    let ops = [
        Op::AddOutput(0),
        Op::AddWindow {
            params: TestWindowParams::new(0),
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::FocusFirstRootChild,
        Op::NestFocusedWindow,
        Op::NestFocusedWindow,
        Op::FocusWindowDown,
        Op::CloseWindow(0),
    ];

    let layout = check_ops(ops);
    let win = layout.focus().unwrap();
    assert_eq!(win.0.id, 1);
}

#[test]
fn move_focused_to_workspace_unfocused_with_multiple_monitors() {
    let ops = [
        Op::AddOutput(1),
        Op::SetWorkspaceName {
            new_ws_name: 101,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddNamedWorkspace {
            ws_name: 102,
            output_name: Some(1),
            layout_config: None,
        },
        Op::FocusWorkspace(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(2),
        Op::FocusOutput(2),
        Op::SetWorkspaceName {
            new_ws_name: 201,
            ws_name: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::AddWindow {
            params: TestWindowParams::new(4),
        },
        Op::MoveFocusedToOutput {
            output_id: 1,
            target_ws_idx: Some(0),
            activate: false,
        },
        Op::FocusOutput(1),
    ];

    let layout = check_ops(ops);

    assert_eq!(layout.active_workspace().unwrap().name().unwrap(), "ws102");

    for (mon, win) in layout.windows() {
        let mon = mon.unwrap();
        let ws = mon
            .workspaces
            .iter()
            .find(|w| w.has_window(win.id()))
            .unwrap();

        assert_eq!(
            ws.name().unwrap(),
            match win.id() {
                1 | 4 => "ws101",
                2 => "ws102",
                3 => "ws201",
                _ => unreachable!(),
            }
        );
    }
}

#[test]
fn move_focused_to_workspace_down_focus_false_on_floating_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleWindowFloating { id: None },
        Op::MoveFocusedToWorkspaceDown(false),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert!(monitors[0].active_workspace_ref().has_window(&1));
}

#[test]
fn move_focused_to_workspace_focus_false_on_floating_window() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleWindowFloating { id: None },
        Op::MoveFocusedToWorkspace(1, false),
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };

    assert!(monitors[0].active_workspace_ref().has_window(&1));
}

#[test]
fn restore_to_floating_persists_across_fullscreen_maximize() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleWindowFloating { id: None },
        // Maximize then fullscreen.
        Op::MaximizeWindowToEdges { id: None },
        Op::FullscreenWindow(1),
        // Unfullscreen.
        Op::FullscreenWindow(1),
    ];

    let mut layout = check_ops(ops);

    // Unfullscreening should return the window to the maximized state.
    let scrolling = layout.active_workspace().unwrap().tiling();
    assert!(scrolling.tiles().next().is_some());

    let ops = [
        // Unmaximize.
        Op::MaximizeWindowToEdges { id: None },
    ];
    check_ops_on_layout(&mut layout, ops);

    // Unmaximize should return the window back to floating.
    let scrolling = layout.active_workspace().unwrap().tiling();
    assert!(scrolling.tiles().next().is_none());
}

#[test]
fn unmaximize_during_fullscreen_does_not_float() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleWindowFloating { id: None },
        // Maximize then fullscreen.
        Op::MaximizeWindowToEdges { id: None },
        Op::FullscreenWindow(1),
        // Unmaximize.
        Op::MaximizeWindowToEdges { id: None },
    ];

    let mut layout = check_ops(ops);

    // Unmaximize shouldn't have changed the window state since it's fullscreen.
    let scrolling = layout.active_workspace().unwrap().tiling();
    assert!(scrolling.tiles().next().is_some());

    let ops = [
        // Unfullscreen.
        Op::FullscreenWindow(1),
    ];
    check_ops_on_layout(&mut layout, ops);

    // Unfullscreen should return the window back to floating.
    let scrolling = layout.active_workspace().unwrap().tiling();
    assert!(scrolling.tiles().next().is_none());
}

#[test]
fn tabs_with_different_border() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                rules: Some(ResolvedWindowRules {
                    border: swayward_config::BorderRule {
                        on: true,
                        ..Default::default()
                    },
                    ..ResolvedWindowRules::default()
                }),
                ..TestWindowParams::new(2)
            },
        },
        Op::SwitchPresetWindowHeight { id: None },
        Op::ToggleFocusedTabbedDisplay,
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::NestOrUnnestWindowLeft { id: None },
    ];

    let options = Options {
        layout: swayward_config::Layout {
            struts: Struts {
                left: FloatOrInt(0.),
                right: FloatOrInt(0.),
                top: FloatOrInt(20000.),
                bottom: FloatOrInt(0.),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    check_ops_with_options(options, ops);
}

#[test]
fn expel_pending_left_from_fullscreen_tabbed_column() {
    let ops = [
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FullscreenWindow(1),
        Op::Communicate(1),
        // 1 is now fullscreen, view_offset_to_restore is set.
        Op::ToggleFocusedTabbedDisplay,
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::NestOrUnnestWindowLeft { id: Some(2) },
        // 2 is consumed into a fullscreen column, fullscreen is requested but not applied.
        //
        // Now, get it back out while keeping it focused.
        //
        // Importantly, we expel it *left*, which results in adding a new column with the exact
        // same active root-child index.
        Op::FocusWindow(2),
        Op::NestOrUnnestWindowLeft { id: None },
    ];

    check_ops(ops);
}

#[test]
fn render_geometry_has_no_workspace_creation_slot() {
    let layout = check_ops([Op::AddOutput(1)]);
    let MonitorSet::Normal { monitors, .. } = layout.monitor_set else {
        unreachable!()
    };
    let monitor = &monitors[0];

    assert_eq!(
        monitor.workspaces_render_geo().count(),
        monitor.workspaces.len()
    );
}

#[test]
fn workspace_render_geo_at_fractional_scale() {
    let ops = [
        Op::AddScaledOutput {
            id: 1,
            scale: 1.1,
            layout_config: None,
        },
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::FocusWorkspaceDown,
        Op::CompleteAnimations,
    ];

    let layout = check_ops(ops);

    let MonitorSet::Normal { monitors, .. } = &layout.monitor_set else {
        unreachable!()
    };

    let mon = &monitors[0];
    let mut iter = mon.workspaces_with_render_geo();
    let (_ws, geo) = iter.next().unwrap();
    assert!(
        iter.next().is_none(),
        "animations are completed, only one workspace should be visible"
    );
    assert_eq!(
        geo.loc.y, 0.,
        "active workspace must be at y = 0 exactly, \
         otherwise a pointer against the screen edge at y = 0 won't hit it"
    );
}

fn parent_id_causes_loop(layout: &Layout<TestWindow>, id: usize, mut parent_id: usize) -> bool {
    if parent_id == id {
        return true;
    }

    'outer: loop {
        for (_, win) in layout.windows() {
            if win.0.id == parent_id {
                match win.0.parent_id.get() {
                    Some(new_parent_id) => {
                        if new_parent_id == id {
                            // Found a loop.
                            return true;
                        }

                        parent_id = new_parent_id;
                        continue 'outer;
                    }
                    // Reached window with no parent.
                    None => return false,
                }
            }
        }

        // Parent is not in the layout.
        return false;
    }
}

fn arbitrary_spacing() -> impl Strategy<Value = f64> {
    // Give equal weight to:
    // - 0: the element is disabled
    // - 4: some reasonable value
    // - random value, likely unreasonably big
    prop_oneof![Just(0.), Just(4.), ((1.)..=65535.)]
}

fn arbitrary_spacing_neg() -> impl Strategy<Value = f64> {
    // Give equal weight to:
    // - 0: the element is disabled
    // - 4: some reasonable value
    // - -4: some reasonable negative value
    // - random value, likely unreasonably big
    prop_oneof![Just(0.), Just(4.), Just(-4.), ((1.)..=65535.)]
}

fn arbitrary_struts() -> impl Strategy<Value = Struts> {
    (
        arbitrary_spacing_neg(),
        arbitrary_spacing_neg(),
        arbitrary_spacing_neg(),
        arbitrary_spacing_neg(),
    )
        .prop_map(|(left, right, top, bottom)| Struts {
            left: FloatOrInt(left),
            right: FloatOrInt(right),
            top: FloatOrInt(top),
            bottom: FloatOrInt(bottom),
        })
}

fn arbitrary_tab_indicator_position() -> impl Strategy<Value = TabIndicatorPosition> {
    prop_oneof![
        Just(TabIndicatorPosition::Left),
        Just(TabIndicatorPosition::Right),
        Just(TabIndicatorPosition::Top),
        Just(TabIndicatorPosition::Bottom),
    ]
}

prop_compose! {
    fn arbitrary_focus_ring()(
        off in any::<bool>(),
        width in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
    ) -> swayward_config::BorderRule {
        swayward_config::BorderRule {
            off,
            on: !off,
            width,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_border()(
        off in any::<bool>(),
        width in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
    ) -> swayward_config::BorderRule {
        swayward_config::BorderRule {
            off,
            on: !off,
            width,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_shadow()(
        off in any::<bool>(),
        softness in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
    ) -> swayward_config::ShadowRule {
        swayward_config::ShadowRule {
            off,
            on: !off,
            softness,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_tab_indicator()(
        off in any::<bool>(),
        hide_when_single_tab in prop::option::of(any::<bool>().prop_map(Flag)),
        place_within_column in prop::option::of(any::<bool>().prop_map(Flag)),
        width in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
        gap in prop::option::of(arbitrary_spacing_neg().prop_map(FloatOrInt)),
        length in prop::option::of((0f64..2f64)
            .prop_map(|x| TabIndicatorLength { total_proportion: Some(x) })),
        position in prop::option::of(arbitrary_tab_indicator_position()),
    ) -> swayward_config::TabIndicatorPart {
        swayward_config::TabIndicatorPart {
            off,
            on: !off,
            hide_when_single_tab,
            place_within_column,
            width,
            gap,
            length,
            position,
            ..Default::default()
        }
    }
}

prop_compose! {
    fn arbitrary_layout_part()(
        gaps in prop::option::of(arbitrary_spacing().prop_map(FloatOrInt)),
        struts in prop::option::of(arbitrary_struts()),
        focus_ring in prop::option::of(arbitrary_focus_ring()),
        border in prop::option::of(arbitrary_border()),
        shadow in prop::option::of(arbitrary_shadow()),
        tab_indicator in prop::option::of(arbitrary_tab_indicator()),
        default_orientation in prop::option::of(arbitrary_default_orientation()),
    ) -> swayward_config::LayoutPart {
        swayward_config::LayoutPart {
            gaps,
            struts,
            default_orientation,
            focus_ring,
            border,
            shadow,
            tab_indicator,
            ..Default::default()
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_none() {
            eprintln!("ignoring slow test");
            0
        } else {
            ProptestConfig::default().cases
        },
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_operations_dont_panic(
        ops: Vec<Op>,
        layout_config in arbitrary_layout_part(),
    ) {
        // eprintln!("{ops:?}");
        let options = Options {
            layout: swayward_config::Layout::from_part(&layout_config),
            ..Default::default()
        };

        check_ops_with_options(options, ops);
    }
}

#[test]
fn focus_parent_with_only_a_floating_window_preserves_tree_invariants() {
    let layout = check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(1)
            },
        },
        Op::FocusParent,
    ]);
    assert_eq!(layout.focus().unwrap().id(), &1);
}

#[test]
fn first_interactive_move_update_focuses_the_destination_output() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let outputs = layout.outputs().cloned().collect::<Vec<_>>();
    layout.focus_output(&outputs[0]);

    assert!(layout.interactive_move_begin(0, &outputs[0], Point::default()));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        outputs[1].clone(),
        Point::default(),
    ));

    assert_eq!(layout.active_output(), Some(&outputs[1]));
}

#[test]
fn drop_on_a_tile_centre_across_outputs_exchanges_windows() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    Op::AddOutput(2).apply(&mut layout);
    let outputs = layout.outputs().cloned().collect::<Vec<_>>();
    layout.focus_output(&outputs[1]);
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    layout.focus_output(&outputs[0]);

    let source_workspace = layout
        .monitor_for_output(&outputs[0])
        .unwrap()
        .active_workspace_ref()
        .id();
    let target_workspace = layout
        .monitor_for_output(&outputs[1])
        .unwrap()
        .active_workspace_ref()
        .id();
    let target_workspace_ref = layout
        .monitor_for_output(&outputs[1])
        .unwrap()
        .active_workspace_ref();
    let target_node = target_workspace_ref.tiling_node_for_window(&1).unwrap();
    let target_rect = target_workspace_ref
        .tiling()
        .node_geometry(target_node)
        .unwrap();
    let (target_pos, target_size) = (target_rect.loc, target_rect.size);
    let target_centre = target_pos + target_size.to_point().downscale(2.);

    assert!(layout.interactive_move_begin(0, &outputs[0], Point::default()));
    assert!(layout.interactive_move_update(
        &0,
        Point::from((1000., 0.)),
        outputs[1].clone(),
        target_centre,
    ));
    layout.interactive_move_end(&0);

    let source = layout
        .workspaces()
        .find(|(_, _, ws)| ws.id() == source_workspace)
        .unwrap()
        .2;
    let target = layout
        .workspaces()
        .find(|(_, _, ws)| ws.id() == target_workspace)
        .unwrap()
        .2;
    assert!(source.has_window(&1));
    assert!(target.has_window(&0));
}

#[test]
fn drop_on_a_tile_centre_swaps_instead_of_inserting() {
    // Sway decides a tiling drop by edge: a centre hit on a container swaps the
    // two windows rather than inserting beside one
    // (sway/sway/input/seatop_move_tiling.c:365-388).
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    for id in [0, 1] {
        Op::AddWindow {
            params: TestWindowParams::new(id),
        }
        .apply(&mut layout);
    }

    let output = layout.outputs().next().unwrap().clone();
    let monitor = layout.monitor_for_output(&output).unwrap();
    let workspace = monitor.active_workspace_ref();
    let target = workspace.tiling_node_for_window(&0).unwrap();
    let target_rect = workspace.tiling().node_geometry(target).unwrap();
    let geo = (target_rect.loc, target_rect.size);

    // The middle of the first tile must be a swap, and its outer quarter an
    // insertion, so the two regions are distinguishable.
    let centre = geo.0 + geo.1.to_point().downscale(2.);
    let left_edge = geo.0 + Size::from((geo.1.w / 8., geo.1.h / 2.)).to_point();
    let centre_position = workspace.scrolling_insert_position(centre);
    assert!(
        matches!(
            centre_position,
            crate::layout::monitor::InsertPosition::SwapWith(_)
        ),
        "a centre drop must swap: {centre:?} {geo:?} {centre_position:?}"
    );
    assert!(
        matches!(
            workspace.scrolling_insert_position(left_edge),
            crate::layout::monitor::InsertPosition::InsertAt(_, _)
        ),
        "an edge drop must insert"
    );
}

#[test]
fn focus_parent_then_move_left_keeps_focus_on_a_live_node() {
    // CI 35986895235 shrank `random_operations_dont_panic` to this sequence
    // (proptest cc acc67c75). Moving a focused parent container left after a
    // column move left the tree's focus pointing at a removed node.
    let layout = check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetFocusedDisplay(ColumnDisplay::Normal),
        Op::MoveLeft,
        Op::FocusDownOrLeft,
        Op::FocusParent,
        Op::MoveWindowInDirection(tiling_tree::Direction::Left),
    ]);
    assert!(matches!(
        layout.focus().map(|window| *window.id()),
        Some(1 | 2)
    ));
    assert_eq!(layout.windows().count(), 2);
}

#[test]
fn refreshing_after_hiding_an_interactive_move_does_not_panic() {
    check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(5),
        },
        Op::AddOutput(2),
        Op::InteractiveMoveBegin {
            window: 5,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::ToggleFocusedContainerFloating,
        Op::MoveFocusedToScratchpad,
        Op::Refresh { is_active: false },
    ]);
}

#[test]
fn interactive_move_update_after_scratchpad_transfer_does_not_panic() {
    check_ops([
        Op::AddOutput(2),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::InteractiveMoveBegin {
            window: 1,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
        Op::MoveFocusedToScratchpad,
        Op::ToggleFocusedContainerFloating,
        Op::InteractiveMoveUpdate {
            window: 1,
            dx: 1.,
            dy: 0.,
            output_idx: 2,
            px: 0.,
            py: 0.,
        },
    ]);
}

#[test]
fn unfloat_container_after_changing_its_layout_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::FocusParent,
        Op::SetFocusedLayout(tiling_tree::Layout::Stacked),
        Op::SetFocusedLayout(tiling_tree::Layout::SplitH),
        Op::ToggleWindowFloating { id: None },
    ]);
}

#[test]
fn interactive_move_on_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(5),
        Op::AddWindow {
            params: TestWindowParams::new(5),
        },
        Op::ToggleFocusedContainerFloating,
        Op::InteractiveMoveBegin {
            window: 5,
            output_idx: 5,
            px: 0.,
            py: 0.,
        },
    ]);
}

#[test]
fn adding_a_floating_window_next_to_a_floating_container_does_not_panic() {
    let mut floating = TestWindowParams::new(3);
    floating.is_floating = true;
    check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(1),
        Op::ToggleFocusedContainerFloating,
        Op::AddWindowNextTo {
            params: floating,
            next_to_id: 2,
        },
    ]);
}

#[test]
fn interactive_resize_on_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleFocusedContainerFloating,
        Op::InteractiveResizeBegin {
            window: 3,
            edges: ResizeEdge::RIGHT,
        },
    ]);
}

#[test]
fn moving_a_floating_workspace_between_fractional_scales_does_not_panic() {
    check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddScaledOutput {
            id: 2,
            scale: 2.,
            layout_config: None,
        },
        Op::ToggleFocusedContainerFloating,
        Op::MoveWindowUpOrToWorkspaceUp,
        Op::AddScaledOutput {
            id: 1,
            scale: 1.5,
            layout_config: None,
        },
        Op::MoveWorkspaceToOutput(1),
    ]);
}

#[test]
fn moving_a_tiny_window_to_scratchpad_with_a_huge_border_does_not_panic() {
    let mut layout = swayward_config::Layout::default();
    layout.border.width = 29_707.;
    check_ops_with_options(
        Options {
            layout,
            ..Default::default()
        },
        vec![
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::MoveFocusedToScratchpad,
        ],
    );
}

#[test]
fn hiding_the_active_floating_container_focuses_the_remaining_leaf() {
    let mut floating = TestWindowParams::new(1);
    floating.is_floating = true;
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::ToggleFocusedContainerFloating,
        Op::AddWindow { params: floating },
        Op::FocusWindowDown,
        Op::MoveFocusedToScratchpad,
    ]);
}

#[test]
fn moving_a_hidden_scratchpad_window_to_an_output_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(2),
        Op::MoveFocusedToScratchpad,
        Op::MoveWindowToOutput {
            window_id: Some(2),
            output_id: 1,
            target_ws_idx: None,
        },
    ]);
}

#[test]
fn centering_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::CenterWindow { id: None },
    ]);
}

#[test]
fn moving_the_last_floating_leaf_keeps_a_resident_tree_active() {
    let mut floating = TestWindowParams::new(1);
    floating.is_floating = true;
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(3),
        },
        Op::ToggleFocusedContainerFloating,
        Op::AddWindow { params: floating },
        Op::MoveWindowToWorkspaceDown(false),
    ]);
}

#[test]
fn resizing_a_window_in_a_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::SetFocusedWidth(SizeChange::SetFixed(0)),
    ]);
}

#[test]
fn directional_focus_with_one_floating_container_does_not_panic() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::FocusLeft,
    ]);
}

#[test]
fn toggling_a_window_in_a_floating_container_unfloats_the_container() {
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::ToggleWindowFloating { id: None },
    ]);
}

#[test]
fn preset_width_on_floating_container_does_not_panic() {
    // The floating-group operation generator found this command dispatching to
    // the leaf-only floating list for a resident tree (cc 7508638e).
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::ToggleFocusedContainerFloating,
        Op::SwitchPresetTiledWidth,
    ]);
}

#[test]
fn unfloat_last_group_after_focusing_parent_deactivates_floating() {
    // The deep proptest soak shrank this to a floating group whose parent had
    // focus while its last member returned to tiling (cc e3487ca6).
    let mut options = Options::default();
    options.layout.default_orientation = swayward_config::DefaultOrientation::Vertical;
    check_ops_with_options(
        options,
        [
            Op::AddOutput(1),
            Op::AddWindow {
                params: TestWindowParams::new(3),
            },
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::MoveFocusedToWorkspaceUp(false),
            Op::UpdateConfig {
                layout_config: Box::default(),
            },
            Op::FocusWindowTop,
            Op::ToggleWindowFloating { id: None },
            Op::FocusParent,
            Op::ToggleWindowFloating { id: Some(3) },
            Op::FocusChild,
        ],
    );
}

#[test]
fn singleton_move_after_floating_close_keeps_the_parent_live() {
    // CI 36310511080 shrank `random_operations_dont_panic` to this sequence
    // (proptest cc 16b75ae3). A directional move of the only window read a
    // parent node that an earlier layout change had already removed.
    let mut floating = TestWindowParams::new(5);
    floating.is_floating = true;
    let mut options = Options::default();
    options.layout.default_orientation = swayward_config::DefaultOrientation::Vertical;
    check_ops_with_options(
        options,
        [
            Op::AddWindow {
                params: TestWindowParams::new(3),
            },
            Op::AddOutput(1),
            Op::CenterWindow { id: None },
            Op::AddWindow { params: floating },
            Op::MoveFocusedToWorkspaceUp(false),
            Op::ToggleWindowFloating { id: None },
            Op::SwapWindowHorizontal(false),
            Op::FocusParent,
            Op::CloseWindow(5),
            Op::SplitFocused(tiling_tree::Layout::SplitH),
            Op::MoveWindowDown,
        ],
    );
}

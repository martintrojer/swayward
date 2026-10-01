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

mod floating_group;
mod floating_scratchpad;
mod fullscreen;
mod gaps_borders;
mod interactive_move;
mod sticky;
mod tree;
mod workspace_lifecycle;

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

fn floating_group_lifecycle_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (1..=5usize).prop_map(|id| Op::AddWindow {
            params: TestWindowParams::new(id),
        }),
        4 => (1..=5usize).prop_map(Op::CloseWindow),
        3 => (1..=5usize).prop_map(Op::FullscreenWindow),
        3 => (1..=5usize, any::<bool>()).prop_map(|(window, is_fullscreen)| {
            Op::SetFullscreenWindow {
                window,
                is_fullscreen,
            }
        }),
        2 => (1..=5usize, arbitrary_resize_edge()).prop_map(|(window, edges)| {
            Op::InteractiveResizeBegin { window, edges }
        }),
        2 => (1..=5usize, -20000f64..20000f64, -20000f64..20000f64).prop_map(
            |(window, dx, dy)| Op::InteractiveResizeUpdate { window, dx, dy },
        ),
        2 => (1..=5usize).prop_map(|window| Op::InteractiveResizeEnd { window }),
        3 => Just(Op::FocusParent),
        3 => Just(Op::FocusChild),
        3 => arbitrary_tree_layout().prop_map(Op::SplitFocused),
        3 => arbitrary_tree_layout().prop_map(Op::SetFocusedLayout),
        3 => Just(Op::ToggleFocusedContainerFloating),
        4 => (0..=4usize, any::<bool>())
            .prop_map(|(workspace, focus)| Op::MoveFocusedToWorkspace(workspace, focus)),
        4 => (1..=2usize, prop::option::of(0..=4usize), any::<bool>()).prop_map(
            |(output_id, target_ws_idx, activate)| Op::MoveFocusedToOutput {
                output_id,
                target_ws_idx,
                activate,
            },
        ),
        3 => (1..=2usize).prop_map(Op::MoveWorkspaceToOutput),
        3 => Just(Op::MoveFocusedToScratchpad),
        2 => Just(Op::MoveFocusedContainerToNextWorkspace),
        2 => (1..=2usize).prop_map(Op::RemoveOutput),
        2 => (1..=2usize).prop_map(Op::AddOutput),
        2 => (1..=5usize).prop_map(Op::FocusWindow),
        2 => (1..=2usize).prop_map(Op::FocusOutput),
        1 => Just(Op::FocusFloating),
        1 => Just(Op::FocusTiling),
    ]
}

fn floating_group_lifecycle_ops() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(floating_group_lifecycle_op(), 0..80).prop_map(|mut tail| {
        let mut ops = vec![
            Op::AddOutput(1),
            Op::AddOutput(2),
            Op::AddWindow {
                params: TestWindowParams::new(1),
            },
            Op::AddWindow {
                params: TestWindowParams::new(2),
            },
            Op::SplitFocused(tiling_tree::Layout::SplitV),
            Op::FocusParent,
            Op::ToggleFocusedContainerFloating,
            Op::FocusChild,
        ];
        ops.append(&mut tail);
        ops
    })
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

fn collect_ipc_windows(node: IpcNode<usize>, windows: &mut Vec<usize>) {
    match node {
        IpcNode::Split { children, .. } => {
            for child in children {
                collect_ipc_windows(child, windows);
            }
        }
        IpcNode::Leaf { window, .. } => windows.push(window),
    }
}

#[track_caller]
fn verify_layout_windows_reachable_once(layout: &Layout<TestWindow>) {
    let mut layout_windows = layout
        .windows()
        .map(|(_, window)| *window.id())
        .collect::<Vec<_>>();
    let layout_window_count = layout_windows.len();
    layout_windows.sort_unstable();
    layout_windows.dedup();
    assert_eq!(layout_windows.len(), layout_window_count);

    let mut ipc_windows = Vec::new();
    for (_, _, workspace) in layout.workspaces() {
        collect_ipc_windows(workspace.ipc_tiling_tree(), &mut ipc_windows);
        for (_, tree, _) in workspace.ipc_floating_trees() {
            collect_ipc_windows(tree, &mut ipc_windows);
        }
    }
    for (tree, _) in layout.scratchpad_trees() {
        collect_ipc_windows(tree, &mut ipc_windows);
    }
    for window in layout.scratchpad_windows() {
        if !ipc_windows.contains(window.id()) {
            ipc_windows.push(*window.id());
        }
    }
    ipc_windows.sort_unstable();
    assert_eq!(ipc_windows, layout_windows);
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
    check_ops(ops);
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
fn move_window_to_different_output() {
    let ops = [
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::AddOutput(1),
        Op::AddOutput(2),
        Op::MoveWorkspaceToOutput(2),
    ];
    check_ops(ops);
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
    check_ops(ops);
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

    #[test]
    fn floating_group_lifecycle_operations_preserve_reachability(
        ops in floating_group_lifecycle_ops(),
    ) {
        let mut layout = Layout::default();
        for op in ops {
            let before = layout.windows().count();
            let is_add = matches!(&op, Op::AddWindow { .. });
            let is_close = matches!(&op, Op::CloseWindow(_));
            op.apply(&mut layout);
            layout.verify_invariants();
            verify_layout_windows_reachable_once(&layout);
            let after = layout.windows().count();
            if is_add {
                assert!(after == before || after == before + 1);
            } else if is_close {
                assert!(after == before || after + 1 == before);
            } else {
                assert_eq!(after, before);
            }
        }
    }
}

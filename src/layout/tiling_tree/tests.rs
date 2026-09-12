use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use proptest::prelude::*;

use super::*;
use crate::animation::Clock;
use crate::layout::{
    tile::Tile, ConfigureIntent, InteractiveResizeData, LayoutElementRenderSnapshot, Options,
    SizingMode,
};
use crate::render_helpers::offscreen::OffscreenData;
use crate::utils::transaction::Transaction;
use crate::window::ResolvedWindowRules;
use smithay::output::{self, Output};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Point, Serial, Transform};

#[derive(Debug)]
struct TestWindowInner {
    id: usize,
    size: Cell<Size<i32, Logical>>,
    requested_size: Cell<Option<Size<i32, Logical>>>,
    requested_mode: Cell<SizingMode>,
    configure_count: Cell<usize>,
    received_transaction: Cell<bool>,
    interactive_resize: Cell<Option<InteractiveResizeData>>,
    rules: ResolvedWindowRules,
}

#[derive(Debug, Clone)]
struct TestWindow(Rc<TestWindowInner>);

impl TestWindow {
    fn new(id: usize) -> Self {
        Self(Rc::new(TestWindowInner {
            id,
            size: Cell::new(Size::from((100, 200))),
            requested_size: Cell::new(None),
            requested_mode: Cell::new(SizingMode::Normal),
            configure_count: Cell::new(0),
            received_transaction: Cell::new(false),
            interactive_resize: Cell::new(None),
            rules: ResolvedWindowRules::default(),
        }))
    }
}

fn tree(size: (f64, f64), gaps: f64) -> TilingTree<TestWindow> {
    let mut options = Options::default();
    options.layout.gaps = gaps;
    let size = Size::from(size);
    TilingTree::new(
        size,
        Rectangle::from_size(size),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    )
}

fn tile(id: usize, size: Size<f64, Logical>) -> Tile<TestWindow> {
    Tile::new(
        TestWindow::new(id),
        size,
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    )
}

impl LayoutElement for TestWindow {
    type Id = usize;

    fn id(&self) -> &Self::Id {
        &self.0.id
    }
    fn size(&self) -> Size<i32, Logical> {
        self.0.size.get()
    }
    fn buf_loc(&self) -> Point<i32, Logical> {
        (0, 0).into()
    }
    fn is_in_input_region(&self, _: Point<f64, Logical>) -> bool {
        false
    }
    fn request_size(
        &mut self,
        size: Size<i32, Logical>,
        mode: SizingMode,
        _: bool,
        transaction: Option<Transaction>,
    ) {
        self.0.requested_size.set(Some(size));
        self.0.received_transaction.set(transaction.is_some());
        self.0.requested_mode.set(mode);
    }
    fn min_size(&self) -> Size<i32, Logical> {
        Size::from((0, 0))
    }
    fn max_size(&self) -> Size<i32, Logical> {
        Size::from((0, 0))
    }
    fn is_wl_surface(&self, _: &WlSurface) -> bool {
        false
    }
    fn set_preferred_scale_transform(&self, _: output::Scale, _: Transform) {}
    fn has_ssd(&self) -> bool {
        false
    }
    fn output_enter(&self, _: &Output) {}
    fn output_leave(&self, _: &Output) {}
    fn set_offscreen_data(&self, _: Option<OffscreenData>) {}
    fn set_activated(&mut self, _: bool) {}
    fn set_bounds(&self, _: Size<i32, Logical>) {}
    fn is_ignoring_opacity_window_rule(&self) -> bool {
        false
    }
    fn configure_intent(&self) -> ConfigureIntent {
        ConfigureIntent::CanSend
    }
    fn send_pending_configure(&mut self) {
        self.0.configure_count.set(self.0.configure_count.get() + 1);
    }
    fn set_active_in_column(&mut self, _: bool) {}
    fn set_floating(&mut self, _: bool) {}
    fn sizing_mode(&self) -> SizingMode {
        SizingMode::Normal
    }
    fn pending_sizing_mode(&self) -> SizingMode {
        self.0.requested_mode.get()
    }
    fn requested_size(&self) -> Option<Size<i32, Logical>> {
        self.0.requested_size.get()
    }
    fn is_child_of(&self, _: &Self) -> bool {
        false
    }
    fn refresh(&self) {}
    fn rules(&self) -> &ResolvedWindowRules {
        &self.0.rules
    }
    fn take_animation_snapshot(&mut self) -> Option<LayoutElementRenderSnapshot> {
        None
    }
    fn set_interactive_resize(&mut self, data: Option<InteractiveResizeData>) {
        self.0.interactive_resize.set(data);
    }
    fn cancel_interactive_resize(&mut self) {}
    fn on_commit(&mut self, _: Serial) {}
    fn interactive_resize_data(&self) -> Option<InteractiveResizeData> {
        None
    }
    fn is_urgent(&self) -> bool {
        false
    }
}

#[test]
fn empty_tree_has_no_focus() {
    let t = tree((1920., 1080.), 0.);
    assert!(t.is_empty());
    assert_eq!(t.focus(), None);
    t.check_invariants();
}

#[test]
fn one_window_fills_the_view() {
    let mut t = tree((1920., 1080.), 0.);
    let id = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    assert_eq!(
        t.geometry(id).unwrap(),
        Rectangle::from_size(Size::from((1920., 1080.)))
    );
    assert_eq!(t.focus(), Some(id));
    t.check_invariants();
}

#[test]
fn two_windows_split_h_halve_the_view() {
    let mut t = tree((1920., 1080.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    assert_eq!(t.geometry(a).unwrap().size.w, 960.);
    assert_eq!(t.geometry(b).unwrap().size.w, 960.);
    t.check_invariants();
}

#[test]
fn inserting_a_sibling_subdivides_the_target_share() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert_eq!(t.geometry(a).unwrap().size.w, 600.);
    assert_eq!(t.geometry(b).unwrap().size.w, 300.);
    assert_eq!(t.geometry(c).unwrap().size.w, 300.);
    t.check_invariants();
}

#[test]
fn removing_a_sibling_collapses_the_implicit_container() {
    let mut t = tree((1920., 1080.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(b, Layout::SplitV);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.remove_tile_node(c);
    t.check_invariants();
    assert_eq!(t.geometry(b).unwrap().size.w, 960.);
    let _ = a;
}

#[test]
fn directional_move_reorders_siblings_and_stops_at_tree_edge() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.move_direction(b, Direction::Left));
    assert_eq!(t.geometry(b).unwrap().loc.x, 0.);
    assert_eq!(t.geometry(a).unwrap().loc.x, 600.);
    assert!(!t.move_direction(b, Direction::Left));
    t.check_invariants();
}

#[test]
fn directional_move_crosses_and_collapses_containers() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(b, Layout::SplitV);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.move_direction(c, Direction::Left));
    assert_eq!(t.geometry(c).unwrap().loc.x, 0.);
    assert!(t.geometry(a).unwrap().loc.x > 0.);
    assert!(t.geometry(b).unwrap().loc.x > t.geometry(a).unwrap().loc.x);
    t.check_invariants();
}

#[test]
fn directional_move_creates_an_implicit_container() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(a, Layout::SplitV);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.move_direction(c, Direction::Left));
    assert_eq!(t.geometry(c).unwrap().loc.x, 0.);
    assert!(t.geometry(a).unwrap().loc.x > 0.);
    assert_eq!(t.geometry(a).unwrap().loc.x, t.geometry(b).unwrap().loc.x);
    t.check_invariants();
}

#[test]
fn reordering_a_subtree_preserves_its_share() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.move_subtree_to_first(c));
    assert_eq!(t.geometry(c).unwrap().size.w, 300.);
    assert_eq!(t.geometry(a).unwrap().loc.x, 300.);
    assert_eq!(t.geometry(b).unwrap().loc.x, 900.);
    t.check_invariants();
}

#[test]
fn resizing_adjacent_siblings_changes_only_that_boundary() {
    let mut t = tree((1000., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.resize_adjacent(a, b, 0.1));
    assert_eq!(t.geometry(a).unwrap().size.w, 600.);
    assert_eq!(t.geometry(b).unwrap().size.w, 150.);
    assert_eq!(t.geometry(c).unwrap().size.w, 250.);
    assert!(!t.resize_adjacent(a, b, 0.6));
    t.check_invariants();
}

#[test]
fn fullscreen_and_maximize_survive_tree_mutations() {
    let mut t = tree((1920., 1080.), 0.);
    let first_window = TestWindow::new(1);
    let first_state = first_window.clone();
    let first = t.add_tile(
        Tile::new(
            first_window,
            t.view_size(),
            1.,
            Clock::with_time(Duration::ZERO),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.set_fullscreen(&1, true));
    assert_eq!(first_state.0.requested_mode.get(), SizingMode::Fullscreen);
    assert!(first_state.0.received_transaction.get());
    assert!(t.move_direction(first, Direction::Right));
    assert!(t.is_active_pending_fullscreen());
    assert_eq!(first_state.0.requested_mode.get(), SizingMode::Fullscreen);

    assert!(t.set_fullscreen(&1, false));
    assert!(t.set_maximized(&1, true));
    assert_eq!(first_state.0.requested_mode.get(), SizingMode::Maximized);
    assert!(t.move_subtree_to_first(first));
    assert_eq!(first_state.0.requested_mode.get(), SizingMode::Maximized);
    assert!(t.geometry(second).is_some());
    t.check_invariants();
}

#[test]
fn interactive_resize_uses_the_adjacent_sibling_boundary() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(1, crate::utils::ResizeEdge::RIGHT));
    assert!(t.interactive_resize_update(&1, Point::from((100., 0.))));
    assert_eq!(t.geometry(first).unwrap().size.w, 600.);
    assert_eq!(t.geometry(second).unwrap().size.w, 400.);
    t.refresh(true, true);
    assert_eq!(
        t.windows()
            .find(|(_, window)| window.id() == &1)
            .unwrap()
            .1
             .0
            .interactive_resize
            .get()
            .unwrap()
            .edges,
        crate::utils::ResizeEdge::RIGHT
    );
    t.interactive_resize_end(Some(&1));
    t.refresh(true, true);
    assert!(t
        .windows()
        .find(|(_, window)| window.id() == &1)
        .unwrap()
        .1
         .0
        .interactive_resize
        .get()
        .is_none());
    t.check_invariants();
}

#[test]
fn interactive_resize_finds_an_adjacent_ancestor_sibling() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);
    t.split(first, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(3, crate::utils::ResizeEdge::RIGHT));
    assert!(t.interactive_resize_update(&3, Point::from((100., 0.))));
    assert_eq!(t.geometry(first).unwrap().size.w, 600.);
    assert_eq!(t.geometry(third).unwrap().size.w, 600.);
    assert_eq!(t.geometry(second).unwrap().size.w, 400.);
    t.interactive_resize_end(None);
    t.check_invariants();
}

#[test]
fn refresh_dispatches_pending_configures() {
    let mut t = tree((800., 600.), 0.);
    let window = TestWindow::new(1);
    let state = window.clone();
    t.add_tile(
        Tile::new(
            window,
            t.view_size(),
            1.,
            Clock::with_time(Duration::ZERO),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );

    t.refresh(true, true);
    assert_eq!(state.0.configure_count.get(), 1);
}

#[test]
fn tabbed_split_only_exposes_the_focused_branch() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);
    t.split(first, Layout::Tabbed);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    let visible: Vec<_> = t
        .tiles_with_render_positions()
        .map(|(tile, _, visible)| (*tile.window().id(), visible))
        .collect();
    assert_eq!(visible, vec![(1, false), (2, true)]);
    t.set_focus(first);
    let visible: Vec<_> = t
        .tiles_with_render_positions()
        .map(|(tile, _, visible)| (*tile.window().id(), visible))
        .collect();
    assert_eq!(visible, vec![(1, true), (2, false)]);
    assert_eq!(t.geometry(first), t.geometry(second));
}

#[test]
fn open_animation_lifecycle_is_owned_by_the_tile() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(
        Tile::new(
            TestWindow::new(1),
            t.view_size(),
            1.,
            t.clock().clone(),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );

    assert!(!t.are_transitions_ongoing());
    assert!(t.start_open_animation(&1));
    assert!(t.are_transitions_ongoing());
    let mut clock = t.clock().clone();
    clock.set_complete_instantly(true);
    t.advance_animations();
    assert!(!t.are_transitions_ongoing());
}

#[test]
fn tab_indicator_animation_follows_tabbed_container_lifecycle() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(
        Tile::new(
            TestWindow::new(1),
            t.view_size(),
            1.,
            t.clock().clone(),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );
    t.split(first, Layout::Tabbed);
    t.add_tile(
        Tile::new(
            TestWindow::new(2),
            t.view_size(),
            1.,
            t.clock().clone(),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );

    t.update_render_elements(true, crate::layout::RenderLayer::Normal);
    assert!(t.are_transitions_ongoing());
    let mut clock = t.clock().clone();
    clock.set_complete_instantly(true);
    t.advance_animations();
    assert!(!t.are_transitions_ongoing());
}

#[test]
fn moving_a_window_starts_and_finishes_tile_movement() {
    let mut t = tree((1000., 800.), 0.);
    for id in 1..=2 {
        t.add_tile(
            Tile::new(
                TestWindow::new(id),
                t.view_size(),
                1.,
                t.clock().clone(),
                Rc::new(Options::default()),
            ),
            InsertTarget::Focused,
        );
    }

    assert!(!t.are_transitions_ongoing());
    assert!(t.move_left());
    assert!(t.are_transitions_ongoing());
    let mut clock = t.clock().clone();
    clock.set_complete_instantly(true);
    t.advance_animations();
    assert!(!t.are_transitions_ongoing());
}

#[test]
fn hit_testing_uses_visible_tile_positions() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert_eq!(
        t.window_under(Point::from((550., 100.)))
            .map(|(window, _)| *window.id()),
        Some(2)
    );
}

#[test]
fn ipc_layout_contains_the_tree_position() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    let positions: Vec<_> = t
        .tiles_with_ipc_layouts()
        .map(|(tile, layout)| (*tile.window().id(), layout.tile_pos_in_workspace_view))
        .collect();
    assert_eq!(positions[0].1, Some((0., 0.)));
    assert_eq!(positions[1].1, Some((500., 0.)));
}

#[test]
fn removing_a_tile_resizes_survivors_in_one_transaction() {
    let mut t = tree((1000., 800.), 0.);
    let first = TestWindow::new(1);
    let first_state = first.clone();
    t.add_tile(
        Tile::new(
            first,
            t.view_size(),
            1.,
            Clock::with_time(Duration::ZERO),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    first_state.0.received_transaction.set(false);

    assert!(t.remove_tile(&2, Transaction::new()).is_some());
    assert_eq!(
        first_state.0.requested_size.get(),
        Some(Size::from((1000, 800)))
    );
    assert!(first_state.0.received_transaction.get());
    t.check_invariants();
}

#[derive(Debug, Clone)]
enum Op {
    Add,
    Remove(usize),
    Split(usize, Layout),
    SetLayout(usize, Layout),
    FocusDirection(Direction),
    Move(usize, Direction),
    ReorderFirst(usize),
    ReorderIndex(usize, usize),
    ReorderLast(usize),
    Resize(usize, usize, f64),
    Fullscreen(usize, bool),
    Maximize(usize, bool),
    ResizeSession(usize, Direction, f64),
}

fn layout_strategy() -> impl Strategy<Value = Layout> {
    prop_oneof![
        Just(Layout::SplitH),
        Just(Layout::SplitV),
        Just(Layout::Tabbed),
        Just(Layout::Stacked),
    ]
}

fn direction_strategy() -> impl Strategy<Value = Direction> {
    prop_oneof![
        Just(Direction::Left),
        Just(Direction::Right),
        Just(Direction::Up),
        Just(Direction::Down),
    ]
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Add),
        (0..32usize).prop_map(Op::Remove),
        (0..32usize, layout_strategy()).prop_map(|(id, layout)| Op::Split(id, layout)),
        (0..32usize, layout_strategy()).prop_map(|(id, layout)| Op::SetLayout(id, layout)),
        direction_strategy().prop_map(Op::FocusDirection),
        (0..32usize, direction_strategy()).prop_map(|(id, direction)| Op::Move(id, direction)),
        (0..32usize).prop_map(Op::ReorderFirst),
        (0..32usize, 0..32usize).prop_map(|(id, index)| Op::ReorderIndex(id, index)),
        (0..32usize).prop_map(Op::ReorderLast),
        (0..32usize, 0..32usize, -0.9f64..0.9)
            .prop_map(|(first, second, delta)| Op::Resize(first, second, delta)),
        (0..32usize, any::<bool>()).prop_map(|(id, value)| Op::Fullscreen(id, value)),
        (0..32usize, any::<bool>()).prop_map(|(id, value)| Op::Maximize(id, value)),
        (0..32usize, direction_strategy(), -1000f64..1000.)
            .prop_map(|(id, direction, delta)| Op::ResizeSession(id, direction, delta)),
    ]
}

fn sync_ids(tree: &TilingTree<TestWindow>, ids: &mut Vec<NodeId>) {
    ids.retain(|id| tree.windows().any(|(candidate, _)| candidate == *id));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_none() { 0 } else { ProptestConfig::default().cases },
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_operations_preserve_invariants(ops in prop::collection::vec(op_strategy(), 0..100)) {
        let mut tree = tree((1920., 1080.), 8.);
        let mut ids = Vec::new();
        let mut next_window = 0;
        for op in ops {
            match op {
                Op::Add => {
                    ids.push(tree.add_tile(tile(next_window, tree.view_size()), InsertTarget::Focused));
                    next_window += 1;
                }
                Op::Remove(index) => {
                    if !ids.is_empty() {
                        let id = ids.remove(index % ids.len());
                        tree.remove_tile_node(id);
                    }
                }
                Op::Split(index, layout) => {
                    if !ids.is_empty() { tree.split(ids[index % ids.len()], layout); }
                }
                Op::SetLayout(index, layout) => {
                    let nodes: Vec<_> = tree.iter_depth_first().map(|(id, _)| id).collect();
                    if !nodes.is_empty() { tree.set_layout(nodes[index % nodes.len()], layout); }
                }
                Op::FocusDirection(direction) => { tree.focus_direction(direction); }
                Op::Move(index, direction) => {
                    if !ids.is_empty() { tree.move_direction(ids[index % ids.len()], direction); }
                }
                Op::ReorderFirst(id) => {
                    if !ids.is_empty() { tree.move_subtree_to_first(ids[id % ids.len()]); }
                }
                Op::ReorderIndex(id, index) => {
                    if !ids.is_empty() { tree.move_subtree_to_index(ids[id % ids.len()], index); }
                }
                Op::ReorderLast(id) => {
                    if !ids.is_empty() { tree.move_subtree_to_last(ids[id % ids.len()]); }
                }
                Op::Resize(first, second, delta) => {
                    if !ids.is_empty() {
                        tree.resize_adjacent(ids[first % ids.len()], ids[second % ids.len()], delta);
                    }
                }
                Op::Fullscreen(index, value) => {
                    if !ids.is_empty() {
                        let window = tree.windows().find(|(id, _)| *id == ids[index % ids.len()]).map(|(_, window)| *window.id());
                        if let Some(window) = window { tree.set_fullscreen(&window, value); }
                    }
                }
                Op::Maximize(index, value) => {
                    if !ids.is_empty() {
                        let window = tree.windows().find(|(id, _)| *id == ids[index % ids.len()]).map(|(_, window)| *window.id());
                        if let Some(window) = window { tree.set_maximized(&window, value); }
                    }
                }
                Op::ResizeSession(index, direction, delta) => {
                    if !ids.is_empty() {
                        let window = tree.windows().find(|(id, _)| *id == ids[index % ids.len()]).map(|(_, window)| *window.id());
                        if let Some(window) = window {
                            let edge = match direction {
                                Direction::Left => crate::utils::ResizeEdge::LEFT,
                                Direction::Right => crate::utils::ResizeEdge::RIGHT,
                                Direction::Up => crate::utils::ResizeEdge::TOP,
                                Direction::Down => crate::utils::ResizeEdge::BOTTOM,
                            };
                            if tree.interactive_resize_begin(window, edge) {
                                tree.interactive_resize_update(&window, Point::from((delta, delta)));
                                tree.interactive_resize_end(Some(&window));
                            }
                        }
                    }
                }
            }
            sync_ids(&tree, &mut ids);
            tree.check_invariants();
        }
    }
}

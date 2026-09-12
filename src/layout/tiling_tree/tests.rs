use std::cell::Cell;
use std::rc::Rc;

use proptest::prelude::*;

use super::*;
use crate::layout::{
    ConfigureIntent, InteractiveResizeData, LayoutElementRenderSnapshot, SizingMode,
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
            rules: ResolvedWindowRules::default(),
        }))
    }
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
        _: SizingMode,
        _: bool,
        _: Option<Transaction>,
    ) {
        self.0.requested_size.set(Some(size));
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
    fn send_pending_configure(&mut self) {}
    fn set_active_in_column(&mut self, _: bool) {}
    fn set_floating(&mut self, _: bool) {}
    fn sizing_mode(&self) -> SizingMode {
        SizingMode::Normal
    }
    fn pending_sizing_mode(&self) -> SizingMode {
        SizingMode::Normal
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
    fn set_interactive_resize(&mut self, _: Option<InteractiveResizeData>) {}
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
    let t: TilingTree<TestWindow> = TilingTree::new(Size::from((1920., 1080.)), 0.);
    assert!(t.is_empty());
    assert_eq!(t.focus(), None);
    t.check_invariants();
}

#[test]
fn one_window_fills_the_view() {
    let mut t = TilingTree::new(Size::from((1920., 1080.)), 0.);
    let id = t.add_window(TestWindow::new(1), InsertTarget::Focused);
    assert_eq!(
        t.geometry(id).unwrap(),
        Rectangle::from_size(Size::from((1920., 1080.)))
    );
    assert_eq!(t.focus(), Some(id));
    t.check_invariants();
}

#[test]
fn two_windows_split_h_halve_the_view() {
    let mut t = TilingTree::new(Size::from((1920., 1080.)), 0.);
    let a = t.add_window(TestWindow::new(1), InsertTarget::Focused);
    let b = t.add_window(TestWindow::new(2), InsertTarget::Focused);
    assert_eq!(t.geometry(a).unwrap().size.w, 960.);
    assert_eq!(t.geometry(b).unwrap().size.w, 960.);
    t.check_invariants();
}

#[test]
fn removing_a_sibling_collapses_the_implicit_container() {
    let mut t = TilingTree::new(Size::from((1920., 1080.)), 0.);
    let a = t.add_window(TestWindow::new(1), InsertTarget::Focused);
    let b = t.add_window(TestWindow::new(2), InsertTarget::Focused);
    t.split(b, Layout::SplitV);
    let c = t.add_window(TestWindow::new(3), InsertTarget::Focused);
    t.remove_window(c);
    t.check_invariants();
    assert_eq!(t.geometry(b).unwrap().size.w, 960.);
    let _ = a;
}

#[derive(Debug, Clone)]
enum Op {
    Add,
    Remove(usize),
    Split(usize, Layout),
    SetLayout(usize, Layout),
    FocusDirection(Direction),
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
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_none() { 0 } else { ProptestConfig::default().cases },
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_operations_preserve_invariants(ops in prop::collection::vec(op_strategy(), 0..100)) {
        let mut tree = TilingTree::new(Size::from((1920., 1080.)), 8.);
        let mut ids = Vec::new();
        let mut next_window = 0;
        for op in ops {
            match op {
                Op::Add => {
                    ids.push(tree.add_window(TestWindow::new(next_window), InsertTarget::Focused));
                    next_window += 1;
                }
                Op::Remove(index) => {
                    if !ids.is_empty() {
                        let id = ids.remove(index % ids.len());
                        tree.remove_window(id);
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
            }
            tree.check_invariants();
        }
    }
}

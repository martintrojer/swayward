use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use proptest::prelude::*;
use smithay::output::{self, Output};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Point, Serial, Transform};

use super::*;
use crate::animation::Clock;
use crate::layout::tile::Tile;
use crate::layout::{
    titlebar, ConfigureIntent, InteractiveResizeData, LayoutElementRenderSnapshot, Options,
    SizingMode,
};
use crate::render_helpers::offscreen::OffscreenData;
use crate::utils::transaction::Transaction;
use crate::window::ResolvedWindowRules;

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
    options.layout.border.off = false;
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
    fn title(&self) -> String {
        format!("window {}", self.0.id)
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
fn working_area_starts_at_physical_pixel() {
    let struts = swayward_config::Struts {
        left: swayward_config::FloatOrInt(0.5),
        right: swayward_config::FloatOrInt(1.),
        top: swayward_config::FloatOrInt(0.75),
        bottom: swayward_config::FloatOrInt(1.),
    };

    let parent_area = Rectangle::from_size(Size::from((1280., 720.)));
    let area = apply_struts(parent_area, 1., struts);

    assert_eq!(
        crate::utils::round_logical_in_physical(1., area.loc.x),
        area.loc.x
    );
    assert_eq!(
        crate::utils::round_logical_in_physical(1., area.loc.y),
        area.loc.y
    );
}

#[test]
fn large_fractional_strut() {
    let struts = swayward_config::Struts {
        left: swayward_config::FloatOrInt(0.),
        right: swayward_config::FloatOrInt(0.),
        top: swayward_config::FloatOrInt(50000.5),
        bottom: swayward_config::FloatOrInt(0.),
    };

    let parent_area = Rectangle::from_size(Size::from((1280., 720.)));
    let area = apply_struts(parent_area, 1., struts);

    assert_eq!(area.size.h, 0.);
}

#[test]
fn asymmetric_struts_move_the_tiled_window_from_the_left_and_top_edges() {
    let mut options = Options::default();
    options.layout.gaps = 0.;
    options.layout.struts = swayward_config::Struts {
        left: swayward_config::FloatOrInt(40.),
        right: swayward_config::FloatOrInt(0.),
        top: swayward_config::FloatOrInt(20.),
        bottom: swayward_config::FloatOrInt(0.),
    };
    let size = Size::from((1200., 800.));
    let mut t = TilingTree::new(
        size,
        Rectangle::from_size(size),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    );
    let id = t.add_tile(tile(1, size), InsertTarget::Focused);

    let window = t.geometry(id).unwrap();
    assert_eq!(window.loc, Point::from((40., 20.)));
    assert_eq!(window.size, Size::from((1160., 780.)));
    assert_eq!(window.loc.x + window.size.w, 1200.);
    assert_eq!(window.loc.y + window.size.h, 800.);
}

#[test]
fn struts_reduce_new_window_bounds() {
    let mut options = Options::default();
    options.layout.gaps = 0.;
    options.layout.border.off = true;
    options.layout.struts = swayward_config::Struts {
        left: swayward_config::FloatOrInt(40.),
        right: swayward_config::FloatOrInt(0.),
        top: swayward_config::FloatOrInt(20.),
        bottom: swayward_config::FloatOrInt(0.),
    };
    let size = Size::from((1200., 800.));
    let t = TilingTree::<TestWindow>::new(
        size,
        Rectangle::from_size(size),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    );

    assert_eq!(
        t.new_window_toplevel_bounds(&ResolvedWindowRules::default()),
        Size::from((1160, 780))
    );
}

#[test]
fn empty_tree_has_no_focus() {
    let t = tree((1920., 1080.), 0.);
    assert!(t.is_empty());
    assert_eq!(t.focus(), None);
    t.check_invariants();
}

#[test]
fn invariant_rejects_stale_and_duplicate_node_side_state() {
    for collection in 0..4 {
        let mut t = tree((1920., 1080.), 0.);
        let stale = NodeId(999);
        match collection {
            0 => t.focus_history.push(stale),
            1 | 2 => {
                t.previous_split_layouts.insert(stale, Layout::SplitV);
            }
            3 => {
                t.pending_modes.insert(
                    stale,
                    PendingMode {
                        fullscreen: true,
                        maximized: false,
                    },
                );
            }
            _ => unreachable!(),
        }
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            t.check_invariants();
        }))
        .is_err());
    }

    let mut t = tree((1920., 1080.), 0.);
    let leaf = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.focus_history.push(leaf);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        t.check_invariants();
    }))
    .is_err());
}

#[test]
fn removing_a_node_clears_every_node_side_collection() {
    let mut t = tree((1920., 1080.), 0.);
    let leaf = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.previous_split_layouts.insert(leaf, Layout::SplitH);
    t.pending_modes.insert(
        leaf,
        PendingMode {
            fullscreen: true,
            maximized: false,
        },
    );

    t.remove_tile_node(leaf);

    t.check_invariants();
}

#[test]
fn shipped_config_preserves_the_default_titlebar_geometry() {
    let config = swayward_config::Config::load_default();
    assert_eq!(config.layout.titlebar, swayward_config::Titlebar::default());
    assert_eq!(titlebar::height(1., &config.layout.titlebar), 22.);
}

#[test]
fn titlebar_padding_changes_derived_height() {
    let mut config = swayward_config::Titlebar::default();
    let default_height = titlebar::height(1., &config);
    config.vertical_padding += 3.;
    assert_eq!(titlebar::height(1., &config), default_height + 6.);
}

#[test]
fn titlebar_state_distinguishes_sway_color_classes() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);
    t.split(first, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);

    assert_eq!(
        t.titlebar_state(third, true),
        titlebar::TitlebarState::Focused
    );
    assert_eq!(
        t.titlebar_state(third, false),
        titlebar::TitlebarState::FocusedInactive
    );
    assert_eq!(
        t.titlebar_state(first, true),
        titlebar::TitlebarState::FocusedTabTitle
    );
    assert_eq!(
        t.titlebar_state(second, true),
        titlebar::TitlebarState::Unfocused
    );
}

#[test]
fn one_window_reserves_a_titlebar_above_its_content() {
    let mut t = tree((1920., 1080.), 0.);
    let id = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let rect = t.geometry(id).unwrap();
    assert_eq!(rect.loc.x, 0.);
    assert!(rect.loc.y > 0.);
    assert_eq!(rect.size.w, 1920.);
    assert_eq!(rect.loc.y + rect.size.h, 1080.);
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
fn split_on_an_empty_tree_sets_the_root_layout() {
    let mut t = tree((1200., 800.), 0.);

    t.split_focused(Layout::SplitV);

    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::SplitV,
            ..
        }
    ));
    t.check_invariants();
}

#[test]
fn split_on_a_nonempty_workspace_wraps_children_and_focuses_the_wrapper() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::SplitH);
    t.set_focus(t.root);

    t.split_focused(Layout::SplitV);

    let TreeNode::Split {
        layout,
        children,
        percents,
    } = &t.nodes[&t.root].value
    else {
        panic!("root must be a split");
    };
    assert_eq!(*layout, Layout::SplitV);
    assert_eq!(percents, &[1.]);
    let [wrapper] = children.as_slice() else {
        panic!("workspace must contain one wrapper");
    };
    assert_eq!(t.focus(), Some(*wrapper));
    assert!(matches!(
        &t.nodes[wrapper].value,
        TreeNode::Split {
            layout: Layout::SplitH,
            children,
            percents,
        } if children == &[first, second] && percents == &[0.5, 0.5]
    ));
    assert_eq!(t.nodes[&first].parent, Some(*wrapper));
    assert_eq!(t.nodes[&second].parent, Some(*wrapper));
    assert_eq!(t.ipc_tree().nodes().len(), 4);
    t.check_invariants();
}

#[test]
fn stacked_layout_wraps_a_single_workspace_leaf() {
    for layout in [Layout::Stacked, Layout::Tabbed] {
        let mut t = tree((1200., 800.), 0.);
        let leaf = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);

        t.set_focused_layout(layout);

        let TreeNode::Split {
            layout: root_layout,
            children,
            ..
        } = &t.nodes[&t.root].value
        else {
            panic!("root must be a split");
        };
        assert_eq!(*root_layout, Layout::SplitH);
        let [wrapper] = children.as_slice() else {
            panic!("workspace must contain one wrapper");
        };
        assert!(matches!(
            &t.nodes[wrapper].value,
            TreeNode::Split { layout: actual, children, .. }
                if *actual == layout && children == &[leaf]
        ));
        assert_eq!(t.nodes[&leaf].parent, Some(*wrapper));
        assert_eq!(t.focus(), Some(leaf));
        t.check_invariants();
    }
}

#[test]
fn split_retargets_a_singleton_split_parent() {
    for (parent_layout, requested_layout) in [
        (Layout::SplitH, Layout::SplitH),
        (Layout::SplitH, Layout::SplitV),
        (Layout::SplitV, Layout::SplitV),
        (Layout::SplitV, Layout::SplitH),
    ] {
        let mut t = tree((1200., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.set_layout(t.root, parent_layout);

        t.split(first, requested_layout);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

        assert_eq!(t.ipc_tree().nodes().len(), 3);
        assert!(matches!(
            t.nodes[&t.root].value,
            TreeNode::Split { layout, .. } if layout == requested_layout
        ));
        t.check_invariants();
    }
}

#[test]
fn splitting_a_container_preserves_focus() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);
    t.set_focus(first);

    t.split(first, Layout::SplitV);

    assert_eq!(t.focus(), Some(first));
    let wrapper = t.nodes[&first].parent.unwrap();
    assert_ne!(wrapper, t.root);
    assert!(matches!(
        t.nodes[&wrapper].value,
        TreeNode::Split {
            layout: Layout::SplitV,
            ..
        }
    ));
    assert_eq!(t.nodes[&second].parent, Some(t.root));
    t.check_invariants();
}

#[test]
fn splitting_an_unfocused_container_does_not_steal_focus() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);

    t.split(first, Layout::SplitV);

    assert_eq!(t.focus(), Some(second));
    t.check_invariants();
}

#[test]
fn repeating_split_on_a_singleton_parent_does_not_grow_the_tree() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::SplitV);

    for _ in 0..10 {
        t.split(first, Layout::SplitV);
    }
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert_eq!(t.ipc_tree().nodes().len(), 3);
    assert_eq!(t.nodes[&first].parent, Some(t.root));
    t.check_invariants();
}

#[test]
fn split_wraps_a_leaf_with_multiple_or_tabbed_siblings() {
    for parent_layout in [Layout::SplitH, Layout::SplitV, Layout::Tabbed] {
        for requested_layout in [Layout::SplitH, Layout::SplitV] {
            let mut t = tree((1200., 800.), 0.);
            let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
            t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
            t.set_layout(t.root, parent_layout);
            t.set_focus(first);

            t.split(first, requested_layout);
            let inserted = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

            let wrapper = t.nodes[&first].parent.unwrap();
            assert_ne!(wrapper, t.root);
            assert_eq!(t.nodes[&inserted].parent, Some(wrapper));
            assert_eq!(t.ipc_tree().nodes().len(), 5);
            assert!(matches!(
                t.nodes[&wrapper].value,
                TreeNode::Split { layout, .. } if layout == requested_layout
            ));
            t.check_invariants();
        }
    }
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
fn directional_focus_follows_parent_axis_and_wraps() {
    for (layout, backward, forward) in [
        (Layout::SplitH, Direction::Left, Direction::Right),
        (Layout::Tabbed, Direction::Left, Direction::Right),
        (Layout::SplitV, Direction::Up, Direction::Down),
        (Layout::Stacked, Direction::Up, Direction::Down),
    ] {
        let mut t = tree((1200., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        let middle = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        let last = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
        t.set_layout(t.root, layout);

        assert!(t.focus_direction(backward));
        assert_eq!(t.focus(), Some(middle));
        assert!(t.focus_direction(backward));
        assert_eq!(t.focus(), Some(first));
        assert!(t.focus_direction(backward));
        assert_eq!(t.focus(), Some(last));
        assert!(t.focus_direction(forward));
        assert_eq!(t.focus(), Some(first));
        t.check_invariants();
    }
}

#[test]
fn directional_focus_escalates_to_an_ancestor_and_descends_by_focus_history() {
    let mut t = tree((1200., 800.), 0.);
    let left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let top_right = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(top_right, Layout::SplitV);
    let bottom_right = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.focus_left());
    assert_eq!(t.focus(), Some(left));
    assert!(t.focus_right());
    assert_eq!(t.focus(), Some(bottom_right));
    t.set_focus(top_right);
    assert!(t.focus_down());
    assert_eq!(t.focus(), Some(bottom_right));
    t.check_invariants();
}

#[test]
fn parent_and_child_focus_walk_the_tree_and_layout_the_selected_parent() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let nested = t.nodes[&third].parent.unwrap();

    assert!(t.focus_parent());
    assert_eq!(t.focus(), Some(nested));
    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    assert!(matches!(
        &children[1],
        IpcNode::Split {
            id,
            focused: true,
            children,
            ..
        } if *id == nested && children.iter().all(|child| matches!(child, IpcNode::Leaf { focused: false, .. }))
    ));
    t.set_focused_layout(Layout::Tabbed);
    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::Tabbed,
            ..
        }
    ));
    assert!(matches!(
        t.nodes[&nested].value,
        TreeNode::Split {
            layout: Layout::SplitV,
            ..
        }
    ));
    assert!(t.focus_parent());
    assert_eq!(t.focus(), Some(t.root));
    assert!(!t.focus_parent());
    assert!(t.focus_child());
    assert_eq!(t.focus(), Some(nested));
    assert!(t.focus_child());
    assert_eq!(t.focus(), Some(third));
    assert!(!t.focus_child());
    assert!(t.geometry(first).is_some());
    t.check_invariants();
}

#[test]
fn layout_toggle_restores_the_previous_split_axis() {
    for previous in [Layout::SplitH, Layout::SplitV] {
        let mut t = tree((1200., 800.), 0.);
        t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.set_layout(t.root, previous);
        t.set_focused_layout(Layout::Tabbed);

        t.toggle_focused_layout_split();

        let wrapper = t.nodes[&t.focus().unwrap()].parent.unwrap();
        assert!(matches!(
            t.nodes[&wrapper].value,
            TreeNode::Split { layout, .. } if layout == previous
        ));
        t.check_invariants();
    }
}

#[test]
fn layout_toggle_targets_the_parent_and_flattens_one_singleton_ancestor() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let focused = t.alloc(Node {
        parent: None,
        value: TreeNode::Split {
            layout: Layout::SplitV,
            children: vec![first, second],
            percents: vec![0.5, 0.5],
        },
    });
    let parent = t.alloc(Node {
        parent: None,
        value: TreeNode::Split {
            layout: Layout::Stacked,
            children: vec![focused],
            percents: vec![1.],
        },
    });
    let grandparent = t.alloc(Node {
        parent: Some(t.root),
        value: TreeNode::Split {
            layout: Layout::SplitV,
            children: vec![parent],
            percents: vec![1.],
        },
    });
    t.nodes.get_mut(&first).unwrap().parent = Some(focused);
    t.nodes.get_mut(&second).unwrap().parent = Some(focused);
    t.nodes.get_mut(&focused).unwrap().parent = Some(parent);
    t.nodes.get_mut(&parent).unwrap().parent = Some(grandparent);
    t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
        layout: Layout::SplitV,
        children: vec![grandparent],
        percents: vec![1.],
    };
    t.set_focus(focused);

    t.toggle_focused_layout_split();

    assert_eq!(t.ipc_tree().nodes().len(), 5);
    assert!(!t.nodes.contains_key(&parent));
    assert!(t.nodes.contains_key(&focused));
    assert_eq!(t.nodes[&focused].parent, Some(grandparent));
    assert!(matches!(
        t.nodes[&grandparent].value,
        TreeNode::Split {
            layout: Layout::SplitH,
            ..
        }
    ));
    assert!(matches!(
        t.nodes[&focused].value,
        TreeNode::Split {
            layout: Layout::SplitV,
            ..
        }
    ));
}

#[test]
fn focus_child_uses_the_most_recent_descendant() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let nested = t.nodes[&third].parent.unwrap();
    t.set_focus(second);

    assert!(t.focus_parent());
    assert_eq!(t.focus(), Some(nested));
    assert!(t.focus_child());
    assert_eq!(t.focus(), Some(second));
    assert!(t.geometry(first).is_some());
    t.check_invariants();
}

#[test]
fn collapse_squashes_redundant_perpendicular_singleton_pairs() {
    for (grandparent_layout, container_layout, child_layout, should_squash) in [
        (Layout::SplitH, Layout::SplitV, Layout::SplitH, true),
        (Layout::Tabbed, Layout::SplitV, Layout::SplitH, true),
        (Layout::SplitV, Layout::SplitH, Layout::SplitV, true),
        (Layout::Stacked, Layout::SplitH, Layout::SplitV, true),
        (Layout::SplitH, Layout::SplitH, Layout::SplitV, false),
        (Layout::SplitV, Layout::SplitV, Layout::SplitH, false),
        (Layout::SplitV, Layout::SplitH, Layout::SplitH, false),
    ] {
        let mut t = tree((1200., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        let child = t.alloc(Node {
            parent: None,
            value: TreeNode::Split {
                layout: child_layout,
                children: vec![first, second],
                percents: vec![0.5, 0.5],
            },
        });
        let container = t.alloc(Node {
            parent: Some(t.root),
            value: TreeNode::Split {
                layout: container_layout,
                children: vec![child],
                percents: vec![1.],
            },
        });
        t.nodes.get_mut(&first).unwrap().parent = Some(child);
        t.nodes.get_mut(&second).unwrap().parent = Some(child);
        t.nodes.get_mut(&child).unwrap().parent = Some(container);
        t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
            layout: grandparent_layout,
            children: vec![container],
            percents: vec![1.],
        };

        t.compact_tree();

        let expected_nodes = if should_squash { 3 } else { 5 };
        assert_eq!(
            t.ipc_tree().nodes().len(),
            expected_nodes,
            "grandparent={grandparent_layout:?}, container={container_layout:?}, child={child_layout:?}"
        );
    }
}

#[test]
fn opening_a_window_preserves_intentional_nested_splits() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);
    t.split(first, Layout::SplitV);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);
    t.split(first, Layout::SplitH);
    t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);

    assert_eq!(t.ipc_tree().nodes().len(), 7);
}

#[test]
fn directional_move_squashes_the_whole_tree() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let child = t.alloc(Node {
        parent: None,
        value: TreeNode::Split {
            layout: Layout::SplitH,
            children: vec![first, second],
            percents: vec![0.5, 0.5],
        },
    });
    let container = t.alloc(Node {
        parent: Some(t.root),
        value: TreeNode::Split {
            layout: Layout::SplitV,
            children: vec![child],
            percents: vec![1.],
        },
    });
    t.nodes.get_mut(&first).unwrap().parent = Some(child);
    t.nodes.get_mut(&second).unwrap().parent = Some(child);
    t.nodes.get_mut(&child).unwrap().parent = Some(container);
    t.nodes.get_mut(&third).unwrap().parent = Some(t.root);
    t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
        layout: Layout::SplitH,
        children: vec![container, third],
        percents: vec![0.5, 0.5],
    };

    assert!(t.move_direction(third, Direction::Left));

    assert_eq!(t.ipc_tree().nodes().len(), 4);
    t.check_invariants();
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
fn consume_wraps_siblings_and_expel_lifts_the_window() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.consume(second, true));
    let parent = t.nodes.get(&second).unwrap().parent.unwrap();
    assert_eq!(t.nodes.get(&third).unwrap().parent, Some(parent));
    assert_ne!(parent, t.root);
    assert_eq!(
        t.geometry(second).unwrap().loc.x,
        t.geometry(third).unwrap().loc.x
    );
    assert!(t.geometry(second).unwrap().loc.y > t.geometry(third).unwrap().loc.y);

    assert!(t.expel(second, true));
    assert_eq!(t.nodes.get(&second).unwrap().parent, Some(t.root));
    assert!(t.geometry(second).unwrap().loc.x > t.geometry(third).unwrap().loc.x);
    assert!(t.geometry(first).is_some());
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
fn axis_resize_compensates_every_sibling() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let fourth = t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);

    let TreeNode::Split { percents, .. } = &mut t.nodes.get_mut(&t.root).unwrap().value else {
        panic!("root must be a split");
    };
    percents.fill(0.25);
    t.set_window_width(Some(&4), SizeChange::AdjustProportion(25.));

    for id in [first, second, third] {
        assert!((t.geometry(id).unwrap().size.w - 1000. / 6.).abs() < 1e-9);
    }
    assert!((t.geometry(fourth).unwrap().size.w - 500.).abs() < 1e-9);
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
fn directional_resize_skips_an_unusable_same_axis_boundary() {
    let mut t = tree((1000., 800.), 0.);
    let left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let upper_right = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(upper_right, Layout::SplitV);
    let middle_right = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);
    assert!(t.focus_parent());
    let right = t.focus().unwrap();
    t.split(right, Layout::SplitH);
    t.add_tile(tile(5, t.view_size()), InsertTarget::Focused);
    t.set_focus(middle_right);

    t.resize_window_edge(
        Some(&3),
        crate::utils::ResizeEdge::LEFT,
        SizeChange::AdjustProportion(25.),
    );

    let right_branch = t.nodes[&right].parent.unwrap();
    assert_eq!(t.nodes[&left].parent, Some(t.root));
    assert_eq!(t.nodes[&right_branch].parent, Some(t.root));
    assert_eq!(t.sibling_percents(left, right_branch), Some((0.25, 0.75)));
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
    let titlebar_height = titlebar::height(1., &swayward_config::Titlebar::default());
    assert!(t.geometry(first).unwrap().loc.y > 0.);
    assert!(t.geometry(first).unwrap().size.h < 800.);
    let first_bar = t.ipc_decoration_rect(&1).unwrap();
    let second_bar = t.ipc_decoration_rect(&2).unwrap();
    assert_eq!(first_bar.size, second_bar.size);
    assert_eq!(first_bar.size.h, titlebar_height);
    assert_eq!(first_bar.loc.y, 0.);
    assert!(second_bar.loc.x > first_bar.loc.x);
}

#[test]
fn stacked_split_reserves_one_titlebar_row_per_child() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::Stacked);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    let first_bar = t.ipc_decoration_rect(&1).unwrap();
    let second_bar = t.ipc_decoration_rect(&2).unwrap();
    assert_eq!(first_bar.size.w, 1000.);
    assert_eq!(second_bar.loc.y, first_bar.loc.y + first_bar.size.h);
    assert_eq!(t.geometry(first).unwrap().loc.y, first_bar.size.h * 2.);
    let (window, hit) = t
        .window_under(Point::from((100., first_bar.size.h + 1.)))
        .unwrap();
    assert_eq!(*window.id(), 2);
    assert_eq!(
        hit,
        HitType::Activate {
            is_tab_indicator: true
        }
    );
}

#[test]
fn fullscreen_suppresses_titlebar() {
    let mut t = tree((1000., 800.), 0.);
    let id = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    assert!(t.ipc_decoration_rect(&1).is_some());
    assert!(t.set_fullscreen(&1, true));
    assert!(t.ipc_decoration_rect(&1).is_none());
    assert_eq!(t.geometry(id).unwrap().loc.y, 0.);
}

#[test]
fn titlebar_hit_targets_the_corresponding_tab() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::Tabbed);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    let (window, hit) = t.window_under(Point::from((100., 5.))).unwrap();
    assert_eq!(*window.id(), 1);
    assert_eq!(
        hit,
        HitType::Activate {
            is_tab_indicator: true
        }
    );
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
    let titlebar_height = titlebar::height(1., &swayward_config::Titlebar::default());
    assert_eq!(positions[0].1, Some((0., titlebar_height)));
    assert_eq!(positions[1].1, Some((500., titlebar_height)));
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
        Some(Size::from((
            992,
            800 - titlebar::height(1., &swayward_config::Titlebar::default()) as i32 - 8
        )))
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
    FocusParent,
    FocusChild,
    Move(usize, Direction),
    ReorderFirst(usize),
    ReorderIndex(usize, usize),
    ReorderLast(usize),
    Resize(usize, usize, f64),
    Fullscreen(usize, bool),
    Maximize(usize, bool),
    ResizeSession(usize, Direction, f64),
    Consume(usize, bool),
    Expel(usize, bool),
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
        Just(Op::FocusParent),
        Just(Op::FocusChild),
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
        (0..32usize, any::<bool>()).prop_map(|(id, right)| Op::Consume(id, right)),
        (0..32usize, any::<bool>()).prop_map(|(id, right)| Op::Expel(id, right)),
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
                Op::FocusParent => { tree.focus_parent(); }
                Op::FocusChild => { tree.focus_child(); }
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
                Op::Consume(index, right) => {
                    if !ids.is_empty() { tree.consume(ids[index % ids.len()], right); }
                }
                Op::Expel(index, right) => {
                    if !ids.is_empty() { tree.expel(ids[index % ids.len()], right); }
                }
            }
            sync_ids(&tree, &mut ids);
            tree.check_invariants();
        }
    }
}

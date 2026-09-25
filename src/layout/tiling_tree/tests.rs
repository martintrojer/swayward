use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use proptest::prelude::*;
use smithay::output::{self, Output};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Point, Serial, Transform};
use swayward_ipc::command::BorderStyle;

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
    has_xdg_decoration: Cell<bool>,
    server_side_decoration_requested: Cell<Option<bool>>,
    rules: ResolvedWindowRules,
}

#[derive(Debug, Clone)]
struct TestWindow(Rc<TestWindowInner>);

impl TestWindow {
    fn new(id: usize) -> Self {
        Self::with_rules(id, ResolvedWindowRules::default())
    }

    fn with_rules(id: usize, rules: ResolvedWindowRules) -> Self {
        Self(Rc::new(TestWindowInner {
            id,
            size: Cell::new(Size::from((100, 200))),
            requested_size: Cell::new(None),
            requested_mode: Cell::new(SizingMode::Normal),
            configure_count: Cell::new(0),
            received_transaction: Cell::new(false),
            interactive_resize: Cell::new(None),
            has_xdg_decoration: Cell::new(false),
            server_side_decoration_requested: Cell::new(None),
            rules,
        }))
    }
}

fn tree_with_options(
    size: (f64, f64),
    gaps: f64,
    update: impl FnOnce(&mut Options),
) -> TilingTree<TestWindow> {
    let mut options = Options::default();
    options.layout.gaps = gaps;
    options.layout.border.off = false;
    update(&mut options);
    let size = Size::from(size);
    TilingTree::new(
        size,
        Rectangle::from_size(size),
        false,
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    )
}

fn tree(size: (f64, f64), gaps: f64) -> TilingTree<TestWindow> {
    tree_with_options(size, gaps, |_| {})
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

#[test]
fn popup_target_uses_its_nested_leaf_allocation() {
    let mut t = tree((1200., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let upper = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(upper, Layout::SplitV);
    let lower = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    let leaf = t.geometry(lower).unwrap();
    let popup = t.popup_target_rect(&3).unwrap();

    assert_eq!(
        popup.loc.y,
        leaf.loc.y + t.tile(lower).unwrap().window_loc().y
    );
    assert_eq!(popup.size.h, t.tile(lower).unwrap().window_size().h);
}

#[test]
fn tile_resolves_toggle_rule_before_storing_border_state() {
    let rules = ResolvedWindowRules {
        sway_border: Some(BorderStyle::Toggle),
        ..Default::default()
    };
    let tile = Tile::new(
        TestWindow::with_rules(1, rules),
        Size::from((500., 500.)),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );

    assert_eq!(tile.sway_border(), (BorderStyle::None, 0));
}

#[test]
fn tile_activation_region_contains_only_server_decorations() {
    let mut options = Options::default();
    options.layout.border.off = false;
    let tile = Tile::new(
        TestWindow::new(1),
        Size::from((500., 500.)),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    );
    let border = tile.effective_border_width().unwrap();
    let window = tile.window_loc();
    assert!(matches!(
        tile.hit((window.x - border / 2., window.y + 10.).into()),
        Some(HitType::Activate {
            is_tab_indicator: false
        })
    ));
    assert!(matches!(
        tile.hit((window.x + 10., window.y + 10.).into()),
        Some(HitType::Input { .. })
    ));
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
    fn is_in_input_region(&self, point: Point<f64, Logical>) -> bool {
        Rectangle::from_size(self.size()).to_f64().contains(point)
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
    fn has_xdg_decoration(&self) -> bool {
        self.0.has_xdg_decoration.get()
    }
    fn request_server_decoration(&mut self, server_side: bool) {
        self.0
            .server_side_decoration_requested
            .set(Some(server_side));
    }
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
        false,
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
        false,
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
fn structural_moves_preserve_unfocused_window_order() {
    let mut t = tree((1200., 800.), 0.);
    for id in 1..=4 {
        t.add_tile(tile(id, t.view_size()), InsertTarget::Focused);
    }
    for id in [4, 3, 2, 1] {
        let node = t.node_for_window(&id).unwrap();
        t.set_focus(node);
    }

    let third = t.node_for_window(&3).unwrap();
    assert!(t.move_node_direction(third, Direction::Up));
    assert_eq!(
        t.focus_history
            .iter()
            .filter_map(|node| t.tile(*node).map(|tile| *tile.window().id()))
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );

    let fourth = t.node_for_window(&4).unwrap();
    let second = t.node_for_window(&2).unwrap();
    assert!(t.move_subtree_to_node(fourth, second));
    assert_eq!(
        t.focus_history
            .iter()
            .filter_map(|node| t.tile(*node).map(|tile| *tile.window().id()))
            .collect::<Vec<_>>(),
        [1, 4, 2, 3]
    );
}

#[test]
fn restoring_a_removed_windows_focus_rank_preserves_close_order() {
    let mut t = tree((1200., 800.), 0.);
    for id in 1..=5 {
        t.add_tile(tile(id, t.view_size()), InsertTarget::Focused);
    }
    assert_eq!(
        t.focus_history
            .iter()
            .filter_map(|node| t.tile(*node).map(|tile| *tile.window().id()))
            .collect::<Vec<_>>(),
        [5, 4, 3, 2, 1]
    );

    let rank = t.focus_rank_for_window(&4).unwrap();
    let removed = t.remove_tile(&4, Transaction::new()).unwrap();
    t.add_tile_with_activation(removed, InsertTarget::Focused, false);
    t.restore_focus_rank(&4, rank);

    assert_eq!(
        t.focus_history
            .iter()
            .filter_map(|node| t.tile(*node).map(|tile| *tile.window().id()))
            .collect::<Vec<_>>(),
        [5, 4, 3, 2, 1]
    );
    for expected in [4, 3, 2, 1] {
        let focused = t.active_window().unwrap().id().to_owned();
        t.remove_tile(&focused, Transaction::new()).unwrap();
        assert_eq!(t.active_window().unwrap().id(), &expected);
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
fn invariant_rejects_stale_and_duplicate_node_side_state() {
    for collection in 0..5 {
        let mut t = tree((1920., 1080.), 0.);
        let stale = NodeId(999);
        match collection {
            0 => t.focus_history.push(stale),
            1 => {
                t.previous_split_layouts.insert(stale, Layout::SplitV);
            }
            2 => {
                t.title_formats.insert(stale, "custom".into());
            }
            3 | 4 => {
                t.pending_modes.insert(
                    stale,
                    PendingMode {
                        fullscreen: Some(FullscreenMode::Workspace),
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
fn invariant_rejects_non_positive_percentages() {
    let mut t = tree((1920., 1080.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let TreeNode::Split { percents, .. } = &mut t.nodes.get_mut(&t.root).unwrap().value else {
        unreachable!();
    };
    *percents = vec![1.5, -0.5];

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
    t.title_formats.insert(leaf, "custom".into());
    t.pending_modes.insert(
        leaf,
        PendingMode {
            fullscreen: Some(FullscreenMode::Workspace),
            maximized: false,
        },
    );
    t.tab_active.insert(leaf, leaf);
    t.tab_indicators
        .insert(leaf, TabIndicator::new(t.options.layout.tab_indicator));

    t.remove_tile_node(leaf);

    assert!(!t.title_formats.contains_key(&leaf));
    assert!(!t.tab_active.contains_key(&leaf));
    assert!(!t.tab_indicators.contains_key(&leaf));
    t.check_invariants();
}

#[test]
fn shipped_config_uses_only_sway_titlebars_for_tabs() {
    let config = swayward_config::Config::load_default();
    assert!(config.layout.tab_indicator.off);

    // The shipped titlebar ring is the top edge of the window outline, so it
    // matches the border below it: same width, and per focus state the same
    // colour. The remaining titlebar colors are an explicit part of the
    // shipped theme and need not match sway's compiled defaults.
    let shipped = &config.layout.titlebar;
    let border = &config.layout.border;
    assert_eq!(f64::from(shipped.border_thickness), border.width);
    assert_eq!(shipped.focused.border_color, border.active_color);
    for inactive in [
        shipped.focused_inactive,
        shipped.unfocused,
        shipped.focused_tab_title,
    ] {
        assert_eq!(inactive.border_color, border.inactive_color);
    }
    assert_eq!(shipped.urgent.border_color, border.urgent_color);
    let mut t = tree_with_options((1000., 800.), 0., |options| {
        options.layout = config.layout.clone();
    });
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::Tabbed);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.update_render_elements(true, crate::layout::RenderLayer::Normal);
    assert_eq!(t.compute_geometry().titlebars.len(), 2);
    assert!(t.tab_indicators.values().all(TabIndicator::is_empty));

    // The height is measured text plus padding, and `titlebar::height` asks
    // pango to lay out real glyphs, so the text part depends on which fonts are
    // installed: this machine measures 14px for "Mg" at monospace 10 and the CI
    // runner measures 13. Assert the relation the shipped config is responsible
    // for -- that it contributes exactly `vertical_padding` twice -- rather than
    // a constant that pins someone else's font stack.
    let padding = config.layout.titlebar.vertical_padding;
    let measured = titlebar::height(1., &config.layout.titlebar) - padding * 2.;
    assert_eq!(padding, 8.);
    assert!(
        (13. ..=15.).contains(&measured),
        "measured text height {measured} is outside the plausible range for \
         monospace 10; the shipped titlebar config or the font lookup changed"
    );
}

#[test]
fn border_toggle_enters_csd_when_xdg_decoration_is_present() {
    let window = TestWindow::new(1);
    window.0.has_xdg_decoration.set(true);
    let mut tile = Tile::new(
        window.clone(),
        Size::from((100., 200.)),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
    );

    tile.set_sway_border(BorderStyle::None, None, true).unwrap();
    tile.set_sway_border(BorderStyle::Toggle, None, true)
        .unwrap();
    tile.set_sway_border(BorderStyle::Toggle, None, true)
        .unwrap();
    tile.set_sway_border(BorderStyle::Toggle, None, true)
        .unwrap();

    assert_eq!(tile.sway_border(), (BorderStyle::Csd, 2));
    assert_eq!(window.0.server_side_decoration_requested.get(), Some(false));
    assert!(!tile.has_sway_titlebar());
    assert_eq!(tile.effective_border_width(), None);

    tile.set_sway_border(BorderStyle::Toggle, None, true)
        .unwrap();
    assert_eq!(tile.sway_border(), (BorderStyle::None, 0));
    assert_eq!(window.0.server_side_decoration_requested.get(), Some(true));
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
fn nested_container_titlebars_show_the_tree_and_update_after_close() {
    for parent_layout in [Layout::Tabbed, Layout::Stacked] {
        let mut t = tree((1200., 800.), 0.);
        let plain = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        let outer_first = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.set_layout(t.root, parent_layout);
        t.split(outer_first, Layout::SplitV);
        let inner_first = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
        t.split(inner_first, Layout::SplitH);
        let inner_second = t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);
        let inner = t.nodes[&inner_first].parent.unwrap();
        t.set_layout(inner, Layout::Tabbed);

        let geometry = t.compute_geometry();
        assert_eq!(geometry.titlebars[&plain].title, "window 1");
        let outer = t.nodes[&outer_first].parent.unwrap();
        assert_eq!(
            geometry.titlebars[&outer].title,
            "V[window 2 T[window 3 window 4]]"
        );

        t.remove_tile_node(inner_second);
        assert_eq!(
            t.compute_geometry().titlebars[&outer].title,
            "V[window 2 window 3]"
        );
    }
}

#[test]
fn split_children_below_a_parent_strip_draw_their_own_titlebars() {
    for parent_layout in [Layout::Tabbed, Layout::Stacked] {
        let mut t = tree((1200., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.set_layout(t.root, parent_layout);
        t.split(first, Layout::SplitH);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

        let geometry = t.compute_geometry();
        let titles = geometry
            .titlebars
            .values()
            .map(|titlebar| titlebar.title.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            titles,
            HashSet::from(["H[window 1 window 2]", "window 1", "window 2"])
        );
    }
}

#[test]
fn tabbed_border_covers_only_the_top_edge_uncovered_by_its_own_titlebar() {
    let assert_spans = |t: &TilingTree<TestWindow>, id, expected: &[(f64, f64)]| {
        let geometry = t.compute_geometry();
        let parts = geometry
            .uncovered_top_borders
            .get(&id)
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let spans = parts
            .iter()
            .map(|rect| (rect.loc.x, rect.size.w))
            .collect::<Vec<_>>();
        assert_eq!(spans, expected);
        if let Some(titlebar) = geometry.titlebars.get(&id) {
            for rect in parts {
                assert_eq!(rect.size.h, 4.);
                assert_eq!(
                    rect.loc.y + rect.size.h,
                    titlebar.rect.loc.y + titlebar.rect.size.h
                );
            }
        }
    };

    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let middle = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let last = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);

    t.set_focus(first);
    assert_spans(&t, first, &[(400., 800.)]);
    t.set_focus(middle);
    assert_spans(&t, middle, &[(0., 400.), (800., 400.)]);
    t.set_focus(last);
    assert_spans(&t, last, &[(0., 800.)]);

    t.set_layout(t.root, Layout::Stacked);
    assert_spans(&t, last, &[]);

    let mut nested = tree((1200., 800.), 0.);
    let left = nested.add_tile(tile(1, nested.view_size()), InsertTarget::Focused);
    nested.set_layout(nested.root, Layout::Tabbed);
    nested.split(left, Layout::SplitH);
    let right = nested.add_tile(tile(2, nested.view_size()), InsertTarget::Focused);
    nested.add_tile_to_existing_parent(tile(3, nested.view_size()), nested.root, false);
    nested.set_focus(left);
    assert_spans(&nested, left, &[]);
    assert_spans(&nested, right, &[]);
}

#[test]
fn uncovered_top_border_setting_does_not_change_geometry() {
    let geometry = |enabled| {
        let mut t = tree_with_options((1200., 800.), 0., |options| {
            options.layout.draw_uncovered_top_border = enabled;
        });
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.set_layout(t.root, Layout::Tabbed);
        t.set_focus(first);
        t.compute_geometry()
    };
    let enabled = geometry(true);
    let disabled = geometry(false);

    assert!(!enabled.uncovered_top_borders.is_empty());
    assert!(disabled.uncovered_top_borders.is_empty());
    let values = |map: HashMap<NodeId, Rectangle<f64, Logical>>| {
        let mut values = map.into_values().collect::<Vec<_>>();
        values.sort_by_key(|rect| (rect.loc.x as i64, rect.loc.y as i64));
        values
    };
    assert_eq!(values(enabled.leaf_boxes), values(disabled.leaf_boxes));
    assert_eq!(
        values(enabled.leaf_contents),
        values(disabled.leaf_contents)
    );
    assert_eq!(
        values(enabled.leaf_ipc_rects),
        values(disabled.leaf_ipc_rects)
    );
    assert_eq!(values(enabled.ipc_nodes), values(disabled.ipc_nodes));
}

#[test]
fn normal_titlebar_keeps_the_decorated_box_radius() {
    let configured = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    let mut t = tree((1200., 800.), 0.);
    let window = TestWindow::with_rules(
        1,
        ResolvedWindowRules {
            geometry_corner_radius: Some(configured),
            clip_to_geometry: Some(true),
            ..Default::default()
        },
    );
    let id = t.add_tile(
        Tile::new(
            window,
            t.view_size(),
            1.,
            t.clock().clone(),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );

    let geometry = t.compute_geometry();
    assert!(geometry.titlebars.contains_key(&id));
    assert!(geometry.titlebar_attached.contains(&id));
    t.update_render_elements(true, crate::layout::RenderLayer::Normal);
    assert_eq!(
        t.tile(id).unwrap().geometry_corner_radius(),
        swayward_config::CornerRadius {
            top_left: 0.,
            top_right: 0.,
            ..configured
        }
    );
    assert_eq!(
        t.tile(id).unwrap().window().geometry_corner_radius(),
        configured
    );
}

#[test]
fn border_owns_the_decorated_box_radius() {
    let configured = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    let mut t = tree((1200., 800.), 0.);
    let window = TestWindow::with_rules(
        1,
        ResolvedWindowRules {
            geometry_corner_radius: Some(configured),
            clip_to_geometry: Some(true),
            ..Default::default()
        },
    );
    let id = t.add_tile(
        Tile::new(
            window,
            t.view_size(),
            1.,
            t.clock().clone(),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );

    t.update_render_elements(true, crate::layout::RenderLayer::Normal);

    let tile = t.tile(id).unwrap();
    let border_radius = tile.decoration_corner_radii().0.unwrap();
    assert_eq!(
        border_radius,
        swayward_config::CornerRadius {
            top_left: 0.,
            top_right: 0.,
            ..configured
        }
    );
}

#[test]
fn decorations_use_the_tile_resolved_radius() {
    let configured = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    let make_tile = |t: &TilingTree<TestWindow>, id, radius, style| {
        let window = TestWindow::with_rules(
            id,
            ResolvedWindowRules {
                geometry_corner_radius: Some(radius),
                clip_to_geometry: Some(true),
                ..Default::default()
            },
        );
        if style == BorderStyle::Csd {
            window.0.has_xdg_decoration.set(true);
        }
        let mut tile = Tile::new(
            window,
            t.view_size(),
            1.,
            t.clock().clone(),
            Rc::new(Options::default()),
        );
        tile.set_sway_border(style, None, style == BorderStyle::Csd)
            .unwrap();
        tile
    };
    let assert_radii = |t: &TilingTree<TestWindow>, id| {
        let tile = t.tile(id).unwrap();
        let resolved = tile.geometry_corner_radius();
        let (border, shadow) = tile.decoration_corner_radii();
        let expected_border = tile.effective_border_width().map(|_| resolved);
        assert_eq!(border, expected_border);
        assert_eq!(shadow, expected_border.unwrap_or(resolved));
    };

    // A normal border supplies its own titlebar.
    let mut t = tree((1200., 800.), 0.);
    let id = t.add_tile(
        make_tile(&t, 1, configured, BorderStyle::Normal),
        InsertTarget::Focused,
    );
    t.update_render_elements(true, crate::layout::RenderLayer::Normal);
    assert_radii(&t, id);

    // A parent tab strip supplies the titlebar for titleless border styles.
    for layout in [Layout::Tabbed, Layout::Stacked] {
        for style in [BorderStyle::Pixel, BorderStyle::None, BorderStyle::Csd] {
            let mut t = tree((1200., 800.), 0.);
            let id = t.add_tile(make_tile(&t, 1, configured, style), InsertTarget::Focused);
            t.set_layout(t.root, layout);
            t.update_render_elements(true, crate::layout::RenderLayer::Normal);
            assert_radii(&t, id);
        }
    }

    // Nested tab strips use the same outer radius as workspace-level tabs.
    let mut t = tree((1200., 800.), 0.);
    let id = t.add_tile(
        make_tile(&t, 1, configured, BorderStyle::Pixel),
        InsertTarget::Focused,
    );
    t.split(id, Layout::SplitH);
    t.add_tile(
        make_tile(&t, 2, configured, BorderStyle::Pixel),
        InsertTarget::Focused,
    );
    let parent = t.nodes[&id].parent.unwrap();
    t.set_layout(parent, Layout::Tabbed);
    t.set_focus(id);
    t.update_render_elements(true, crate::layout::RenderLayer::Normal);
    assert_radii(&t, id);

    // Expanding a square radius must not create rounded decorations.
    let mut t = tree((1200., 800.), 0.);
    let id = t.add_tile(
        make_tile(&t, 1, 0f32.into(), BorderStyle::Normal),
        InsertTarget::Focused,
    );
    t.update_render_elements(true, crate::layout::RenderLayer::Normal);
    assert_radii(&t, id);
}

#[test]
fn titleless_borders_keep_the_window_top_corners() {
    let configured = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    for style in [BorderStyle::Pixel, BorderStyle::None] {
        let mut t = tree((1200., 800.), 0.);
        let window = TestWindow::with_rules(
            1,
            ResolvedWindowRules {
                geometry_corner_radius: Some(configured),
                clip_to_geometry: Some(true),
                ..Default::default()
            },
        );
        let mut tile = Tile::new(
            window,
            t.view_size(),
            1.,
            t.clock().clone(),
            Rc::new(Options::default()),
        );
        tile.set_sway_border(style, None, false).unwrap();
        let id = t.add_tile(tile, InsertTarget::Focused);

        let geometry = t.compute_geometry();
        assert!(!geometry.titlebars.contains_key(&id), "{style:?}");
        assert!(!geometry.titlebar_attached.contains(&id), "{style:?}");
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);
        assert_eq!(
            t.tile(id).unwrap().geometry_corner_radius(),
            configured,
            "{style:?}"
        );
    }

    let mut t = tree((1200., 800.), 0.);
    let window = TestWindow::with_rules(
        1,
        ResolvedWindowRules {
            geometry_corner_radius: Some(configured),
            clip_to_geometry: Some(true),
            ..Default::default()
        },
    );
    window.0.has_xdg_decoration.set(true);
    let mut tile = Tile::new(
        window,
        t.view_size(),
        1.,
        t.clock().clone(),
        Rc::new(Options::default()),
    );
    tile.set_sway_border(BorderStyle::Csd, None, true).unwrap();
    let id = t.add_tile(tile, InsertTarget::Focused);

    let geometry = t.compute_geometry();
    assert!(!geometry.titlebars.contains_key(&id));
    assert!(!geometry.titlebar_attached.contains(&id));
    t.update_render_elements(true, crate::layout::RenderLayer::Normal);
    assert_eq!(t.tile(id).unwrap().geometry_corner_radius(), configured);
}

#[test]
fn tab_strips_leave_only_the_box_bottom_corners_on_the_child_border() {
    let configured = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    for layout in [Layout::Tabbed, Layout::Stacked] {
        let mut t = tree((1200., 800.), 0.);
        let window = TestWindow::with_rules(
            1,
            ResolvedWindowRules {
                geometry_corner_radius: Some(configured),
                clip_to_geometry: Some(true),
                ..Default::default()
            },
        );
        let id = t.add_tile(
            Tile::new(
                window,
                t.view_size(),
                1.,
                t.clock().clone(),
                Rc::new(Options::default()),
            ),
            InsertTarget::Focused,
        );
        t.set_layout(t.root, layout);
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);

        assert_eq!(
            t.tile(id).unwrap().geometry_corner_radius(),
            swayward_config::CornerRadius {
                top_left: 0.,
                top_right: 0.,
                bottom_right: configured.bottom_right,
                bottom_left: configured.bottom_left,
            }
        );

        assert!(t.set_fullscreen(&1, true));
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);
        assert_eq!(
            t.tile(id).unwrap().geometry_corner_radius(),
            swayward_config::CornerRadius::default()
        );
    }
}

#[test]
fn tab_strips_leave_only_outer_bottom_corners_on_split_children() {
    // A split below a tab strip is one decorated box. Its child borders can
    // own only the box's outer bottom corners; every interior corner is square.
    let configured = swayward_config::CornerRadius {
        top_left: 11.,
        top_right: 12.,
        bottom_right: 13.,
        bottom_left: 14.,
    };
    let square = swayward_config::CornerRadius::default();
    let rules = || ResolvedWindowRules {
        geometry_corner_radius: Some(configured),
        clip_to_geometry: Some(true),
        ..Default::default()
    };
    let add = |t: &mut TilingTree<TestWindow>, id: usize| {
        let window = TestWindow::with_rules(id, rules());
        let size = t.view_size();
        let clock = t.clock().clone();
        let mut tile = Tile::new(window, size, 1., clock, Rc::new(Options::default()));
        tile.set_sway_border(BorderStyle::Pixel, None, false)
            .unwrap();
        t.add_tile(tile, InsertTarget::Focused)
    };

    for (split, first_expected, second_expected) in [
        (
            Layout::SplitV,
            square,
            swayward_config::CornerRadius {
                bottom_right: configured.bottom_right,
                bottom_left: configured.bottom_left,
                ..square
            },
        ),
        (
            Layout::SplitH,
            swayward_config::CornerRadius {
                bottom_left: configured.bottom_left,
                ..square
            },
            swayward_config::CornerRadius {
                bottom_right: configured.bottom_right,
                ..square
            },
        ),
    ] {
        let mut t = tree((1200., 800.), 0.);
        let first = add(&mut t, 1);
        t.set_layout(t.root, Layout::Tabbed);
        // Split the tab's single child, then add a second leaf to it.
        t.split(first, split);
        let second = add(&mut t, 2);
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);

        let radius_of =
            |t: &TilingTree<TestWindow>, id| t.tile(id).unwrap().geometry_corner_radius();
        assert_eq!(
            radius_of(&t, first),
            first_expected,
            "{split:?}: first child"
        );
        assert_eq!(
            radius_of(&t, second),
            second_expected,
            "{split:?}: second child"
        );
    }
}

#[test]
fn one_window_reserves_a_titlebar_above_its_content() {
    let mut t = tree((1920., 1080.), 0.);
    let id = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let geometry = t.compute_geometry();
    let decorated_box = geometry.leaf_boxes[&id];
    let content = geometry.leaf_contents[&id];
    assert_eq!(decorated_box, Rectangle::from_size(t.view_size()));
    assert!(content.loc.y > decorated_box.loc.y);
    assert!(content.loc.x > decorated_box.loc.x);
    assert!(content.size.w < decorated_box.size.w);
    assert_eq!(content.loc.y + content.size.h, decorated_box.size.h - 4.);
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
fn inserting_a_sibling_scales_existing_shares_for_an_equal_new_share() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    for id in [a, b, c] {
        assert!((t.geometry(id).unwrap().size.w - 400.).abs() < 1e-9);
    }
    t.check_invariants();
}

#[test]
fn configured_default_orientations_set_the_root_at_creation() {
    for (orientation, size, expected) in [
        (
            swayward_config::DefaultOrientation::Horizontal,
            (800., 1200.),
            Layout::SplitH,
        ),
        (
            swayward_config::DefaultOrientation::Vertical,
            (1200., 800.),
            Layout::SplitV,
        ),
        (
            swayward_config::DefaultOrientation::Auto,
            (1200., 800.),
            Layout::SplitH,
        ),
        (
            swayward_config::DefaultOrientation::Auto,
            (800., 1200.),
            Layout::SplitV,
        ),
        (
            swayward_config::DefaultOrientation::Auto,
            (800., 800.),
            Layout::SplitH,
        ),
    ] {
        let mut options = Options::default();
        options.layout.default_orientation = orientation;
        let t = tree_with_options(size, 0., |configured| *configured = options);
        assert!(matches!(
            t.nodes[&t.root].value,
            TreeNode::Split { layout, .. } if layout == expected
        ));
    }
}

#[test]
fn moving_a_single_window_sets_the_workspace_split_axis() {
    let mut t = tree_with_options((800., 1200.), 0., |options| {
        options.layout.default_orientation = swayward_config::DefaultOrientation::Auto;
    });
    let window = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);

    assert!(!t.move_direction(window, Direction::Right));
    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::SplitH,
            ..
        }
    ));
}

#[test]
fn layout_on_an_empty_tree_sets_the_root_layout() {
    let mut t = tree((1200., 800.), 0.);

    t.set_focused_layout(Layout::SplitV);

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
fn removing_a_tile_for_floating_preserves_its_parent_for_reinsertion() {
    let mut t = tree((1200., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let parent = t.nodes[&third].parent.unwrap();

    assert_eq!(t.non_root_parent_for_window(&2), Some(parent));
    let removed = t.remove_tile_preserving_parent(&2).unwrap();
    assert!(t.contains(parent));
    let restored = t.add_tile_to_existing_parent(removed, parent, false);

    assert_eq!(t.nodes[&restored].parent, Some(parent));
    assert_eq!(t.nodes[&third].parent, Some(parent));
    t.check_invariants();

    t.set_focus(restored);
    let parent = t.non_root_parent_for_window(&2).unwrap();
    let removed = t.remove_tile_preserving_parent(&2).unwrap();
    let restored = t.add_tile_to_existing_parent(removed, parent, true);
    assert_eq!(t.focus(), Some(restored));
    assert_eq!(t.nodes[&restored].parent, Some(parent));
    t.check_invariants();
}

#[test]
fn removing_last_tile_while_preserving_parent_reaps_empty_split() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::SplitV);
    let parent = t.nodes[&first].parent.unwrap();

    t.remove_tile_preserving_parent(&1).unwrap();

    assert!(!t.contains(parent));
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
fn detached_subtree_attaches_with_shape_and_internal_focus() {
    let mut source = tree((1200., 800.), 0.);
    let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    source.split(first, Layout::SplitV);
    source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);
    source.set_focus(first);
    let subtree = source.nodes[&first].parent.unwrap();
    let mut destination = tree((1200., 800.), 0.);
    destination.add_tile(tile(3, destination.view_size()), InsertTarget::Focused);

    let (detached, old_parent) = source.detach_subtree(subtree).unwrap();
    destination.attach_subtree(detached);
    source.finish_subtree_detach(old_parent);

    assert_eq!(source.windows().count(), 0);
    let moved = destination
        .iter_depth_first()
        .filter_map(|(id, node)| matches!(node, TreeNode::Split { .. }).then_some(id))
        .find(|id| destination.leaf_ids_in(*id).len() == 2)
        .unwrap();
    assert_eq!(destination.leaf_ids_in(moved).len(), 2);
    assert_eq!(destination.focus(), destination.node_for_window(&3));
    destination.set_focus(moved);
    destination.focus_child();
    assert_eq!(destination.focus(), destination.node_for_window(&1));
    source.check_invariants();
    destination.check_invariants();
}

#[test]
fn attaching_subtree_to_empty_tree_restores_focus() {
    let mut source = tree((1200., 800.), 0.);
    let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    source.split(first, Layout::SplitV);
    source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);
    source.set_focus(first);
    let subtree = source.nodes[&first].parent.unwrap();
    let mut destination = tree((1200., 800.), 0.);

    let (detached, _) = source.detach_subtree(subtree).unwrap();
    destination.attach_subtree(detached);

    assert_eq!(destination.focus(), destination.node_for_window(&1));
    destination.check_invariants();
}

#[test]
fn swapping_nodes_preserves_focus_history_and_rejects_ancestry() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::SplitV);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let parent = t.nodes[&first].parent.unwrap();
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);
    let focus = t.focus();
    let history = t.window_focus_history();
    assert!(t.set_node_fullscreen(first, Some(FullscreenMode::Workspace)));

    t.swap_nodes(first, third).unwrap();
    assert_eq!(t.nodes[&first].parent, Some(t.root));
    assert_eq!(t.nodes[&third].parent, Some(parent));
    assert_eq!(t.focus(), focus);
    assert_eq!(t.window_focus_history(), history);
    assert_eq!(t.fullscreen_mode(first), None);
    assert_eq!(t.fullscreen_mode(third), Some(FullscreenMode::Workspace));
    assert_eq!(t.nodes[&parent].parent, None);
    assert_eq!(
        t.swap_nodes(first, first),
        Err("Cannot swap a container with itself")
    );
    assert_eq!(
        t.swap_nodes(parent, second),
        Err("Cannot swap ancestor and descendant")
    );
    t.check_invariants();
}

#[test]
fn tiled_drop_uses_the_visible_tab() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::Tabbed);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);

    assert_eq!(
        t.tiled_drop_target(Point::from((600., 400.))),
        Some((first, ResizeEdge::empty()))
    );
}

#[test]
fn swapping_subtrees_between_trees_preserves_parent_shares() {
    let mut first_tree = tree((1200., 800.), 0.);
    let first = first_tree.add_tile(tile(1, first_tree.view_size()), InsertTarget::Focused);
    first_tree.add_tile(tile(2, first_tree.view_size()), InsertTarget::Focused);
    let mut second_tree = tree((1200., 800.), 0.);
    let third = second_tree.add_tile(tile(3, second_tree.view_size()), InsertTarget::Focused);
    second_tree.add_tile(tile(4, second_tree.view_size()), InsertTarget::Focused);
    second_tree.add_tile(tile(5, second_tree.view_size()), InsertTarget::Focused);

    let (first_subtree, first_slot) = first_tree.detach_subtree_for_swap(first).unwrap();
    let (second_subtree, second_slot) = second_tree.detach_subtree_for_swap(third).unwrap();
    first_tree.attach_subtree_for_swap(second_subtree, first_slot);
    second_tree.attach_subtree_for_swap(first_subtree, second_slot);

    first_tree.check_invariants();
    second_tree.check_invariants();
}

#[test]
fn swapping_focused_node_into_tabbed_parent_preserves_visible_tab() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let tabbed = t.nodes[&second].parent.unwrap();
    t.set_layout(tabbed, Layout::Tabbed);
    t.set_focus(first);

    t.swap_nodes(first, second).unwrap();

    assert_eq!(t.focus(), Some(second));
    assert_eq!(t.focused_leaf_in(tabbed), Some(first));
    t.check_invariants();
}

#[test]
fn tiled_drop_on_nested_lower_pane_uses_that_pane_and_edge() {
    let mut t = tree((1200., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let upper = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(upper, Layout::SplitV);
    let lower = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let lower_rect = t.geometry(lower).unwrap();

    let (target, edge) = t
        .tiled_drop_target(Point::from((
            lower_rect.loc.x + lower_rect.size.w - 1.,
            lower_rect.loc.y + lower_rect.size.h / 2.,
        )))
        .unwrap();

    assert_eq!(target, lower);
    assert_eq!(edge, ResizeEdge::RIGHT);
    let inserted = t.add_tile_at_drop(tile(4, t.view_size()), target, edge, true);
    let parent = t.nodes[&inserted].parent.unwrap();
    assert_eq!(t.nodes[&lower].parent, Some(parent));
    assert!(matches!(
        t.nodes[&parent].value,
        TreeNode::Split {
            layout: Layout::SplitH,
            ..
        }
    ));
    t.check_invariants();
}

#[test]
fn move_subtree_to_node_inserts_beside_a_leaf_and_into_a_split() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.move_subtree_to_node(first, second));
    assert_eq!(t.root_children().unwrap(), &[second, first, third]);

    t.split(second, Layout::SplitV);
    let split = t.nodes[&second].parent.unwrap();
    assert!(t.move_subtree_to_node(third, split));
    assert_eq!(t.nodes[&third].parent, Some(split));
    assert_eq!(t.root_children().unwrap(), &[split, first]);
    t.check_invariants();
}

#[test]
fn move_subtree_to_ancestor_appends_after_existing_children() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let parent = t.alloc(Node {
        parent: Some(t.root),
        value: TreeNode::Split {
            layout: Layout::SplitH,
            children: vec![b],
            percents: vec![1.],
        },
    });
    t.nodes.get_mut(&b).unwrap().parent = Some(parent);
    t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
        layout: Layout::SplitH,
        children: vec![a, parent, c],
        percents: vec![1. / 3.; 3],
    };
    assert!(t.move_subtree_to_node(b, t.root));

    assert_eq!(t.root_children().unwrap(), &[a, c, b]);
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
fn disabled_directional_focus_does_not_record_a_wrap_candidate() {
    let mut t = tree_with_options((1200., 800.), 0., |options| {
        options.layout.focus_wrapping = swayward_config::FocusWrapping::No;
    });
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);

    assert!(!t.focus_left());
    assert_eq!(t.focus(), Some(first));
    t.check_invariants();
}

#[test]
fn directional_focus_defers_the_innermost_wrap_while_walking_ancestors() {
    let make_tree = |inner_first: bool| {
        let mut tree = tree((1200., 800.), 0.);
        let first = tree.add_tile(tile(1, tree.view_size()), InsertTarget::Focused);
        let focused = tree.add_tile(tile(2, tree.view_size()), InsertTarget::Focused);
        let outer = tree.add_tile(tile(3, tree.view_size()), InsertTarget::Focused);
        let inner = tree.alloc(Node {
            parent: Some(tree.root),
            value: TreeNode::Split {
                layout: Layout::SplitH,
                children: vec![first, focused],
                percents: vec![0.5, 0.5],
            },
        });
        tree.nodes.get_mut(&first).unwrap().parent = Some(inner);
        tree.nodes.get_mut(&focused).unwrap().parent = Some(inner);
        tree.nodes.get_mut(&outer).unwrap().parent = Some(tree.root);
        tree.nodes.get_mut(&tree.root).unwrap().value = TreeNode::Split {
            layout: Layout::SplitH,
            children: if inner_first {
                vec![inner, outer]
            } else {
                vec![outer, inner]
            },
            percents: vec![0.5, 0.5],
        };
        tree.set_focus(focused);
        (tree, first, focused, outer)
    };

    let (mut sibling_tree, _, _, outer) = make_tree(true);
    assert!(sibling_tree.focus_right());
    assert_eq!(sibling_tree.focus(), Some(outer));
    sibling_tree.check_invariants();

    let (mut wrap_tree, inner_wrap, _, _) = make_tree(false);
    assert!(wrap_tree.focus_right());
    assert_eq!(wrap_tree.focus(), Some(inner_wrap));
    wrap_tree.check_invariants();
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
    assert_eq!(t.active_window().map(|window| *window.id()), Some(3));
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
fn focus_next_sibling_stops_at_container_while_bare_next_descends() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let nested = t.nodes[&third].parent.unwrap();

    t.set_focus(first);
    assert!(t.focus_next_prev_sibling(true));
    assert_eq!(t.focus(), Some(nested));

    t.set_focus(first);
    assert!(t.focus_right());
    assert_eq!(t.focus(), Some(third));
    t.check_invariants();
}

#[test]
fn split_parent_preserves_stacked_child_focus_axis() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focused_layout(Layout::Stacked);

    assert!(t.focus_parent());
    t.split_focused(Layout::SplitH);
    assert!(t.focus_child());
    assert!(t.focus_down());
    assert_eq!(t.focus(), Some(first));
    assert!(t.focus_up());
    assert_eq!(t.focus(), Some(second));
    t.check_invariants();
}

#[test]
fn layout_default_restores_the_same_previous_split_as_toggle() {
    for previous in [Layout::SplitH, Layout::SplitV] {
        let setup = || {
            let mut tree = tree((1200., 800.), 0.);
            tree.add_tile(tile(1, tree.view_size()), InsertTarget::Focused);
            tree.add_tile(tile(2, tree.view_size()), InsertTarget::Focused);
            tree.set_layout(tree.root, previous);
            tree.set_focused_layout(Layout::Tabbed);
            tree
        };
        let mut direct = setup();
        let mut toggle = setup();

        direct.restore_focused_split_layout();
        toggle.toggle_focused_layout_split();

        assert_eq!(
            direct.ipc_tree().nodes().len(),
            toggle.ipc_tree().nodes().len()
        );
        assert!(matches!(
            direct.nodes[&direct.root].value,
            TreeNode::Split { layout, .. } if layout == previous
        ));
        assert!(matches!(
            toggle.nodes[&toggle.root].value,
            TreeNode::Split { layout, .. } if layout == previous
        ));
        direct.check_invariants();
    }
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
    t.title_formats.insert(focused, "child format".into());
    t.pending_modes.insert(
        parent,
        PendingMode {
            fullscreen: Some(FullscreenMode::Workspace),
            maximized: false,
        },
    );

    let remapped = t.toggle_focused_layout_split();

    assert_eq!(remapped, vec![(parent, focused)]);
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
    assert_eq!(t.fullscreen_mode(focused), Some(FullscreenMode::Workspace));
    assert_eq!(t.title_formats.get(&focused).unwrap(), "child format");
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
fn opening_a_window_after_a_focused_split_adds_its_sibling() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    let split = t.nodes[&second].parent.unwrap();
    t.set_focus(split);

    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    assert!(matches!(
        &children[..],
        [
            IpcNode::Leaf { id: left, .. },
            IpcNode::Split {
                id,
                children: nested,
                ..
            },
            IpcNode::Leaf { id: right, .. },
        ] if *left == first && *id == split
            && matches!(&nested[..], [IpcNode::Leaf { id, .. }] if *id == second)
            && *right == third
    ));
    t.check_invariants();
}

#[test]
fn workspace_layout_wraps_each_inserted_window() {
    for (workspace_layout, expected) in [
        (swayward_config::WorkspaceLayout::Stacking, Layout::Stacked),
        (swayward_config::WorkspaceLayout::Tabbed, Layout::Tabbed),
    ] {
        let mut options = Options::default();
        options.layout.workspace_layout = workspace_layout;
        let mut tree = TilingTree::new(
            Size::from((1000., 1000.)),
            Rectangle::from_size(Size::from((1000., 1000.))),
            false,
            1.,
            Clock::with_time(Duration::ZERO),
            Rc::new(options),
        );

        tree.add_tile(tile(1, tree.view_size()), InsertTarget::Focused);
        tree.add_tile(tile(2, tree.view_size()), InsertTarget::Focused);

        let TreeNode::Split { children, .. } = &tree.nodes[&tree.root].value else {
            unreachable!()
        };
        assert_eq!(children.len(), 1);
        let TreeNode::Split {
            layout, children, ..
        } = &tree.nodes[&children[0]].value
        else {
            panic!("workspace layout must wrap the inserted leaf")
        };
        assert_eq!(*layout, expected);
        assert_eq!(children.len(), 2);
    }
}

#[test]
fn split_descendant_of_stacked_container_has_no_inner_gap() {
    let mut t = tree((500., 300.), 10.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let split = t.alloc(Node {
        parent: Some(t.root),
        value: TreeNode::Split {
            layout: Layout::SplitH,
            children: vec![first, second],
            percents: vec![0.5, 0.5],
        },
    });
    t.nodes.get_mut(&first).unwrap().parent = Some(split);
    t.nodes.get_mut(&second).unwrap().parent = Some(split);
    t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
        layout: Layout::Stacked,
        children: vec![split],
        percents: vec![1.],
    };

    assert_eq!(t.geometry(first).unwrap().size.w, 240.);
    assert_eq!(t.geometry(second).unwrap().loc.x, 250.);
}

#[test]
fn deeply_nested_split_descendant_of_stacked_container_has_no_inner_gap() {
    let mut t = tree((500., 300.), 10.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let split = t.alloc(Node {
        parent: None,
        value: TreeNode::Split {
            layout: Layout::SplitH,
            children: vec![first, second],
            percents: vec![0.5, 0.5],
        },
    });
    let middle = t.alloc(Node {
        parent: Some(t.root),
        value: TreeNode::Split {
            layout: Layout::SplitV,
            children: vec![split],
            percents: vec![1.],
        },
    });
    t.nodes.get_mut(&first).unwrap().parent = Some(split);
    t.nodes.get_mut(&second).unwrap().parent = Some(split);
    t.nodes.get_mut(&split).unwrap().parent = Some(middle);
    t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
        layout: Layout::Stacked,
        children: vec![middle],
        percents: vec![1.],
    };

    assert_eq!(t.geometry(first).unwrap().size.w, 240.);
    assert_eq!(t.geometry(second).unwrap().loc.x, 250.);
}

#[test]
fn inner_gaps_shrink_and_floor_on_both_split_axes() {
    let mut horizontal = tree_with_options((205., 300.), 10., |options| {
        options.layout.default_orientation = swayward_config::DefaultOrientation::Horizontal;
    });
    let first = horizontal.add_tile(tile(1, horizontal.view_size()), InsertTarget::Focused);
    let second = horizontal.add_tile(tile(2, horizontal.view_size()), InsertTarget::Focused);
    let geometry = horizontal.compute_geometry();
    assert_eq!(geometry.ipc_nodes[&first].size.w, 92.5);
    assert_eq!(geometry.ipc_nodes[&second].loc.x, 102.5);

    let mut vertical = tree((500., 125.), 10.);
    let first = vertical.add_tile(tile(1, vertical.view_size()), InsertTarget::Focused);
    let second = vertical.add_tile(tile(2, vertical.view_size()), InsertTarget::Focused);
    vertical.nodes.get_mut(&vertical.root).unwrap().value = TreeNode::Split {
        layout: Layout::SplitV,
        children: vec![first, second],
        percents: vec![0.5, 0.5],
    };
    let geometry = vertical.compute_geometry();
    assert_eq!(geometry.ipc_nodes[&first].size.h, 52.5);
    assert_eq!(geometry.ipc_nodes[&second].loc.y, 62.5);
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
fn directional_move_squashes_after_reordering_siblings() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let fourth = t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);
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
    t.nodes.get_mut(&fourth).unwrap().parent = Some(t.root);
    t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
        layout: Layout::SplitH,
        children: vec![container, third, fourth],
        percents: vec![0.5, 0.25, 0.25],
    };

    assert!(t.move_direction(fourth, Direction::Left));

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    assert!(matches!(
        &children[..],
        [
            IpcNode::Leaf { id: left, .. },
            IpcNode::Leaf { id: middle, .. },
            IpcNode::Leaf { id: moved, .. },
            IpcNode::Leaf { id: right, .. },
        ] if *left == first && *middle == second && *moved == fourth && *right == third
    ));
    t.check_invariants();
}

#[test]
fn directional_move_swaps_same_parent_siblings_and_preserves_their_shares() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    assert!(t.resize_adjacent(a, b, 0.1));

    assert!(t.move_direction(b, Direction::Left));
    assert_eq!(t.geometry(b).unwrap().loc.x, 0.);
    assert_eq!(t.geometry(b).unwrap().size.w, 480.);
    assert_eq!(t.geometry(a).unwrap().loc.x, 480.);
    assert_eq!(t.geometry(a).unwrap().size.w, 720.);
    assert!(!t.move_direction(b, Direction::Left));
    t.check_invariants();
}

#[test]
fn directional_move_preserves_a_nonsquashable_singleton_source() {
    let mut t = tree((1200., 800.), 0.);
    let top_left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let right = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(top_left);
    t.split(top_left, Layout::SplitV);
    let bottom_left = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focus(bottom_left);

    assert!(t.move_direction(bottom_left, Direction::Right));

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    assert!(matches!(
        &children[..],
        [
            IpcNode::Split {
                layout: Layout::SplitV,
                children: source,
                ..
            },
            IpcNode::Leaf { id, .. },
            IpcNode::Leaf { id: last, .. },
        ] if matches!(&source[..], [IpcNode::Leaf { id, .. }] if *id == top_left)
            && *id == bottom_left && *last == right
    ));
    t.check_invariants();
}

#[test]
fn directional_move_descends_after_the_inactive_child_of_a_perpendicular_branch() {
    let mut t = tree((1200., 800.), 0.);
    let left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let top_right = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(top_right, Layout::SplitV);
    let bottom_right = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.move_direction(left, Direction::Right));

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    assert_eq!(children.len(), 1);
    assert!(matches!(
        &children[0],
        IpcNode::Split {
            layout: Layout::SplitV,
            children,
            ..
        } if matches!(&children[..], [
            IpcNode::Leaf { id: top, .. },
            IpcNode::Leaf { id: inactive, .. },
            IpcNode::Leaf { id, .. },
        ] if *top == top_right && *inactive == bottom_right && *id == left)
    ));
    let parent = t.nodes[&left].parent.unwrap();
    let TreeNode::Split { percents, .. } = &t.nodes[&parent].value else {
        panic!("destination must be a split");
    };
    for percent in percents {
        assert!((percent - 1. / 3.).abs() < 1e-9);
    }
    t.check_invariants();
}

#[test]
fn directional_move_prepends_to_a_parallel_branch() {
    let mut t = tree((1200., 800.), 0.);
    t.set_focused_layout(Layout::SplitV);
    let top = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let first_bottom = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(first_bottom, Layout::SplitV);
    t.set_focused_layout(Layout::Stacked);
    let second_bottom = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(second_bottom);

    assert!(t.move_direction(top, Direction::Down));

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    let IpcNode::Split {
        id: branch,
        layout,
        children,
        ..
    } = &children[0]
    else {
        panic!("destination must be a split");
    };
    assert_eq!(*layout, Layout::Stacked);
    assert!(matches!(
        &children[..],
        [
            IpcNode::Leaf { id, .. },
            IpcNode::Leaf { id: first, .. },
            IpcNode::Leaf { id: second, .. },
        ] if *id == top && *first == first_bottom && *second == second_bottom
    ));
    let TreeNode::Split { percents, .. } = &t.nodes[branch].value else {
        unreachable!();
    };
    for percent in percents {
        assert!((percent - 1. / 3.).abs() < 1e-9);
    }
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
    assert_eq!(t.geometry(a).unwrap().loc.x, 0.);
    assert!(t.geometry(c).unwrap().loc.x > t.geometry(a).unwrap().loc.x);
    assert!(t.geometry(b).unwrap().loc.x > t.geometry(c).unwrap().loc.x);
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
fn consuming_between_two_children_preserves_parent_layout() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);

    assert!(t.consume(second, false));

    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::Tabbed,
            ..
        }
    ));
    let wrapper = t.nodes[&second].parent.unwrap();
    assert_ne!(wrapper, t.root);
    assert_eq!(t.nodes[&first].parent, Some(wrapper));
    t.check_invariants();
}

#[test]
fn reordering_a_subtree_preserves_its_share() {
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.resize_adjacent(a, b, 0.1));
    assert!(t.move_subtree_to_first(c));
    assert_eq!(t.geometry(c).unwrap().size.w, 400.);
    assert_eq!(t.geometry(a).unwrap().loc.x, 400.);
    assert_eq!(t.geometry(b).unwrap().loc.x, 920.);
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
fn fixed_resize_entry_points_use_the_tiled_child_extent() {
    let resize = |directional| {
        let mut t = tree((1000., 800.), 10.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
        let initial = t.geometry(first).unwrap().size.w;
        if directional {
            assert!(t.resize_window_edge(
                Some(&1),
                crate::utils::ResizeEdge::RIGHT,
                SizeChange::AdjustFixed(100),
            ));
        } else {
            t.set_window_width(Some(&1), SizeChange::AdjustFixed(100));
        }
        (initial, t.geometry(first).unwrap().size.w)
    };

    let axis = resize(false);
    let edge = resize(true);
    assert!((axis.1 - axis.0 - 100.).abs() < 1e-9);
    assert!((axis.0 - edge.0).abs() < 1e-9);
    assert!((axis.1 - edge.1).abs() < 1e-9);
}

#[test]
fn set_size_entry_points_use_parent_extent_and_all_siblings() {
    let resize = |sway| {
        let mut t = tree((1000., 800.), 0.);
        t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        let top_right = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.split(top_right, Layout::SplitV);
        t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
        t.set_focus(top_right);
        t.split(top_right, Layout::SplitH);
        let middle = t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);
        let right = t.add_tile(tile(5, t.view_size()), InsertTarget::Focused);

        if sway {
            t.set_window_size_sway(&2, Some(SizeChange::SetProportion(60.)), None);
        } else {
            t.set_window_width(Some(&2), SizeChange::SetProportion(60.));
        }
        [top_right, middle, right].map(|id| t.geometry(id).unwrap().size.w)
    };

    let sway = resize(true);
    for (actual, expected) in resize(false).into_iter().zip(sway) {
        assert!((actual - expected).abs() < 1e-9);
    }
    for (actual, expected) in sway.into_iter().zip([300., 100., 100.]) {
        assert!((actual - expected).abs() < 1e-9);
    }
}

#[test]
fn resizing_adjacent_siblings_changes_only_that_boundary() {
    let mut t = tree((1000., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.resize_adjacent(a, b, 0.1));
    assert!((t.geometry(a).unwrap().size.w - 1000. * (1. / 3. + 0.1)).abs() < 1e-9);
    assert!((t.geometry(b).unwrap().size.w - 1000. * (1. / 3. - 0.1)).abs() < 1e-9);
    assert!((t.geometry(c).unwrap().size.w - 1000. / 3.).abs() < 1e-9);
    assert!(!t.resize_adjacent(a, b, 0.6));
    t.check_invariants();
}

#[test]
fn mapping_under_fullscreen_preserves_focus_and_sibling_percents() {
    let mut t = tree((1920., 1080.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let fullscreen = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    assert!(t.set_node_fullscreen(fullscreen, Some(FullscreenMode::Workspace)));

    let mapped = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert_eq!(t.focus(), Some(fullscreen));
    let TreeNode::Split { percents, .. } = &t.nodes[&t.root].value else {
        panic!("root must be a split");
    };
    assert_eq!(percents.len(), 3);
    assert_eq!(t.nodes[&first].parent, Some(t.root));
    assert_eq!(t.nodes[&mapped].parent, Some(t.root));
    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("IPC root must be a split");
    };
    assert!(matches!(
        &children[..],
        [
            IpcNode::Leaf { percent: Some(first), .. },
            IpcNode::Leaf { percent: Some(second), .. },
            IpcNode::Leaf { percent: Some(mapped), .. },
        ] if *first == 0.5 && *second == 0.5 && *mapped == 0.
    ));
    t.check_invariants();
}

#[test]
fn mapping_fullscreen_window_replaces_existing_fullscreen() {
    let mut t = tree((1920., 1080.), 0.);
    let first_window = TestWindow::new(1);
    first_window.0.requested_mode.set(SizingMode::Fullscreen);
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
    let second_window = TestWindow::new(2);
    second_window.0.requested_mode.set(SizingMode::Fullscreen);
    let second = t.add_tile(
        Tile::new(
            second_window,
            t.view_size(),
            1.,
            Clock::with_time(Duration::ZERO),
            Rc::new(Options::default()),
        ),
        InsertTarget::Focused,
    );

    assert_eq!(t.fullscreen_node(), Some(second));
    assert_eq!(t.fullscreen_mode(first), None);
    assert_eq!(t.fullscreen_mode(second), Some(FullscreenMode::Workspace));
    t.check_invariants();
}

#[test]
fn unfullscreening_another_node_preserves_the_active_fullscreen() {
    let mut t = tree((1920., 1080.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.set_node_fullscreen(second, Some(FullscreenMode::Workspace)));
    assert!(!t.set_node_fullscreen(first, None));
    assert_eq!(t.fullscreen_node(), Some(second));
    t.check_invariants();
}

#[test]
fn fullscreen_leaf_blocks_directional_focus_escape() {
    let mut t = tree((1920., 1080.), 0.);
    let _first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.set_node_fullscreen(second, Some(FullscreenMode::Workspace)));
    assert!(!t.focus_direction(Direction::Left));
    assert_eq!(t.focus(), Some(second));
    t.check_invariants();
}

#[test]
fn fullscreen_container_restricts_focus_and_move_to_its_subtree() {
    let mut t = tree((1920., 1080.), 0.);
    let _left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let upper = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(upper, Layout::SplitV);
    let lower = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let branch = t.nodes[&lower].parent.unwrap();

    assert!(t.set_node_fullscreen(branch, Some(FullscreenMode::Workspace)));
    assert!(t.activate_window(&2));
    assert!(!t.focus_direction(Direction::Left));
    assert_eq!(t.focus(), Some(upper));
    assert!(!t.move_direction(upper, Direction::Left));
    assert_eq!(t.nodes[&upper].parent, Some(branch));
    assert!(t.activate_window(&3));
    assert!(t.focus_direction(Direction::Up));
    assert_eq!(t.focus(), Some(upper));
    assert_eq!(t.fullscreen_node(), Some(branch));
    assert_eq!(t.visible_leaves(), HashSet::from([upper, lower]));
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
    assert!(!t.move_direction(first, Direction::Right));
    assert!(t.is_active_pending_fullscreen());
    assert_eq!(first_state.0.requested_mode.get(), SizingMode::Fullscreen);

    assert!(t.set_fullscreen(&1, false));
    assert!(t.set_maximized(&1, true));
    assert_eq!(first_state.0.requested_mode.get(), SizingMode::Maximized);
    t.move_subtree_to_first(first);
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
fn external_resize_cancels_interactive_resize_without_reverting_it() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(2, crate::utils::ResizeEdge::RIGHT));
    assert!(t.interactive_resize_update(&2, Point::from((50., 0.))));
    t.set_window_width(Some(&3), SizeChange::AdjustFixed(50));
    let widths: Vec<_> = t
        .windows()
        .map(|(id, _)| t.geometry(id).unwrap().size.w)
        .collect();

    assert!(t.interactive_resize.is_none());
    assert!(!t.interactive_resize_update(&2, Point::from((110., 0.))));
    assert_eq!(
        t.windows()
            .map(|(id, _)| t.geometry(id).unwrap().size.w)
            .collect::<Vec<_>>(),
        widths
    );
    t.check_invariants();
}

#[test]
fn removing_from_a_resize_branch_cancels_interactive_resize() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(2, crate::utils::ResizeEdge::LEFT));
    t.remove_tile(&3, Transaction::new()).unwrap();

    assert!(t.interactive_resize.is_none());
    t.check_invariants();
}

#[test]
fn detaching_a_resize_sibling_cancels_interactive_resize() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(1, crate::utils::ResizeEdge::RIGHT));
    t.detach_subtree(second).unwrap();

    assert!(t.interactive_resize.is_none());
    t.check_invariants();
}

#[test]
fn expelling_a_resize_sibling_cancels_interactive_resize() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(2, crate::utils::ResizeEdge::LEFT));
    assert!(t.expel(third, true));

    assert!(t.interactive_resize.is_none());
    t.check_invariants();
}

#[test]
fn consuming_a_resize_sibling_cancels_interactive_resize() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(1, crate::utils::ResizeEdge::RIGHT));
    assert!(t.consume(second, true));

    assert!(t.interactive_resize.is_none());
    t.check_invariants();
}

#[test]
fn width_resize_walks_past_tabbed_parent() {
    let mut t = tree((1000., 800.), 0.);
    let left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let first_tab = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(first_tab, Layout::Tabbed);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    let tabs = t.nodes[&first_tab].parent.unwrap();

    t.set_window_width(Some(&2), SizeChange::AdjustProportion(10.));

    assert_eq!(t.sibling_percents(left, tabs), Some((0.4, 0.6)));
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
fn nested_sway_set_size_uses_outer_allocations_and_ipc_reports_content() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let top_right = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(top_right, Layout::SplitV);
    let bottom_right = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    t.set_window_size_sway(
        &3,
        Some(SizeChange::SetFixed(201)),
        Some(SizeChange::SetFixed(131)),
    );

    let geometry = t.compute_geometry();
    assert_eq!(geometry.ipc_nodes[&bottom_right].size, (201., 131.).into());
    assert_eq!(
        geometry.leaf_contents[&bottom_right].size,
        (193., 105.).into()
    );
    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be split")
    };
    let IpcNode::Split { children, .. } = &children[1] else {
        panic!("right branch must be split")
    };
    let IpcNode::Leaf { rect, .. } = &children[1] else {
        panic!("bottom-right window must be a leaf")
    };
    assert_eq!(rect.size, (201., 109.).into());
}

#[test]
fn sway_set_size_uses_the_matching_axis_branch_extent() {
    let mut t = tree((1000., 800.), 0.);
    let left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let top_right = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(top_right, Layout::SplitV);
    let bottom_right = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    t.set_window_size_sway(&3, None, Some(SizeChange::SetProportion(75.)));
    let (top_percent, bottom_percent) = t.sibling_percents(top_right, bottom_right).unwrap();
    assert!((top_percent - 0.25).abs() < 0.001);
    assert!((bottom_percent - 0.75).abs() < 0.001);

    t.set_window_size_sway(&3, Some(SizeChange::SetFixed(200)), None);
    assert_eq!(t.geometry(bottom_right).unwrap().size.w, 200.);
    assert_eq!(t.geometry(left).unwrap().size.w, 800.);
    t.check_invariants();
}

#[test]
fn sway_set_percentage_uses_nearest_axis_parent_and_all_its_siblings() {
    let mut t = tree((1001., 800.), 0.);
    let outer_left = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let nested_top = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(nested_top, Layout::SplitV);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(nested_top);
    t.split(nested_top, Layout::SplitH);
    let nested_middle = t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);
    let nested_right = t.add_tile(tile(5, t.view_size()), InsertTarget::Focused);

    t.set_window_size_sway(&2, Some(SizeChange::SetProportion(60.)), None);

    let widths = [outer_left, nested_top, nested_middle, nested_right]
        .map(|id| t.geometry(id).unwrap().size.w);
    assert_eq!(widths[0], 500.5);
    assert_eq!(widths[1], 300.);
    assert!((widths[2] - 100.25).abs() < 1e-9);
    assert!((widths[3] - 100.25).abs() < 1e-9);
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
fn default_tab_and_stack_titlebars_match_the_window_outer_width() {
    for (layout, scale) in [
        (Layout::Tabbed, 1.),
        (Layout::Tabbed, 1.25),
        (Layout::Stacked, 1.),
        (Layout::Stacked, 1.25),
    ] {
        let config = swayward_config::Config::load_default();
        let size = Size::from((1001., 800.));
        let options = Options {
            layout: config.layout,
            ..Default::default()
        };
        let mut t = TilingTree::new(
            size,
            Rectangle::from_size(size),
            false,
            scale,
            Clock::with_time(Duration::ZERO),
            Rc::new(options),
        );
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.split(first, layout);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        for tile in t.tiles_mut() {
            tile.window()
                .0
                .size
                .set(tile.window().0.requested_size.get().unwrap());
        }

        let geometry = t.compute_geometry();
        let ids = [first, t.node_for_window(&2).unwrap()];
        let first_content = geometry.leaf_contents[&first];
        let window_width = titlebar::physical_extent(
            scale,
            first_content.loc.x,
            t.tile(first).unwrap().tile_size().w,
        );
        if layout == Layout::Tabbed {
            let strip_width: i32 = ids
                .into_iter()
                .map(|id| geometry.titlebars[&id].rect)
                .map(|bar| titlebar::physical_extent(scale, bar.loc.x, bar.size.w))
                .sum();
            assert_eq!(strip_width, window_width, "tabbed at scale {scale}");
        } else {
            for id in ids {
                let bar = geometry.titlebars[&id].rect;
                assert_eq!(
                    titlebar::physical_extent(scale, bar.loc.x, bar.size.w),
                    window_width,
                    "stacked at scale {scale}: titlebar={bar:?}"
                );
            }
        }
    }
}

#[test]
fn tab_and_stack_borders_follow_the_active_child() {
    for layout in [Layout::Tabbed, Layout::Stacked] {
        let mut t = tree((1000., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.split(first, layout);
        let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

        let geometry = t.compute_geometry();
        assert_eq!(geometry.border_visible, HashSet::from([second]));
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);
        assert!(!t.tile(first).unwrap().border_is_visible());
        assert!(t.tile(second).unwrap().border_is_visible());

        t.set_focus(first);
        let geometry = t.compute_geometry();
        assert_eq!(geometry.border_visible, HashSet::from([first]));
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);
        assert!(t.tile(first).unwrap().border_is_visible());
        assert!(!t.tile(second).unwrap().border_is_visible());
    }
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
fn leaves_nested_below_a_tab_or_stack_report_their_own_titlebars() {
    for layout in [Layout::Tabbed, Layout::Stacked] {
        let mut t = tree((1000., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.set_layout(t.root, layout);
        t.split(first, Layout::SplitH);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.set_focus(t.root);
        t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

        let IpcNode::Split { children, .. } = t.ipc_tree() else {
            panic!("workspace root is not a split");
        };
        let IpcNode::Split { children, .. } = &children[0] else {
            panic!("tab or stack child is not a split");
        };
        assert!(children.iter().all(|child| matches!(
            child,
            IpcNode::Leaf {
                deco_rect: Some(_),
                ..
            }
        )));
    }
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
fn scrolling_a_nested_tab_uses_the_innermost_strip() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::Tabbed);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let inner = t.nodes[&first].parent.unwrap();
    t.set_focus(inner);
    t.split(inner, Layout::Tabbed);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(first);

    assert_eq!(t.scroll_tab_indicator(&1, 1), Some(2));
}

#[test]
fn scrolling_a_non_active_tab_uses_active_child_and_clamps_at_both_ends() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::Tabbed);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert_eq!(t.scroll_tab_indicator(&1, 1), Some(3));
    assert_eq!(t.active_window().map(|window| *window.id()), Some(3));
    assert_eq!(t.scroll_tab_indicator(&1, -1), Some(2));
    assert_eq!(t.active_window().map(|window| *window.id()), Some(2));
    assert_eq!(t.scroll_tab_indicator(&3, -10), Some(1));
    assert_eq!(t.active_window().map(|window| *window.id()), Some(1));
    assert_eq!(t.scroll_tab_indicator(&3, 10), Some(3));
    assert_eq!(t.active_window().map(|window| *window.id()), Some(3));
}

#[test]
fn tab_indicator_focus_target_is_the_focused_descendant() {
    for layout in [Layout::Tabbed, Layout::Stacked] {
        let mut t = tree((1000., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.split(first, layout);
        let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.split(second, Layout::SplitH);
        t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

        assert_eq!(
            t.tab_indicator_focus_target(&1).map(|window| *window.id()),
            Some(3)
        );
        assert_eq!(
            t.tab_indicator_focus_target(&2).map(|window| *window.id()),
            Some(3)
        );
    }
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
fn fullscreen_ignores_default_inner_gaps() {
    let mut t = tree((1000., 800.), 16.);
    let id = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    assert!(t.set_fullscreen(&1, true));

    assert_eq!(t.geometry(id), Some(Rectangle::from_size(t.view_size())));
    assert_eq!(
        t.tiles_with_render_positions().next().unwrap().1,
        Point::default()
    );
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
        Some(Size::from((
            992,
            800 - titlebar::height(1., &swayward_config::Titlebar::default()) as i32 - 4
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

fn check_geometry(tree: &TilingTree<TestWindow>) {
    let geometry = tree.compute_geometry();
    for (id, rect) in &geometry.ipc_nodes {
        assert!(
            rect.loc.x.is_finite()
                && rect.loc.y.is_finite()
                && rect.size.w.is_finite()
                && rect.size.h.is_finite()
                && rect.size.w >= 0.
                && rect.size.h >= 0.,
            "invalid geometry for {id:?}: {rect:?}"
        );
    }
    for (parent, node) in &tree.nodes {
        let TreeNode::Split {
            layout, children, ..
        } = &node.value
        else {
            continue;
        };
        let parent_rect = geometry.ipc_nodes[parent];
        for child in children {
            let child_rect = geometry.ipc_nodes[child];
            if child_rect.size.w > 0. && child_rect.size.h > 0. {
                assert!(
                    child_rect.loc.x >= parent_rect.loc.x - 1e-6
                        && child_rect.loc.y >= parent_rect.loc.y - 1e-6
                        && child_rect.loc.x + child_rect.size.w
                            <= parent_rect.loc.x + parent_rect.size.w + 1e-6
                        && child_rect.loc.y + child_rect.size.h
                            <= parent_rect.loc.y + parent_rect.size.h + 1e-6,
                    "child {child:?} lies outside parent {parent:?}"
                );
            }
        }
        if matches!(layout, Layout::SplitH | Layout::SplitV) {
            for pair in children.windows(2) {
                let first = geometry.ipc_nodes[&pair[0]];
                let second = geometry.ipc_nodes[&pair[1]];
                let separated = match layout {
                    Layout::SplitH => first.loc.x + first.size.w <= second.loc.x + 1e-6,
                    Layout::SplitV => first.loc.y + first.size.h <= second.loc.y + 1e-6,
                    _ => unreachable!(),
                };
                assert!(separated, "split children overlap in {parent:?}");
            }
        }
    }
}

fn tiling_tree_proptest_cases() -> u32 {
    if std::env::var_os("RUN_SLOW_TESTS").is_none() {
        16
    } else {
        ProptestConfig::default().cases
    }
}

#[test]
fn tiling_tree_proptest_runs_cases_on_the_fast_gate() {
    assert!(tiling_tree_proptest_cases() > 0);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: tiling_tree_proptest_cases(),
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
            if tree.fullscreen_node().is_none() {
                check_geometry(&tree);
            }
        }
    }
}

#[test]
fn focused_leaf_renders_in_front_of_its_siblings() {
    // Render elements are collected front to back. With the focused window
    // later in the list, the preceding sibling's shadow darkened the focused
    // border where the two met: three stray pixels in the decoration matrix,
    // visible only as exact-colour endpoints.
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    for focused in [a, b, c] {
        t.set_focus(focused);
        let order: Vec<_> = t
            .leaf_render_order(t.focus)
            .filter(|(_, node)| matches!(node, TreeNode::Leaf { .. }))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(
            order.first(),
            Some(&focused),
            "focused leaf must render first"
        );
        assert_eq!(order.len(), 3, "every leaf is rendered exactly once");
    }
}

#[test]
fn uncovered_top_border_is_collected_before_titlebars() {
    // Render elements are collected front to back: an earlier push is drawn on
    // top. The uncovered top border occupies the same rows as an inactive tab's
    // titlebar ring, so collecting it after titlebars let every inactive ring
    // paint over it. The line then only appeared beside the focused tab, and no
    // other test noticed.
    let layers = TilingTree::<TestWindow>::DECORATION_LAYERS;
    let position = |layer| layers.iter().position(|l| *l == layer).unwrap();
    assert!(
        position(DecorationLayer::UncoveredTopBorders) < position(DecorationLayer::Titlebars),
        "the uncovered top border must be drawn above titlebars"
    );
    assert!(position(DecorationLayer::Titlebars) < position(DecorationLayer::Tiles));
    assert_eq!(
        layers.len(),
        3,
        "every decoration layer is collected exactly once"
    );
}

#[test]
fn strips_inside_hidden_tabs_are_not_drawn() {
    // Sway only arranges the active child of a tabbed or stacked container;
    // the rest go to disable_container, which hides the whole subtree,
    // strips included (sway/desktop/transaction.c:316-321). Drawing the strips
    // of a hidden branch stacked them over the focused window: with the outer
    // tab focused in a three-level stack, the two inner strips covered the top
    // 60px of its window, so its top border appeared to float above it.
    let mut t = tree((1200., 800.), 0.);
    let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);
    let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(b, Layout::Tabbed);
    let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.split(c, Layout::Tabbed);
    let d = t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);
    let inner = t.nodes[&b].parent.unwrap();
    let innermost = t.nodes[&c].parent.unwrap();
    assert_eq!(t.nodes[&d].parent, Some(innermost));

    let drawn = |t: &TilingTree<TestWindow>| {
        let geometry = t.compute_geometry();
        let mut strips: Vec<_> = geometry
            .titlebars
            .iter()
            .filter(|(_, bar)| bar.visible)
            .map(|(id, bar)| (*id, bar.rect.loc.y))
            .collect();
        strips.sort_by(|x, y| x.1.total_cmp(&y.1).then(x.0.cmp(&y.0)));
        strips
    };

    // Focus at the bottom of the stack: every level is on the focused path.
    t.set_focus(d);
    let strip_rows: HashSet<_> = drawn(&t).iter().map(|(_, y)| y.to_bits()).collect();
    assert_eq!(
        strip_rows.len(),
        3,
        "three levels of strips when the deepest tab is focused"
    );

    // Focus the outer tab: the inner and innermost containers are hidden, so
    // only the outer strip is drawn, and A's window sits directly beneath it.
    t.set_focus(a);
    let geometry = t.compute_geometry();
    let visible: Vec<_> = drawn(&t).into_iter().map(|(id, _)| id).collect();
    assert_eq!(visible.len(), 2, "only the outer strip: {visible:?}");
    assert!(visible.iter().all(|id| t.nodes[id].parent == Some(t.root)));
    let strip_bottom = geometry.titlebars[&a].rect.loc.y + geometry.titlebars[&a].rect.size.h;
    assert_eq!(geometry.leaf_boxes[&a].loc.y, strip_bottom);
    for hidden in [b, c, d] {
        assert!(
            geometry
                .titlebars
                .get(&hidden)
                .is_none_or(|bar| !bar.visible),
            "strip entry for a leaf in a hidden branch is drawn"
        );
    }
    let hidden = geometry.titlebars[&d].rect;
    assert_eq!(
        t.window_under(hidden.loc + hidden.size.downscale(2.).to_point())
            .map(|(window, _)| *window.id()),
        None
    );
    let _ = inner;
}

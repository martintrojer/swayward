use super::*;

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
fn tab_container_shows_its_last_focused_child_when_focus_leaves_it() {
    // H[T[A, B], C]: focusing B, then C, leaves the tabbed container showing
    // B, the child its focus last visited. Sway arranges the
    // focused-inactive child of a tabbed or stacked container
    // (sway/desktop/transaction.c:468-470) and disables the others (:316-321).
    for layout in [Layout::Tabbed, Layout::Stacked] {
        let mut t = tree((1200., 800.), 0.);
        let a = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        let c = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
        t.set_focus(a);
        t.split(a, layout);
        let b = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.set_focus(b);
        t.set_focus(c);

        assert_eq!(t.visible_leaves(), HashSet::from([b, c]), "{layout:?}");
        let geometry = t.compute_geometry();
        assert!(geometry.border_visible.contains(&b), "{layout:?}");
        assert!(!geometry.border_visible.contains(&a), "{layout:?}");
        assert_eq!(
            geometry.leaf_boxes[&b], geometry.leaf_boxes[&a],
            "{layout:?}"
        );
        let visible: Vec<_> = t
            .tiles_with_render_positions()
            .map(|(tile, _, visible)| (*tile.window().id(), visible))
            .collect();
        assert!(visible.contains(&(2, true)), "{layout:?}: {visible:?}");
        assert!(visible.contains(&(1, false)), "{layout:?}: {visible:?}");
        t.check_invariants();
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
    let first_bar = ipc_deco_rect(&t, 1).unwrap();
    let second_bar = ipc_deco_rect(&t, 2).unwrap();
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

    let first_bar = ipc_deco_rect(&t, 1).unwrap();
    let second_bar = ipc_deco_rect(&t, 2).unwrap();
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
    t.add_tile(tile_from(first, t.view_size()), InsertTarget::Focused);
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
    for (focused, window) in [(a, 1), (b, 2), (c, 3)] {
        t.set_focus(focused);
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);
        let owners: Vec<_> = rendered_order(&t)
            .into_iter()
            .filter_map(|(_, owner)| owner)
            .collect();
        let last_focused = owners.iter().rposition(|owner| *owner == window).unwrap();
        let first_other = owners.iter().position(|owner| *owner != window).unwrap();
        assert!(
            last_focused < first_other,
            "window {window}'s elements must all precede its siblings': {owners:?}"
        );
    }
}

#[test]
fn uncovered_top_border_is_collected_before_titlebars() {
    // Render elements are collected front to back: an earlier push is drawn on
    // top. The uncovered top border occupies the same rows as an inactive tab's
    // titlebar ring, so collecting it after titlebars let every inactive ring
    // paint over it. The line then only appeared beside the focused tab, and no
    // other test noticed.
    let mut t = tree((1200., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);
    t.update_render_elements(true, crate::layout::RenderLayer::Normal);

    let kinds: Vec<_> = rendered_order(&t)
        .into_iter()
        .map(|(kind, _)| kind)
        .filter(|kind| *kind != "tab_indicator")
        .collect();
    let position = |kind| kinds.iter().position(|k| *k == kind).unwrap();
    let last = |kind| kinds.iter().rposition(|k| *k == kind).unwrap();
    assert!(
        last("uncovered_top_border") < position("titlebar"),
        "the uncovered top border must be drawn above titlebars: {kinds:?}"
    );
    assert!(
        last("titlebar") < position("tile"),
        "titlebars must be drawn above tiles: {kinds:?}"
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

#[test]
fn overlapping_tab_indicators_hit_the_innermost_container() {
    // An inner tabbed container shown inside an outer one has the same area, so their
    // indicators overlap. The deeper one must win on every hash seed.
    for _ in 0..32 {
        let mut t = tree((1000., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.split(first, Layout::Tabbed);
        let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        t.split(second, Layout::Tabbed);
        t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
        t.update_render_elements(true, crate::layout::RenderLayer::Normal);
        let mut clock = t.clock().clone();
        clock.set_complete_instantly(true);
        t.advance_animations();

        let area = t.geometry(second).unwrap();
        let config = Options::default().layout.tab_indicator;
        let pos = Point::from((
            area.loc.x - config.gap - config.width / 2.,
            area.loc.y + area.size.h * 3. / 8.,
        ));
        let (window, _) = t.window_under(pos).expect("indicator hit");
        assert_eq!(window.id(), &2);
    }
}

/// Renders `t` with a surfaceless GL renderer and returns, front to back, the kind of each
/// pushed element and, for tile elements, the window that pushed it.
fn rendered_order(t: &TilingTree<TestWindow>) -> Vec<(&'static str, Option<usize>)> {
    use smithay::backend::egl::native::EGLSurfacelessDisplay;
    use smithay::backend::egl::{EGLContext, EGLDisplay};
    use smithay::backend::renderer::element::Element;

    let mut renderer = unsafe {
        let display = EGLDisplay::new(EGLSurfacelessDisplay).expect("EGL display");
        let context = EGLContext::new(&display).expect("EGL context");
        GlesRenderer::new(context).expect("renderer")
    };
    crate::render_helpers::resources::init(&mut renderer);
    crate::render_helpers::shaders::init(&mut renderer);

    let geometries = t.compute_geometry();
    // The test window's border and focus ring hug its surface at the leaf's top-left corner,
    // overhanging it by at most the 4 px border; nudge an element's corner inward past that and
    // take the leaf whose box holds it.
    let owner = |rect: Rectangle<f64, Logical>| {
        let corner = rect.loc + Point::from((8., 8.));
        geometries
            .leaf_boxes
            .iter()
            .filter(|(id, _)| t.visible_leaves().contains(id))
            .find(|(_, leaf)| leaf.contains(corner))
            .map(|(id, _)| *t.tile(*id).unwrap().window().id())
    };
    let mut order = Vec::new();
    t.render(
        crate::render_helpers::RenderCtx {
            renderer: &mut renderer,
            target: crate::render_helpers::RenderTarget::Output,
            xray: None,
        },
        crate::render_helpers::xray::XrayPos::new(Point::default(), 1.),
        true,
        crate::layout::RenderLayer::Normal,
        &mut |element| {
            let kind = match &element {
                TilingTreeRenderElement::Tile(_) => "tile",
                TilingTreeRenderElement::ClosingWindow(_) => "closing",
                TilingTreeRenderElement::TabIndicator(_) => "tab_indicator",
                TilingTreeRenderElement::Titlebar(_) => "titlebar",
                TilingTreeRenderElement::UncoveredTopBorder(_) => "uncovered_top_border",
            };
            let rect = element.geometry(Scale::from(1.)).to_f64().to_logical(1.);
            let leaf = matches!(element, TilingTreeRenderElement::Tile(_))
                .then(|| owner(rect))
                .flatten();
            order.push((kind, leaf));
        },
    );
    order
}

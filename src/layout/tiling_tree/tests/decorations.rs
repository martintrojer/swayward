use super::*;

/// A corner radius with four distinct corners, so a test can tell which corner went where.
const TEST_RADIUS: swayward_config::CornerRadius = swayward_config::CornerRadius {
    top_left: 11.,
    top_right: 12.,
    bottom_right: 13.,
    bottom_left: 14.,
};

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
    let mut tile = tile_from(window.clone(), Size::from((100., 200.)));

    tile.set_sway_border(BorderStyle::None, None, true).unwrap();
    tile.set_sway_border(BorderStyle::Toggle, None, true)
        .unwrap();
    tile.set_sway_border(BorderStyle::Toggle, None, true)
        .unwrap();
    tile.set_sway_border(BorderStyle::Toggle, None, true)
        .unwrap();

    // The toggles keep the tile's default thickness (sway/commands/border.c:90-92).
    let default_width = tile.sway_border_thickness().1;
    assert_eq!(tile.sway_border(), (BorderStyle::Csd, default_width));
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
            "V[window 2 T[window 3]]"
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
    let configured = TEST_RADIUS;
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
    let configured = TEST_RADIUS;
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
    let configured = TEST_RADIUS;
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
    let configured = TEST_RADIUS;
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
    let configured = TEST_RADIUS;
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
    let configured = TEST_RADIUS;
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

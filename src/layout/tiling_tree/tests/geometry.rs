use super::*;

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

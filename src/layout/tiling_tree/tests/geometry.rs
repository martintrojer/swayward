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
fn refresh_sends_the_same_bounds_as_a_new_window_gets() {
    // A window's xdg bounds must not change between its first configure and its first
    // refresh, so both use the strut-reduced working area.
    let mut options = Options::default();
    options.layout.gaps = 0.;
    options.layout.border.off = true;
    options.layout.struts.left = swayward_config::FloatOrInt(40.);
    options.layout.struts.top = swayward_config::FloatOrInt(20.);
    let size = Size::from((1200., 800.));
    let mut t = TilingTree::<TestWindow>::new(
        size,
        Rectangle::from_size(size),
        false,
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(options),
    );
    let window = TestWindow::new(1);
    let inner = window.0.clone();
    t.add_tile(
        Tile::new(window, size, 1., t.clock().clone(), t.options.clone()),
        InsertTarget::Focused,
    );

    t.refresh(true, true);

    assert_eq!(
        inner.bounds.get(),
        Some(t.new_window_toplevel_bounds(&ResolvedWindowRules::default()))
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
    assert_eq!(t.window_focus_history(), [1, 2, 3, 4]);

    let fourth = t.node_for_window(&4).unwrap();
    let second = t.node_for_window(&2).unwrap();
    assert!(t.move_subtree_to_node(fourth, second));
    assert_eq!(t.window_focus_history(), [1, 4, 2, 3]);
}

#[test]
fn restoring_a_removed_windows_focus_rank_preserves_close_order() {
    let mut t = tree((1200., 800.), 0.);
    for id in 1..=5 {
        t.add_tile(tile(id, t.view_size()), InsertTarget::Focused);
    }
    assert_eq!(t.window_focus_history(), [5, 4, 3, 2, 1]);

    let rank = t.focus_rank_for_window(&4).unwrap();
    let removed = t.remove_tile(&4, Transaction::new()).unwrap();
    t.add_tile_with_activation(removed, InsertTarget::Focused, false);
    t.restore_focus_rank(&4, rank);

    assert_eq!(t.window_focus_history(), [5, 4, 3, 2, 1]);
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
fn a_sub_pixel_last_child_reports_a_non_negative_percent() {
    // Tiling proptest case (integrator batch 7): a tabbed root holding a vertical split whose
    // last child a resize left 0.23 px tall. Rounding the earlier children's shares up left
    // the last child -1 px and GET_TREE reported percent -0.00096. Sway's last child takes the
    // parent's remainder (sway/tree/arrange.c:171-174) and never reports a negative percent.
    let mut t = tree((1920., 1080.), 8.);
    let leaves = (0..4)
        .map(|window| t.add_tile(tile(window, t.view_size()), InsertTarget::Focused))
        .collect::<Vec<_>>();
    let root = t.root;
    let split = t.alloc(Node {
        parent: Some(root),
        value: TreeNode::Split {
            layout: Layout::SplitV,
            children: leaves.clone(),
            percents: vec![0.25, 0.4997758843775979, 0.25, 0.00022411562240215455],
            meta: SplitMeta::default(),
        },
    });
    for leaf in &leaves {
        t.nodes.get_mut(leaf).unwrap().parent = Some(split);
    }
    t.nodes.get_mut(&root).unwrap().value = TreeNode::Split {
        layout: Layout::Tabbed,
        children: vec![split],
        percents: vec![1.],
        meta: SplitMeta::default(),
    };
    t.request_window_sizes();

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    let [IpcNode::Split { children, .. }] = children.as_slice() else {
        panic!("the tabbed root holds one split");
    };
    let percents = children
        .iter()
        .map(|child| match child {
            IpcNode::Split { percent, .. } | IpcNode::Leaf { percent, .. } => *percent,
        })
        .collect::<Vec<_>>();
    assert!(
        percents
            .iter()
            .all(|percent| percent.is_some_and(|p| p >= 0.)),
        "negative percent in {percents:?}"
    );
}

#[test]
fn a_fractional_split_width_rounds_shares_like_sway() {
    // A floating group's box is fractional (here 634.6 px). Sway's widths are integers: it
    // splits the integer child_total_width (sway/tree/arrange.c:70-88), giving round(317.5) =
    // 318 then the 317 px remainder. Splitting 634.6 directly gave 317 then 318, and focus
    // events reported the two percents swapped (oracle rows events/grouped_raise_events and
    // events/grouped_scratchpad_events).
    let mut t = tree_with_options((634.6, 400.), 0., |options| {
        options.layout.border.off = true;
    });
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("root must be a split");
    };
    let percents = children
        .iter()
        .map(|child| match child {
            IpcNode::Split { percent, .. } | IpcNode::Leaf { percent, .. } => percent.unwrap(),
        })
        .collect::<Vec<_>>();
    assert!(
        percents[0] > percents[1],
        "the first child takes the rounded-up half: {percents:?}"
    );
}

use super::*;

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
fn corner_interactive_resize_moves_both_boundaries() {
    // [1 | [2 / 3]]: window 2's bottom-left corner borders 1 horizontally and
    // 3 vertically, so a diagonal drag moves both, as sway does.
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);
    let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(2, crate::utils::ResizeEdge::BOTTOM_LEFT));
    assert!(t.interactive_resize_update(&2, Point::from((-100., 80.))));

    assert_eq!(t.geometry(first).unwrap().size.w, 400.);
    assert_eq!(t.geometry(second).unwrap().size.w, 600.);
    assert_eq!(t.geometry(second).unwrap().size.h, 480.);
    assert_eq!(t.geometry(third).unwrap().size.h, 320.);
    t.check_invariants();
}

#[test]
fn corner_interactive_resize_skips_an_axis_without_a_neighbour() {
    // Side by side, a top-right corner has no vertical neighbour: the drag
    // still resizes horizontally and ignores the vertical motion.
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.interactive_resize_begin(1, crate::utils::ResizeEdge::TOP_RIGHT));
    assert!(t.interactive_resize_update(&1, Point::from((100., -50.))));

    assert_eq!(t.geometry(first).unwrap().size.w, 600.);
    assert_eq!(t.geometry(first).unwrap().size.h, 800.);
    t.check_invariants();
}

#[test]
fn a_lone_window_has_no_interactive_resize() {
    let mut t = tree((1000., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);

    assert!(!t.interactive_resize_begin(1, crate::utils::ResizeEdge::BOTTOM_RIGHT));
    assert!(t.interactive_resize.is_none());
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

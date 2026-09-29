use super::*;

#[test]
fn mapping_under_fullscreen_preserves_focus_and_sibling_percents() {
    let mut t = tree((1920., 1080.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let fullscreen = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    assert!(t.set_node_fullscreen(fullscreen, Some(FullscreenMode::Workspace)));

    let mapped = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert_eq!(t.focus(), Some(fullscreen));
    let first_geometry = t.geometry(first).unwrap();
    assert_eq!(first_geometry.size.w, t.view_size().w / 2.);
    assert_eq!(t.ipc_decoration_rect(&3), None);
    let TreeNode::Split { percents, .. } = &t.nodes[&t.root].value else {
        panic!("root must be a split");
    };
    assert_eq!(percents.len(), 3);
    assert_eq!(t.nodes[&first].parent, Some(t.root));
    assert_eq!(t.nodes[&mapped].parent, Some(t.root));
    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("IPC root must be a split");
    };
    let percents = children
        .iter()
        .map(|child| match child {
            IpcNode::Leaf { percent, .. } => *percent,
            IpcNode::Split { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(percents, [Some(0.5), Some(1.), Some(0.)]);
    t.check_invariants();
}

#[test]
fn fullscreen_leaf_reports_pending_area_percent_without_changing_siblings() {
    let mut t = tree((1280., 720.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let fullscreen = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.set_node_fullscreen(fullscreen, Some(FullscreenMode::Workspace)));

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("IPC root must be a split");
    };
    let percents = children
        .iter()
        .map(|child| match child {
            IpcNode::Leaf { percent, .. } => *percent,
            IpcNode::Split { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(percents, [Some(427. / 1280.), Some(1.), Some(426. / 1280.)]);
}

#[test]
fn nested_fullscreen_leaf_reports_area_relative_to_pending_parent() {
    let mut t = tree((1920., 1080.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let upper = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(upper, Layout::SplitV);
    let fullscreen = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(t.set_node_fullscreen(fullscreen, Some(FullscreenMode::Workspace)));

    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("IPC root must be a split");
    };
    let IpcNode::Split { children, .. } = &children[1] else {
        panic!("second child must be a split");
    };
    let IpcNode::Leaf { percent, .. } = &children[1] else {
        panic!("second nested child must be a leaf");
    };
    assert_eq!(*percent, Some(2.));
}

#[test]
fn mapping_under_fullscreen_tab_keeps_normal_ipc_state() {
    let mut t = tree((1920., 1080.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Tabbed);
    assert!(t.set_node_fullscreen(first, Some(FullscreenMode::Workspace)));

    let mapped = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);

    assert!(!t.mapped_under_fullscreen.contains(&mapped));
    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("IPC root must be a split");
    };
    assert!(matches!(
        &children[2],
        IpcNode::Leaf {
            percent: Some(1.),
            mapped_under_fullscreen: false,
            ..
        }
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
fn moving_a_fullscreen_leaf_reveals_later_windows_in_the_source_tree() {
    let mut source = tree((1920., 1080.), 0.);
    let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    let fullscreen = source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);
    assert!(source.set_node_fullscreen(fullscreen, Some(FullscreenMode::Workspace)));
    let mapped = source.add_tile(tile(3, source.view_size()), InsertTarget::Focused);
    source.remove_tile_node(fullscreen).unwrap();

    let IpcNode::Split { children, .. } = source.ipc_tree() else {
        panic!("root must be a split");
    };
    assert!(matches!(
        &children[..],
        [
            IpcNode::Leaf {
                id,
                mapped_under_fullscreen: false,
                ..
            },
            IpcNode::Leaf {
                id: revealed,
                mapped_under_fullscreen: false,
                ..
            },
        ] if *id == first && *revealed == mapped
    ));
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
fn stacked_siblings_keep_their_geometry_while_one_is_fullscreen() {
    let mut t = tree((1000., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::Stacked);

    assert!(t.set_node_fullscreen(second, Some(FullscreenMode::Workspace)));

    assert_eq!(t.geometry(first).unwrap().loc.y, t.titlebar_height * 2.);
    assert_eq!(
        t.geometry(second),
        Some(Rectangle::from_size(t.view_size()))
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

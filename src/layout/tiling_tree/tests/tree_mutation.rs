use super::*;

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
fn empty_auto_tree_tracks_output_orientation_changes() {
    let mut t = tree_with_options((1280., 720.), 0., |options| {
        options.layout.default_orientation = swayward_config::DefaultOrientation::Auto;
    });

    t.update_config(
        (720., 1280.).into(),
        Rectangle::from_size((720., 1280.).into()),
        false,
        1.,
        t.options.clone(),
    );

    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::SplitV,
            ..
        }
    ));
}

#[test]
fn explicit_empty_layout_survives_output_orientation_changes() {
    let mut t = tree_with_options((1280., 720.), 0., |options| {
        options.layout.default_orientation = swayward_config::DefaultOrientation::Auto;
    });
    t.set_focused_layout(Layout::Stacked);

    t.update_config(
        (720., 1280.).into(),
        Rectangle::from_size((720., 1280.).into()),
        false,
        1.,
        t.options.clone(),
    );

    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::Stacked,
            ..
        }
    ));
}

#[test]
fn emptied_initial_tree_keeps_its_pre_mode_orientation() {
    let mut t = tree_with_options((1280., 720.), 0., |options| {
        options.layout.default_orientation = swayward_config::DefaultOrientation::Auto;
    });
    t.preserve_empty_auto_layout();
    t.update_config(
        (720., 1280.).into(),
        Rectangle::from_size((720., 1280.).into()),
        false,
        1.,
        t.options.clone(),
    );
    let window = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);

    t.remove_tile_node(window).unwrap();
    t.reset_empty_layout();

    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::SplitH,
            ..
        }
    ));
    assert_eq!(t.representation_layout(), Layout::SplitH);
}

#[test]
fn removing_the_last_window_resets_the_reported_layout() {
    let mut t = tree((1200., 800.), 0.);
    let window = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    assert!(!t.move_direction(window, Direction::Down));

    t.remove_tile_node(window).unwrap();
    t.reset_empty_layout();

    assert_eq!(t.representation_layout(), Layout::SplitH);
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
fn splitting_a_fullscreen_leaf_transfers_fullscreen_to_its_wrapper() {
    let mut t = tree((1200., 800.), 0.);
    let window = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_focus(window);
    assert!(t.set_node_fullscreen(window, Some(FullscreenMode::Workspace)));

    t.split_focused(Layout::SplitV);

    let wrapper = t.nodes[&window].parent.unwrap();
    assert_eq!(t.fullscreen_node(), Some(wrapper));
    assert_eq!(t.fullscreen_mode(window), None);
    let IpcNode::Split { children, .. } = t.ipc_tree() else {
        panic!("workspace root is not a split");
    };
    let IpcNode::Split { children, .. } = &children[0] else {
        panic!("fullscreen child is not wrapped");
    };
    assert!(matches!(
        &children[..],
        [IpcNode::Leaf {
            deco_rect: Some(_),
            rect,
            ..
        }] if rect.loc.y > 0.
    ));
    t.check_invariants();
}

#[test]
fn split_on_an_emptied_tree_updates_layout_but_retains_its_representation() {
    let mut t = tree((1200., 800.), 0.);
    let window = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.remove_tile_node(window).unwrap();

    t.split_focused(Layout::SplitV);

    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::SplitV,
            ..
        }
    ));
    assert_eq!(t.representation_layout(), Layout::SplitH);

    t.set_focused_layout(Layout::Stacked);
    assert_eq!(t.representation_layout(), Layout::Stacked);
    t.check_invariants();
}

#[test]
fn split_on_an_empty_tree_preserves_the_previous_layout_representation() {
    let mut t = tree((1200., 800.), 0.);
    t.set_focused_layout(Layout::Stacked);

    t.split_focused(Layout::SplitV);

    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::SplitV,
            ..
        }
    ));
    assert_eq!(t.representation_layout(), Layout::Stacked);
    t.check_invariants();
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
fn layout_on_an_empty_tree_initializes_representation_only_when_it_changes() {
    let mut t = tree((1200., 800.), 0.);
    assert!(!t.has_had_tile());

    t.set_focused_layout(Layout::SplitH);
    assert!(!t.has_had_tile());

    t.set_focused_layout(Layout::Stacked);
    assert!(t.has_had_tile());
}

#[test]
fn toggle_split_on_an_empty_tree_changes_the_root_layout() {
    let mut t = tree((1200., 800.), 0.);
    assert!(matches!(
        t.nodes[&t.root].value,
        TreeNode::Split {
            layout: Layout::SplitH,
            ..
        }
    ));

    t.toggle_focused_split();

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
fn layout_split_wraps_a_single_workspace_leaf_when_changing_axis() {
    let mut t = tree((1200., 800.), 0.);
    let leaf = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);

    t.set_focused_layout(Layout::SplitV);

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
        TreeNode::Split {
            layout: Layout::SplitV,
            children,
            ..
        } if children == &[leaf]
    ));
    assert_eq!(t.nodes[&leaf].parent, Some(*wrapper));
    assert_eq!(t.focus(), Some(leaf));
    t.check_invariants();
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
fn repeated_nested_splits_stop_at_the_tree_depth_limit() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    // Far past the depth that overflowed the stack before the bound existed.
    for index in 0..8192 {
        t.wrap_node(
            first,
            if index % 2 == 0 {
                Layout::SplitV
            } else {
                Layout::SplitH
            },
        );
    }

    assert_eq!(t.tree_depth(), MAX_TREE_DEPTH);
    t.compute_geometry();
    t.ipc_tree();
    t.check_invariants();
}

#[test]
fn consume_at_the_depth_limit_is_refused() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    for index in 0..MAX_TREE_DEPTH {
        t.wrap_node(
            first,
            if index % 2 == 0 {
                Layout::SplitV
            } else {
                Layout::SplitH
            },
        );
    }
    let depth = t.tree_depth();
    assert_eq!(depth, MAX_TREE_DEPTH);
    let leaf = t.add_tile_to_existing_parent(
        tile(3, t.view_size()),
        t.nodes[&first].parent.unwrap(),
        true,
    );
    assert!(!t.consume(leaf, false));
    assert_eq!(t.tree_depth(), depth);
    t.check_invariants();
}

use super::*;

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
fn bare_layout_toggle_restores_the_previous_split() {
    let mut t = tree((1200., 800.), 0.);
    t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.set_layout(t.root, Layout::SplitV);
    t.set_focused_layout(Layout::Tabbed);

    t.toggle_focused_layout(&swayward_ipc::command::LayoutToggle::Default);

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
fn focus_top_skips_hidden_tabs() {
    // Every tab shares the shown tab's box, so a geometric pick that includes the hidden tabs
    // lands on one chosen by HashMap order. Build several trees (each with its own hash seed).
    for _ in 0..32 {
        let mut t = tree((1200., 800.), 0.);
        let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
        t.split(first, Layout::Tabbed);
        t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
        let third = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
        t.set_focus(third);

        t.focus_top();
        assert_eq!(t.focus, Some(third));
        t.focus_bottom();
        assert_eq!(t.focus, Some(third));
    }
}

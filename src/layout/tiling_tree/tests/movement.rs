use super::*;

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
fn directional_move_escapes_a_singleton_parallel_parent() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(second, Layout::SplitV);

    assert!(t.move_direction(second, Direction::Down));

    let IpcNode::Split {
        layout, children, ..
    } = t.ipc_tree()
    else {
        panic!("root must be a split");
    };
    assert_eq!(layout, Layout::SplitV);
    assert!(matches!(
        &children[..],
        [IpcNode::Leaf { id: top, .. }, IpcNode::Leaf { id: bottom, .. }]
            if *top == first && *bottom == second
    ));
    t.check_invariants();
}

#[test]
fn directional_move_keeps_an_explicit_split_left_with_one_child() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let moved = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(moved, Layout::SplitV);
    let remaining = t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(moved);

    assert!(t.move_direction(moved, Direction::Down));
    assert!(t.move_direction(moved, Direction::Down));

    let IpcNode::Split {
        layout, children, ..
    } = t.ipc_tree()
    else {
        panic!("root must be a split");
    };
    assert_eq!(layout, Layout::SplitV);
    assert!(matches!(
        &children[..],
        [
            IpcNode::Split {
                layout: Layout::SplitH,
                children: horizontal,
                ..
            },
            IpcNode::Leaf { id, .. },
        ] if matches!(&horizontal[..], [
            IpcNode::Leaf { id: left, .. },
            IpcNode::Split {
                layout: Layout::SplitV,
                children: vertical,
                ..
            },
        ] if *left == first
            && matches!(&vertical[..], [IpcNode::Leaf { id, .. }] if *id == remaining))
            && *id == moved
    ));
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
fn swapping_the_root_is_refused_instead_of_panicking() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    let root = t.root;

    assert_eq!(
        t.swap_nodes(root, first),
        Err("Cannot swap ancestor and descendant")
    );
    let other = tree((1200., 800.), 0.).root;
    t.nodes.insert(
        other,
        Node {
            parent: None,
            value: TreeNode::Split {
                layout: Layout::SplitH,
                children: Vec::new(),
                percents: Vec::new(),
            },
        },
    );
    // A second parentless node is not a container, as sway requires
    // (sway/commands/swap.c:73-75).
    assert_eq!(
        t.swap_nodes(other, first),
        Err("Can only swap with containers and views")
    );
    t.nodes.remove(&other);
    t.check_invariants();
}

// random seed 3 step 9 (sway-1.12-random): a same-axis move of the only
// window promotes it out of nested wrappers to workspace level.
#[test]
fn directional_move_reaps_nested_wrappers_around_the_only_window() {
    let mut t = tree((1200., 800.), 0.);
    let window = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let inner = t.wrap_node(window, Layout::SplitH);
    t.wrap_node(inner, Layout::SplitH);
    t.set_layout(t.root, Layout::SplitV);

    assert!(!t.move_direction(window, Direction::Down));

    let tree = t.ipc_tree();
    assert!(
        matches!(
            tree,
            IpcNode::Split {
                layout: Layout::SplitV,
                ref children,
                ..
            } if matches!(&children[..], [IpcNode::Leaf { id, .. }] if *id == window)
        ),
        "{tree:?}"
    );
    t.check_invariants();
}

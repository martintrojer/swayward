use super::*;

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
fn detached_subtree_keeps_node_ids_when_the_destination_has_no_collision() {
    let mut source = tree((1200., 800.), 0.);
    let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    source.split(first, Layout::SplitV);
    let second = source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);
    let subtree = source.nodes[&first].parent.unwrap();
    assert_eq!(subtree, source.root);
    let old_ids = [first, second];
    let mut destination = tree((1200., 800.), 0.);
    destination.add_tile(tile(3, destination.view_size()), InsertTarget::Focused);

    let (detached, _) = source.detach_subtree(subtree).unwrap();
    let (_, remapped) = destination.attach_subtree(detached);

    assert!(remapped.is_empty());
    assert!(old_ids.into_iter().all(|id| destination.contains(id)));
    // The emptied root keeps its ID in the source tree, so the detached split
    // must not carry it into the destination.
    assert!(source.contains(subtree));
    assert!(!destination.contains(subtree));
    source.check_invariants();
    destination.check_invariants();
}

#[test]
fn resident_subtree_uses_its_outer_rectangle_without_workspace_outer_gaps() {
    let mut source = tree((1200., 800.), 0.);
    let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    source.split(first, Layout::SplitV);
    source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);
    let subtree = source.nodes[&first].parent.unwrap();
    assert_eq!(subtree, source.root);
    let original_ids: HashSet<_> = source
        .iter_depth_first()
        .filter(|(id, _)| *id != subtree && source.contains_node(subtree, *id))
        .map(|(id, _)| id)
        .collect();
    let (detached, _) = source.detach_subtree(subtree).unwrap();
    let rect = Rectangle::new((100., 200.).into(), (600., 450.).into());

    let (resident, root, remapped) = TilingTree::from_detached_subtree(
        (1200., 800.).into(),
        rect,
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
        detached,
    );

    assert_eq!(resident.compute_geometry().ipc_nodes[&root], rect);
    assert!(remapped.is_empty());
    assert!(original_ids.into_iter().all(|id| resident.contains(id)));
    assert_ne!(root, subtree);
    assert!(source.contains(subtree));
    assert_eq!(resident.windows().count(), 2);
    resident.check_invariants();
}

#[test]
fn resident_subtree_can_be_detached_with_its_ids_and_focus_history() {
    let mut source = tree((1200., 800.), 0.);
    let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    source.split(first, Layout::Tabbed);
    source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);
    let subtree = source.nodes[&first].parent.unwrap();
    source.set_focus(first);
    let (detached, _) = source.detach_subtree(subtree).unwrap();
    let (resident, root, _) = TilingTree::from_detached_subtree(
        (1200., 800.).into(),
        Rectangle::new((100., 200.).into(), (600., 450.).into()),
        1.,
        Clock::with_time(Duration::ZERO),
        Rc::new(Options::default()),
        detached,
    );

    let detached = resident.detach_resident_root(root).unwrap();
    let mut destination = tree((1200., 800.), 0.);
    destination.add_tile(tile(3, destination.view_size()), InsertTarget::Focused);
    let (attached, remapped) = destination.attach_subtree(detached);

    assert_eq!(attached, root);
    assert!(remapped.is_empty());
    destination.set_focus(root);
    assert!(destination.focus_child());
    assert_eq!(destination.focus(), destination.node_for_window(&1));
    destination.check_invariants();
}

#[test]
fn detaching_the_full_root_resets_the_empty_workspace_layout() {
    let mut source = tree((800., 1200.), 0.);
    source.reset_empty_layout();
    source.set_layout(source.root, Layout::SplitH);
    let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    source.split(first, Layout::SplitV);
    source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);

    source.detach_subtree(source.root).unwrap();

    assert_eq!(source.representation_layout(), Layout::SplitH);
    source.check_invariants();
}

#[test]
fn attaching_split_to_empty_tree_preserves_root_state() {
    for layout in [Layout::SplitV, Layout::Tabbed] {
        let mut source = tree((1200., 800.), 0.);
        let first = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
        source.add_tile(tile(2, source.view_size()), InsertTarget::Focused);
        source.set_layout(source.root, Layout::SplitV);
        source.set_layout(source.root, layout);
        source.split_meta_mut(source.root).unwrap().previous_layout = Some(Layout::SplitH);
        source.set_title_format(source.root, "root format".into());
        source.set_node_fullscreen(source.root, Some(FullscreenMode::Workspace));
        let old_root = source.root;
        let mut destination = tree((1200., 800.), 0.);

        let (detached, _) = source.detach_subtree(old_root).unwrap();
        let (attached, remapped) = destination.attach_subtree(detached);

        assert_eq!(attached, destination.root);
        assert_eq!(remapped.len(), 1);
        assert_ne!(remapped[0].0, old_root);
        assert_eq!(remapped[0].1, destination.root);
        assert!(source.contains(old_root));
        assert!(matches!(
            destination.nodes[&destination.root].value,
            TreeNode::Split {
                layout: actual,
                ..
            } if actual == layout
        ));
        assert_eq!(
            destination.previous_layout(destination.root),
            Some(Layout::SplitH)
        );
        assert_eq!(
            destination
                .split_meta(destination.root)
                .and_then(|meta| meta.title_format.as_deref()),
            Some("root format")
        );
        assert_eq!(
            destination.fullscreen_mode(destination.root),
            Some(FullscreenMode::Workspace)
        );
        assert!(destination.contains(first));
        destination.check_invariants();
    }
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
fn attaching_a_fullscreen_subtree_replaces_the_destinations_fullscreen() {
    let mut source = tree((1200., 800.), 0.);
    let source_leaf = source.add_tile(tile(1, source.view_size()), InsertTarget::Focused);
    assert!(source.set_node_fullscreen(source_leaf, Some(FullscreenMode::Workspace)));
    let mut destination = tree((1200., 800.), 0.);
    let destination_leaf =
        destination.add_tile(tile(2, destination.view_size()), InsertTarget::Focused);
    assert!(destination.set_node_fullscreen(destination_leaf, Some(FullscreenMode::Workspace),));

    let (subtree, old_parent) = source.detach_subtree(source_leaf).unwrap();
    source.finish_subtree_detach(old_parent);
    let (attached, _) = destination.attach_subtree(subtree);

    assert_eq!(destination.fullscreen_node(), Some(attached));
    destination.check_invariants();
}

#[test]
fn swapping_a_maximized_leaf_with_a_split_keeps_maximize_on_a_leaf() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);
    t.split(first, Layout::SplitV);
    t.add_tile(tile(3, t.view_size()), InsertTarget::Focused);
    t.set_focus(second);
    t.split(second, Layout::SplitV);
    t.add_tile(tile(4, t.view_size()), InsertTarget::Focused);
    let other_split = t.nodes[&second].parent.unwrap();
    assert!(t.set_maximized(&1, true));

    t.swap_nodes(first, other_split).unwrap();

    assert!(t.is_pending_maximized(&1));
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
fn moving_a_subtree_to_its_existing_position_preserves_focus_order() {
    let mut t = tree((1200., 800.), 0.);
    let first = t.add_tile(tile(1, t.view_size()), InsertTarget::Focused);
    let second = t.add_tile(tile(2, t.view_size()), InsertTarget::Focused);

    assert!(t.move_subtree_to_node(second, first));

    assert_eq!(t.focused_child_in(t.root), Some(second));
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
            meta: SplitMeta::default(),
        },
    });
    t.nodes.get_mut(&b).unwrap().parent = Some(parent);
    t.nodes.get_mut(&t.root).unwrap().value = TreeNode::Split {
        layout: Layout::SplitH,
        children: vec![a, parent, c],
        percents: vec![1. / 3.; 3],
        meta: SplitMeta::default(),
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

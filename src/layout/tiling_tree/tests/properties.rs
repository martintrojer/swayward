use super::*;

#[derive(Debug, Clone)]
enum Op {
    Add,
    Remove(usize),
    Split(usize, Layout),
    SetLayout(usize, Layout),
    FocusDirection(Direction),
    FocusParent,
    FocusChild,
    Move(usize, Direction),
    ReorderFirst(usize),
    ReorderIndex(usize, usize),
    ReorderLast(usize),
    Resize(usize, usize, f64),
    Fullscreen(usize, bool),
    Maximize(usize, bool),
    ResizeSession(usize, Direction, f64),
    Consume(usize, bool),
    Expel(usize, bool),
    Swap(usize, usize),
    ToggleLayout(usize),
    Border(usize, BorderStyle),
    Drop(usize, ResizeEdge),
    Transfer(usize, bool),
}

fn layout_strategy() -> impl Strategy<Value = Layout> {
    prop_oneof![
        Just(Layout::SplitH),
        Just(Layout::SplitV),
        Just(Layout::Tabbed),
        Just(Layout::Stacked),
    ]
}

fn direction_strategy() -> impl Strategy<Value = Direction> {
    prop_oneof![
        Just(Direction::Left),
        Just(Direction::Right),
        Just(Direction::Up),
        Just(Direction::Down),
    ]
}

fn resize_edge_strategy() -> impl Strategy<Value = ResizeEdge> {
    prop_oneof![
        Just(ResizeEdge::LEFT),
        Just(ResizeEdge::RIGHT),
        Just(ResizeEdge::TOP),
        Just(ResizeEdge::BOTTOM),
        Just(ResizeEdge::empty()),
    ]
}

fn border_style_strategy() -> impl Strategy<Value = BorderStyle> {
    prop_oneof![
        Just(BorderStyle::Normal),
        Just(BorderStyle::Pixel),
        Just(BorderStyle::None),
        Just(BorderStyle::Toggle),
    ]
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Add),
        (0..32usize).prop_map(Op::Remove),
        (0..32usize, layout_strategy()).prop_map(|(id, layout)| Op::Split(id, layout)),
        (0..32usize, layout_strategy()).prop_map(|(id, layout)| Op::SetLayout(id, layout)),
        direction_strategy().prop_map(Op::FocusDirection),
        Just(Op::FocusParent),
        Just(Op::FocusChild),
        (0..32usize, direction_strategy()).prop_map(|(id, direction)| Op::Move(id, direction)),
        (0..32usize).prop_map(Op::ReorderFirst),
        (0..32usize, 0..32usize).prop_map(|(id, index)| Op::ReorderIndex(id, index)),
        (0..32usize).prop_map(Op::ReorderLast),
        (0..32usize, 0..32usize, -0.9f64..0.9)
            .prop_map(|(first, second, delta)| Op::Resize(first, second, delta)),
        (0..32usize, any::<bool>()).prop_map(|(id, value)| Op::Fullscreen(id, value)),
        (0..32usize, any::<bool>()).prop_map(|(id, value)| Op::Maximize(id, value)),
        (0..32usize, direction_strategy(), -1000f64..1000.)
            .prop_map(|(id, direction, delta)| Op::ResizeSession(id, direction, delta)),
        (0..32usize, any::<bool>()).prop_map(|(id, right)| Op::Consume(id, right)),
        (0..32usize, any::<bool>()).prop_map(|(id, right)| Op::Expel(id, right)),
        (0..32usize, 0..32usize).prop_map(|(first, second)| Op::Swap(first, second)),
        (0..32usize).prop_map(Op::ToggleLayout),
        (0..32usize, border_style_strategy()).prop_map(|(id, style)| Op::Border(id, style)),
        (0..32usize, resize_edge_strategy()).prop_map(|(id, edge)| Op::Drop(id, edge)),
        (0..32usize, any::<bool>()).prop_map(|(id, reverse)| Op::Transfer(id, reverse)),
    ]
}

fn sync_ids(tree: &TilingTree<TestWindow>, ids: &mut Vec<NodeId>) {
    ids.retain(|id| tree.windows().any(|(candidate, _)| candidate == *id));
}

/// Checks the GET_TREE snapshot: every node id appears once, a split's focus list is a
/// permutation of its children, and split percents are finite and non-negative.
///
/// It deliberately does not bound the per-split percent sum. Sway's child extents add up to the
/// parent box exactly (sway/tree/arrange.c:160-174, the last child takes the remainder), but
/// swayward rounds the parent extent and the children's extent separately, so ordinary splits
/// can sum just past 1. That is tracked as its own task and needs a pinned-sway row first.
fn check_ipc(node: &IpcNode<usize>, seen: &mut HashSet<NodeId>) {
    let id = match node {
        IpcNode::Split { id, .. } | IpcNode::Leaf { id, .. } => *id,
    };
    assert!(seen.insert(id), "node {id:?} appears twice in the IPC tree");
    let IpcNode::Split {
        focus, children, ..
    } = node
    else {
        return;
    };
    let child_ids = children
        .iter()
        .map(|child| match child {
            IpcNode::Split { id, .. } | IpcNode::Leaf { id, .. } => *id,
        })
        .collect::<Vec<_>>();
    let mut sorted_focus = focus.clone();
    sorted_focus.sort_by_key(|id| id.0);
    let mut sorted_children = child_ids.clone();
    sorted_children.sort_by_key(|id| id.0);
    assert_eq!(
        sorted_focus, sorted_children,
        "focus of {id:?} is not a permutation of its children"
    );
    let percents = children
        .iter()
        .map(|child| match child {
            IpcNode::Split { percent, .. } | IpcNode::Leaf { percent, .. } => *percent,
        })
        .collect::<Vec<_>>();
    for percent in percents.iter().flatten() {
        assert!(
            percent.is_finite() && *percent >= 0.,
            "child of {id:?} has percent {percent}"
        );
    }
    for child in children {
        check_ipc(child, seen);
    }
}

/// Under fullscreen only the fullscreen subtree is laid out, so containment and separation do
/// not apply; every rect must still be finite and non-negative.
fn check_rects_are_finite(tree: &TilingTree<TestWindow>) {
    let geometry = tree.compute_geometry();
    for (id, rect) in &geometry.ipc_nodes {
        assert!(
            rect.loc.x.is_finite()
                && rect.loc.y.is_finite()
                && rect.size.w.is_finite()
                && rect.size.h.is_finite()
                && rect.size.w >= 0.
                && rect.size.h >= 0.,
            "invalid geometry for {id:?}: {rect:?}"
        );
    }
}

fn check_geometry(tree: &TilingTree<TestWindow>) {
    let geometry = tree.compute_geometry();
    for (id, rect) in &geometry.ipc_nodes {
        assert!(
            rect.loc.x.is_finite()
                && rect.loc.y.is_finite()
                && rect.size.w.is_finite()
                && rect.size.h.is_finite()
                && rect.size.w >= 0.
                && rect.size.h >= 0.,
            "invalid geometry for {id:?}: {rect:?}"
        );
    }
    for (parent, node) in &tree.nodes {
        let TreeNode::Split {
            layout, children, ..
        } = &node.value
        else {
            continue;
        };
        let parent_rect = geometry.ipc_nodes[parent];
        for child in children {
            let child_rect = geometry.ipc_nodes[child];
            if child_rect.size.w > 0. && child_rect.size.h > 0. {
                assert!(
                    child_rect.loc.x >= parent_rect.loc.x - 1e-6
                        && child_rect.loc.y >= parent_rect.loc.y - 1e-6
                        && child_rect.loc.x + child_rect.size.w
                            <= parent_rect.loc.x + parent_rect.size.w + 1e-6
                        && child_rect.loc.y + child_rect.size.h
                            <= parent_rect.loc.y + parent_rect.size.h + 1e-6,
                    "child {child:?} lies outside parent {parent:?}"
                );
            }
        }
        if matches!(layout, Layout::SplitH | Layout::SplitV) {
            for pair in children.windows(2) {
                let first = geometry.ipc_nodes[&pair[0]];
                let second = geometry.ipc_nodes[&pair[1]];
                let separated = match layout {
                    Layout::SplitH => first.loc.x + first.size.w <= second.loc.x + 1e-6,
                    Layout::SplitV => first.loc.y + first.size.h <= second.loc.y + 1e-6,
                    _ => unreachable!(),
                };
                assert!(separated, "split children overlap in {parent:?}");
            }
        }
    }
}

fn tiling_tree_proptest_cases() -> u32 {
    if std::env::var_os("RUN_SLOW_TESTS").is_none() {
        16
    } else {
        ProptestConfig::default().cases
    }
}

fn run_operations(ops: Vec<Op>) {
    let mut tree = tree((1920., 1080.), 8.);
    let mut peer = super::tests::tree((1280., 720.), 4.);
    let mut ids = Vec::new();
    let mut next_window = 0;
    for op in ops {
        match op {
            Op::Add => {
                ids.push(tree.add_tile(tile(next_window, tree.view_size()), InsertTarget::Focused));
                next_window += 1;
            }
            Op::Remove(index) => {
                if !ids.is_empty() {
                    let id = ids.remove(index % ids.len());
                    tree.remove_tile_node(id);
                }
            }
            Op::Split(index, layout) => {
                if !ids.is_empty() {
                    tree.split(ids[index % ids.len()], layout);
                }
            }
            Op::SetLayout(index, layout) => {
                let nodes: Vec<_> = tree.iter_depth_first().map(|(id, _)| id).collect();
                if !nodes.is_empty() {
                    tree.set_layout(nodes[index % nodes.len()], layout);
                }
            }
            Op::FocusDirection(direction) => {
                tree.focus_direction(direction);
            }
            Op::FocusParent => {
                tree.focus_parent();
            }
            Op::FocusChild => {
                tree.focus_child();
            }
            Op::Move(index, direction) => {
                if !ids.is_empty() {
                    tree.move_direction(ids[index % ids.len()], direction);
                }
            }
            Op::ReorderFirst(index) => {
                let nodes: Vec<_> = tree.iter_depth_first().map(|(id, _)| id).collect();
                if !nodes.is_empty() {
                    tree.move_subtree_to_first(nodes[index % nodes.len()]);
                }
            }
            Op::ReorderIndex(id, index) => {
                let nodes: Vec<_> = tree.iter_depth_first().map(|(id, _)| id).collect();
                if !nodes.is_empty() {
                    tree.move_subtree_to_index(nodes[id % nodes.len()], index);
                }
            }
            Op::ReorderLast(index) => {
                let nodes: Vec<_> = tree.iter_depth_first().map(|(id, _)| id).collect();
                if !nodes.is_empty() {
                    tree.move_subtree_to_last(nodes[index % nodes.len()]);
                }
            }
            Op::Resize(first, second, delta) => {
                if !ids.is_empty() {
                    tree.resize_adjacent(ids[first % ids.len()], ids[second % ids.len()], delta);
                }
            }
            Op::Fullscreen(index, value) => {
                if !ids.is_empty() {
                    let window = tree
                        .windows()
                        .find(|(id, _)| *id == ids[index % ids.len()])
                        .map(|(_, window)| *window.id());
                    if let Some(window) = window {
                        tree.set_fullscreen(&window, value);
                    }
                }
            }
            Op::Maximize(index, value) => {
                if !ids.is_empty() {
                    let window = tree
                        .windows()
                        .find(|(id, _)| *id == ids[index % ids.len()])
                        .map(|(_, window)| *window.id());
                    if let Some(window) = window {
                        tree.set_maximized(&window, value);
                    }
                }
            }
            Op::ResizeSession(index, direction, delta) => {
                if !ids.is_empty() {
                    let window = tree
                        .windows()
                        .find(|(id, _)| *id == ids[index % ids.len()])
                        .map(|(_, window)| *window.id());
                    if let Some(window) = window {
                        let edge = match direction {
                            Direction::Left => crate::utils::ResizeEdge::LEFT,
                            Direction::Right => crate::utils::ResizeEdge::RIGHT,
                            Direction::Up => crate::utils::ResizeEdge::TOP,
                            Direction::Down => crate::utils::ResizeEdge::BOTTOM,
                        };
                        if tree.interactive_resize_begin(window, edge) {
                            tree.interactive_resize_update(&window, Point::from((delta, delta)));
                            tree.interactive_resize_end(Some(&window));
                        }
                    }
                }
            }
            Op::Consume(index, right) => {
                if !ids.is_empty() {
                    tree.consume(ids[index % ids.len()], right);
                }
            }
            Op::Expel(index, right) => {
                if !ids.is_empty() {
                    tree.expel(ids[index % ids.len()], right);
                }
            }
            Op::Swap(first, second) => {
                let nodes: Vec<_> = tree.iter_depth_first().map(|(id, _)| id).collect();
                let first = nodes
                    .get(first)
                    .copied()
                    .unwrap_or(NodeId(u64::MAX - first as u64));
                let second = nodes
                    .get(second)
                    .copied()
                    .unwrap_or(NodeId(u64::MAX - second as u64));
                let _ = tree.swap_nodes(first, second);
            }
            Op::ToggleLayout(index) => {
                let nodes: Vec<_> = tree.iter_depth_first().map(|(id, _)| id).collect();
                if !nodes.is_empty() {
                    tree.toggle_node_layout(nodes[index % nodes.len()], &LayoutToggle::All);
                }
            }
            Op::Border(index, style) => {
                let windows: Vec<_> = tree.windows().map(|(_, window)| *window.id()).collect();
                if !windows.is_empty() {
                    tree.set_window_border(&windows[index % windows.len()], style, Some(3));
                }
            }
            Op::Drop(index, edge) => {
                let nodes: Vec<_> = tree.windows().map(|(id, _)| id).collect();
                if !nodes.is_empty() {
                    let target = nodes[index % nodes.len()];
                    ids.push(tree.add_tile_at_drop(
                        tile(next_window, tree.view_size()),
                        target,
                        edge,
                        true,
                    ));
                    next_window += 1;
                }
            }
            Op::Transfer(index, reverse) => {
                let (source, destination) = if reverse {
                    (&mut peer, &mut tree)
                } else {
                    (&mut tree, &mut peer)
                };
                let nodes: Vec<_> = source
                    .iter_depth_first()
                    .map(|(id, _)| id)
                    .filter(|id| *id != source.root)
                    .collect();
                if let Some(id) = nodes.get(index % nodes.len().max(1)).copied() {
                    if let Some((subtree, old_parent)) = source.detach_subtree(id) {
                        source.finish_subtree_detach(old_parent);
                        destination.attach_subtree(subtree);
                    }
                }
            }
        }
        sync_ids(&tree, &mut ids);
        tree.check_invariants();
        peer.check_invariants();
        for tree in [&tree, &peer] {
            check_ipc(&tree.ipc_tree(), &mut HashSet::new());
            if tree.fullscreen_node().is_none() {
                check_geometry(tree);
            } else {
                check_rects_are_finite(tree);
            }
        }
    }
}

// Shrunk sequences behind the `cc` seeds in
// proptest-regressions/layout/tiling_tree/tests/properties.txt, replayed by name so the minimal
// reproduction survives even if the seed file is regenerated.

#[test]
fn regression_stacked_then_tabbed_split_survives_window_removal() {
    // Seed 08e57fa6, fixed by 216d2bb4: a one-window directional move read its parent before
    // root compaction and left that parent empty.
    run_operations(vec![
        Op::Add,
        Op::FocusChild,
        Op::FocusChild,
        Op::Split(0, Layout::Stacked),
        Op::Split(0, Layout::Tabbed),
        Op::SetLayout(2, Layout::SplitV),
        Op::Add,
        Op::Move(11, Direction::Up),
        Op::SetLayout(18, Layout::SplitH),
        Op::Remove(12),
        Op::Move(0, Direction::Left),
    ]);
}

#[test]
fn regression_transfer_fullscreen_and_drop_sequence() {
    // Seed da2b5410, recorded by c15d1fbd (broaden tiling-tree mutation properties). The
    // shrunk sequence no longer fails on c15d1fbd with its swap or attach fix reverted, so this
    // replays the recorded case rather than pinning a known failure.
    run_operations(vec![
        Op::Add,
        Op::Add,
        Op::Remove(5),
        Op::Add,
        Op::ReorderLast(24),
        Op::Add,
        Op::Add,
        Op::Resize(9, 10, 0.3883400016140841),
        Op::Expel(13, false),
        Op::Remove(18),
        Op::ReorderFirst(19),
        Op::Expel(1, false),
        Op::Add,
        Op::Consume(21, true),
        Op::SetLayout(10, Layout::Stacked),
        Op::Add,
        Op::Transfer(3, true),
        Op::ReorderIndex(14, 31),
        Op::ResizeSession(24, Direction::Down, 41.03084835101202),
        Op::ToggleLayout(17),
        Op::FocusParent,
        Op::Maximize(3, false),
        Op::Maximize(9, true),
        Op::FocusParent,
        Op::Transfer(9, true),
        Op::Fullscreen(9, false),
        Op::Transfer(5, true),
        Op::Resize(3, 10, 0.13694612537910983),
        Op::Swap(9, 15),
        Op::Split(4, Layout::Tabbed),
        Op::Fullscreen(6, false),
        Op::Drop(3, ResizeEdge::BOTTOM),
        Op::Swap(20, 30),
        Op::Maximize(14, false),
        Op::ReorderIndex(25, 5),
        Op::Drop(6, ResizeEdge::TOP),
        Op::ToggleLayout(19),
    ]);
}

#[test]
fn regression_sub_pixel_last_child_reports_a_non_negative_percent() {
    // Integrator batch 7 shrunk case (no `cc` line was saved): a resize leaves the last child
    // of a split 0.23 px tall. Rounding the earlier children's shares up then left the last
    // child -1 px, and GET_TREE reported a negative percent. The direct unit test is
    // tests/geometry.rs a_sub_pixel_last_child_reports_a_non_negative_percent.
    run_operations(vec![
        Op::Remove(0),
        Op::Add,
        Op::Maximize(0, false),
        Op::Split(0, Layout::Tabbed),
        Op::Drop(0, ResizeEdge::TOP),
        Op::Add,
        Op::Resize(17, 15, 0.33303451250346383),
        Op::Add,
    ]);
}

#[test]
fn regression_fullscreen_transfer_after_drop_and_expel() {
    // Seed 30a67654, fixed by c15d1fbd: attaching a fullscreen subtree left two fullscreen
    // nodes until attach_subtree_at cleared the destination's.
    run_operations(vec![
        Op::Add,
        Op::Drop(0, ResizeEdge::BOTTOM),
        Op::Fullscreen(0, true),
        Op::Add,
        Op::Split(4, Layout::SplitH),
        Op::Consume(8, false),
        Op::Transfer(12, false),
        Op::Add,
        Op::Fullscreen(3, true),
        Op::Remove(2),
        Op::Expel(0, false),
        Op::Transfer(8, false),
    ]);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: tiling_tree_proptest_cases(),
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_operations_preserve_invariants(ops in prop::collection::vec(op_strategy(), 0..100)) {
        run_operations(ops);
    }
}

#[derive(Debug, Clone)]
enum FloatingOp {
    Add,
    Split(usize, Layout),
    Float(usize),
    Unfloat,
    Move(i16, i16),
    OuterResize(u16, u16),
    InternalResize(usize, usize, f64),
    FocusParent,
    FocusChild,
    Close(usize),
    CrossWorkspace,
}

fn floating_op_strategy() -> impl Strategy<Value = FloatingOp> {
    prop_oneof![
        Just(FloatingOp::Add),
        (0..32usize, layout_strategy()).prop_map(|(id, layout)| FloatingOp::Split(id, layout)),
        (0..32usize).prop_map(FloatingOp::Float),
        Just(FloatingOp::Unfloat),
        (any::<i16>(), any::<i16>()).prop_map(|(x, y)| FloatingOp::Move(x, y)),
        (1..2000u16, 1..1200u16).prop_map(|(w, h)| FloatingOp::OuterResize(w, h)),
        (0..32usize, 0..32usize, -0.9f64..0.9)
            .prop_map(|(a, b, delta)| FloatingOp::InternalResize(a, b, delta)),
        Just(FloatingOp::FocusParent),
        Just(FloatingOp::FocusChild),
        (0..32usize).prop_map(FloatingOp::Close),
        Just(FloatingOp::CrossWorkspace),
    ]
}

fn run_resident_operations(ops: Vec<FloatingOp>) {
    let view_size = Size::from((1920., 1080.));
    let mut tiled = tree((1920., 1080.), 8.);
    let mut resident: Option<(TilingTree<TestWindow>, NodeId)> = None;
    let mut next_window = 0;
    for op in ops {
        match op {
            FloatingOp::Add => {
                if let Some((tree, _)) = &mut resident {
                    let parent = tree
                        .focus()
                        .filter(|id| tree.is_split(*id))
                        .or_else(|| tree.windows().next().map(|(id, _)| id));
                    if let Some(parent) = parent {
                        tree.add_tile_to_subtree(parent, tile(next_window, tree.view_size()), true);
                    }
                } else {
                    tiled.add_tile(tile(next_window, tiled.view_size()), InsertTarget::Focused);
                }
                next_window += 1;
            }
            FloatingOp::Split(index, layout) => {
                let target = resident
                    .as_mut()
                    .map(|(tree, _)| tree)
                    .unwrap_or(&mut tiled);
                let leaves: Vec<_> = target.windows().map(|(id, _)| id).collect();
                if let Some(id) = leaves.get(index % leaves.len().max(1)).copied() {
                    target.split(id, layout);
                }
            }
            FloatingOp::Float(index) if resident.is_none() => {
                let nodes: Vec<_> = tiled
                    .iter_depth_first()
                    .filter_map(|(id, node)| {
                        (id != tiled.root
                            && matches!(node, TreeNode::Split { .. })
                            && tiled.leaf_ids_in(id).len() > 1)
                            .then_some(id)
                    })
                    .collect();
                if let Some(id) = nodes.get(index % nodes.len().max(1)).copied() {
                    if tiled.leaf_ids_in(id).len() > 1 {
                        if let Some((subtree, old_parent)) = tiled.detach_subtree(id) {
                            tiled.finish_subtree_detach(old_parent);
                            let rect = Rectangle::new((100., 100.).into(), (800., 600.).into());
                            let (tree, root, _) = TilingTree::from_detached_subtree(
                                view_size,
                                rect,
                                1.,
                                Clock::with_time(Duration::ZERO),
                                Rc::new(Options::default()),
                                subtree,
                            );
                            resident = Some((tree, root));
                        }
                    }
                }
            }
            FloatingOp::Unfloat => {
                if let Some((tree, root)) = resident.take() {
                    if let Some(subtree) = tree.detach_resident_root(root) {
                        tiled.attach_subtree(subtree);
                    }
                }
            }
            FloatingOp::Move(x, y) => {
                if let Some((tree, _)) = &mut resident {
                    let old = tree.parent_area();
                    tree.update_config(
                        view_size,
                        Rectangle::new((f64::from(x), f64::from(y)).into(), old.size),
                        false,
                        1.,
                        Rc::new(Options::default()),
                    );
                }
            }
            FloatingOp::OuterResize(w, h) => {
                if let Some((tree, _)) = &mut resident {
                    let old = tree.parent_area();
                    tree.update_config(
                        view_size,
                        Rectangle::new(old.loc, (f64::from(w), f64::from(h)).into()),
                        false,
                        1.,
                        Rc::new(Options::default()),
                    );
                }
            }
            FloatingOp::InternalResize(first, second, delta) => {
                if let Some((tree, _)) = &mut resident {
                    let ids: Vec<_> = tree.windows().map(|(id, _)| id).collect();
                    if !ids.is_empty() {
                        tree.resize_adjacent(
                            ids[first % ids.len()],
                            ids[second % ids.len()],
                            delta,
                        );
                    }
                }
            }
            FloatingOp::FocusParent => {
                if let Some((tree, _)) = &mut resident {
                    tree.focus_parent();
                }
            }
            FloatingOp::FocusChild => {
                if let Some((tree, _)) = &mut resident {
                    tree.focus_child();
                }
            }
            FloatingOp::Close(index) => {
                if let Some((tree, _)) = &mut resident {
                    let ids: Vec<_> = tree.windows().map(|(id, _)| id).collect();
                    if let Some(id) = ids.get(index % ids.len().max(1)).copied() {
                        tree.remove_tile_node(id);
                    }
                }
                if resident.as_ref().is_some_and(|(tree, _)| tree.is_empty()) {
                    resident = None;
                }
            }
            FloatingOp::CrossWorkspace => {
                if let Some((tree, root)) = resident.take() {
                    if let Some(subtree) = tree.detach_resident_root(root) {
                        let rect = Rectangle::new((200., 150.).into(), (700., 500.).into());
                        let (tree, root, _) = TilingTree::from_detached_subtree(
                            view_size,
                            rect,
                            1.,
                            Clock::with_time(Duration::ZERO),
                            Rc::new(Options::default()),
                            subtree,
                        );
                        resident = Some((tree, root));
                    }
                }
            }
            FloatingOp::Float(_) => {}
        }
        tiled.check_invariants();
        check_geometry(&tiled);
        if let Some((tree, _)) = &resident {
            tree.check_invariants();
            assert!(!tree.is_empty());
            check_geometry(tree);
            let tiled_ids: HashSet<_> = tiled.iter_depth_first().map(|(id, _)| id).collect();
            assert!(tree
                .iter_depth_first()
                .all(|(id, _)| !tiled_ids.contains(&id)));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: tiling_tree_proptest_cases(),
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_resident_tree_operations_preserve_invariants(
        ops in prop::collection::vec(floating_op_strategy(), 0..100)
    ) {
        run_resident_operations(ops);
    }
}

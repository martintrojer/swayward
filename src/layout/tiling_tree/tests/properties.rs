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

/// State for one `run_operations` case: the tree under test, a peer tree for cross-tree
/// transfers, the leaves added so far and the next window id.
struct PropState {
    tree: TilingTree<TestWindow>,
    peer: TilingTree<TestWindow>,
    ids: Vec<NodeId>,
    next_window: usize,
}

impl PropState {
    fn new() -> Self {
        Self {
            tree: tree((1920., 1080.), 8.),
            peer: super::tests::tree((1280., 720.), 4.),
            ids: Vec::new(),
            next_window: 0,
        }
    }

    /// A tracked leaf, chosen by wrapping `index` over the live leaves.
    fn pick_leaf(&self, index: usize) -> Option<NodeId> {
        (!self.ids.is_empty()).then(|| self.ids[index % self.ids.len()])
    }

    /// Any node in depth-first order, chosen by wrapping `index`.
    fn pick_node(&self, index: usize) -> Option<NodeId> {
        let nodes: Vec<_> = self.tree.iter_depth_first().map(|(id, _)| id).collect();
        (!nodes.is_empty()).then(|| nodes[index % nodes.len()])
    }

    /// The window id of a tracked leaf.
    fn pick_window(&self, index: usize) -> Option<usize> {
        let leaf = self.pick_leaf(index)?;
        self.tree
            .windows()
            .find(|(id, _)| *id == leaf)
            .map(|(_, window)| *window.id())
    }

    fn next_tile(&mut self) -> Tile<TestWindow> {
        let tile = tile(self.next_window, self.tree.view_size());
        self.next_window += 1;
        tile
    }

    fn apply(&mut self, op: Op) {
        match op {
            Op::Add
            | Op::Remove(_)
            | Op::Split(..)
            | Op::SetLayout(..)
            | Op::ToggleLayout(_)
            | Op::Consume(..)
            | Op::Expel(..)
            | Op::Drop(..) => self.apply_structure(op),
            Op::FocusDirection(_) | Op::FocusParent | Op::FocusChild => self.apply_focus(op),
            Op::Move(..)
            | Op::ReorderFirst(_)
            | Op::ReorderIndex(..)
            | Op::ReorderLast(_)
            | Op::Swap(..)
            | Op::Transfer(..) => self.apply_transfer(op),
            Op::Resize(..)
            | Op::Fullscreen(..)
            | Op::Maximize(..)
            | Op::ResizeSession(..)
            | Op::Border(..) => self.apply_sizing(op),
        }
    }

    fn apply_structure(&mut self, op: Op) {
        match op {
            Op::Add => {
                let tile = self.next_tile();
                self.ids
                    .push(self.tree.add_tile(tile, InsertTarget::Focused));
            }
            Op::Remove(index) => {
                if !self.ids.is_empty() {
                    let id = self.ids.remove(index % self.ids.len());
                    self.tree.remove_tile_node(id);
                }
            }
            Op::Split(index, layout) => {
                if let Some(id) = self.pick_leaf(index) {
                    self.tree.split(id, layout);
                }
            }
            Op::SetLayout(index, layout) => {
                if let Some(id) = self.pick_node(index) {
                    self.tree.set_layout(id, layout);
                }
            }
            Op::ToggleLayout(index) => {
                if let Some(id) = self.pick_node(index) {
                    self.tree.toggle_node_layout(id, &LayoutToggle::All);
                }
            }
            Op::Consume(index, right) => {
                if let Some(id) = self.pick_leaf(index) {
                    self.tree.consume(id, right);
                }
            }
            Op::Expel(index, right) => {
                if let Some(id) = self.pick_leaf(index) {
                    self.tree.expel(id, right);
                }
            }
            Op::Drop(index, edge) => {
                let nodes: Vec<_> = self.tree.windows().map(|(id, _)| id).collect();
                if !nodes.is_empty() {
                    let target = nodes[index % nodes.len()];
                    let tile = self.next_tile();
                    self.ids
                        .push(self.tree.add_tile_at_drop(tile, target, edge, true));
                }
            }
            _ => unreachable!("not a structure op: {op:?}"),
        }
    }

    fn apply_focus(&mut self, op: Op) {
        match op {
            Op::FocusDirection(direction) => {
                self.tree.focus_direction(direction);
            }
            Op::FocusParent => {
                self.tree.focus_parent();
            }
            Op::FocusChild => {
                self.tree.focus_child();
            }
            _ => unreachable!("not a focus op: {op:?}"),
        }
    }

    fn apply_transfer(&mut self, op: Op) {
        match op {
            Op::Move(index, direction) => {
                if let Some(id) = self.pick_leaf(index) {
                    self.tree.move_direction(id, direction);
                }
            }
            Op::ReorderFirst(index) => {
                if let Some(id) = self.pick_node(index) {
                    self.tree.move_subtree_to_first(id);
                }
            }
            Op::ReorderIndex(id, index) => {
                if let Some(id) = self.pick_node(id) {
                    self.tree.move_subtree_to_index(id, index);
                }
            }
            Op::ReorderLast(index) => {
                if let Some(id) = self.pick_node(index) {
                    self.tree.move_subtree_to_last(id);
                }
            }
            Op::Swap(first, second) => {
                // Out-of-range picks become ids that are not in the tree, so swaps also exercise
                // the invalid-node path.
                let nodes: Vec<_> = self.tree.iter_depth_first().map(|(id, _)| id).collect();
                let pick = |index: usize| {
                    nodes
                        .get(index)
                        .copied()
                        .unwrap_or(NodeId(u64::MAX - index as u64))
                };
                let _ = self.tree.swap_nodes(pick(first), pick(second));
            }
            Op::Transfer(index, reverse) => {
                let (source, destination) = if reverse {
                    (&mut self.peer, &mut self.tree)
                } else {
                    (&mut self.tree, &mut self.peer)
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
            _ => unreachable!("not a transfer op: {op:?}"),
        }
    }

    fn apply_sizing(&mut self, op: Op) {
        match op {
            Op::Resize(first, second, delta) => {
                if let (Some(first), Some(second)) = (self.pick_leaf(first), self.pick_leaf(second))
                {
                    self.tree.resize_adjacent(first, second, delta);
                }
            }
            Op::Fullscreen(index, value) => {
                if let Some(window) = self.pick_window(index) {
                    self.tree.set_fullscreen(&window, value);
                }
            }
            Op::Maximize(index, value) => {
                if let Some(window) = self.pick_window(index) {
                    self.tree.set_maximized(&window, value);
                }
            }
            Op::ResizeSession(index, direction, delta) => {
                if let Some(window) = self.pick_window(index) {
                    let edge = match direction {
                        Direction::Left => crate::utils::ResizeEdge::LEFT,
                        Direction::Right => crate::utils::ResizeEdge::RIGHT,
                        Direction::Up => crate::utils::ResizeEdge::TOP,
                        Direction::Down => crate::utils::ResizeEdge::BOTTOM,
                    };
                    if self.tree.interactive_resize_begin(window, edge) {
                        self.tree
                            .interactive_resize_update(&window, Point::from((delta, delta)));
                        self.tree.interactive_resize_end(Some(&window));
                    }
                }
            }
            Op::Border(index, style) => {
                let windows: Vec<_> = self
                    .tree
                    .windows()
                    .map(|(_, window)| *window.id())
                    .collect();
                if !windows.is_empty() {
                    self.tree
                        .set_window_border(&windows[index % windows.len()], style, Some(3));
                }
            }
            _ => unreachable!("not a sizing op: {op:?}"),
        }
    }

    fn check(&mut self) {
        sync_ids(&self.tree, &mut self.ids);
        self.tree.check_invariants();
        self.peer.check_invariants();
        for tree in [&self.tree, &self.peer] {
            check_ipc(&tree.ipc_tree(), &mut HashSet::new());
            if tree.fullscreen_node().is_none() {
                check_geometry(tree);
            } else {
                check_rects_are_finite(tree);
            }
        }
    }
}

fn run_operations(ops: Vec<Op>) {
    let mut state = PropState::new();
    for op in ops {
        state.apply(op);
        state.check();
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

/// State for one `run_resident_operations` case: a tiled tree and, once a split has been
/// floated, the resident tree that holds it with its root.
struct ResidentState {
    tiled: TilingTree<TestWindow>,
    resident: Option<(TilingTree<TestWindow>, NodeId)>,
    next_window: usize,
}

impl ResidentState {
    const VIEW_SIZE: (f64, f64) = (1920., 1080.);

    fn new() -> Self {
        Self {
            tiled: tree(Self::VIEW_SIZE, 8.),
            resident: None,
            next_window: 0,
        }
    }

    /// Builds a resident tree for `subtree` at `rect`, as a floating group would hold it.
    fn float(
        subtree: DetachedSubtree<TestWindow>,
        rect: Rectangle<f64, Logical>,
    ) -> (TilingTree<TestWindow>, NodeId) {
        let (tree, root, _) = TilingTree::from_detached_subtree(
            Size::from(Self::VIEW_SIZE),
            rect,
            1.,
            Clock::with_time(Duration::ZERO),
            Rc::new(Options::default()),
            subtree,
        );
        (tree, root)
    }

    /// Reconfigures the resident tree's parent area, as moving or resizing its group does.
    fn reconfigure(
        &mut self,
        area: impl FnOnce(Rectangle<f64, Logical>) -> Rectangle<f64, Logical>,
    ) {
        if let Some((tree, _)) = &mut self.resident {
            let area = area(tree.parent_area());
            tree.update_config(
                Size::from(Self::VIEW_SIZE),
                area,
                false,
                1.,
                Rc::new(Options::default()),
            );
        }
    }

    /// A resident leaf, chosen by wrapping `index`.
    fn pick_resident_leaf(&self, index: usize) -> Option<NodeId> {
        let (tree, _) = self.resident.as_ref()?;
        let ids: Vec<_> = tree.windows().map(|(id, _)| id).collect();
        (!ids.is_empty()).then(|| ids[index % ids.len()])
    }

    fn apply(&mut self, op: FloatingOp) {
        match op {
            FloatingOp::Add => self.add(),
            FloatingOp::Split(index, layout) => {
                let target = self
                    .resident
                    .as_mut()
                    .map(|(tree, _)| tree)
                    .unwrap_or(&mut self.tiled);
                let leaves: Vec<_> = target.windows().map(|(id, _)| id).collect();
                if let Some(id) = leaves.get(index % leaves.len().max(1)).copied() {
                    target.split(id, layout);
                }
            }
            FloatingOp::Float(index) => self.float_split(index),
            FloatingOp::Unfloat => {
                if let Some((tree, root)) = self.resident.take() {
                    if let Some(subtree) = tree.detach_resident_root(root) {
                        self.tiled.attach_subtree(subtree);
                    }
                }
            }
            FloatingOp::Move(x, y) => {
                self.reconfigure(|old| {
                    Rectangle::new((f64::from(x), f64::from(y)).into(), old.size)
                });
            }
            FloatingOp::OuterResize(w, h) => {
                self.reconfigure(|old| {
                    Rectangle::new(old.loc, (f64::from(w), f64::from(h)).into())
                });
            }
            FloatingOp::InternalResize(first, second, delta) => {
                if let (Some(first), Some(second)) = (
                    self.pick_resident_leaf(first),
                    self.pick_resident_leaf(second),
                ) {
                    if let Some((tree, _)) = &mut self.resident {
                        tree.resize_adjacent(first, second, delta);
                    }
                }
            }
            FloatingOp::FocusParent => {
                if let Some((tree, _)) = &mut self.resident {
                    tree.focus_parent();
                }
            }
            FloatingOp::FocusChild => {
                if let Some((tree, _)) = &mut self.resident {
                    tree.focus_child();
                }
            }
            FloatingOp::Close(index) => {
                if let Some(id) = self.pick_resident_leaf(index) {
                    if let Some((tree, _)) = &mut self.resident {
                        tree.remove_tile_node(id);
                    }
                }
                if self
                    .resident
                    .as_ref()
                    .is_some_and(|(tree, _)| tree.is_empty())
                {
                    self.resident = None;
                }
            }
            FloatingOp::CrossWorkspace => {
                if let Some((tree, root)) = self.resident.take() {
                    if let Some(subtree) = tree.detach_resident_root(root) {
                        let rect = Rectangle::new((200., 150.).into(), (700., 500.).into());
                        self.resident = Some(Self::float(subtree, rect));
                    }
                }
            }
        }
    }

    fn add(&mut self) {
        let window = self.next_window;
        self.next_window += 1;
        if let Some((tree, _)) = &mut self.resident {
            let parent = tree
                .focus()
                .filter(|id| tree.is_split(*id))
                .or_else(|| tree.windows().next().map(|(id, _)| id));
            if let Some(parent) = parent {
                tree.add_tile_to_subtree(parent, tile(window, tree.view_size()), true);
            }
        } else {
            self.tiled
                .add_tile(tile(window, self.tiled.view_size()), InsertTarget::Focused);
        }
    }

    /// Floats a multi-window split of the tiled tree, while nothing is floating yet.
    fn float_split(&mut self, index: usize) {
        if self.resident.is_some() {
            return;
        }
        let tiled = &self.tiled;
        let nodes: Vec<_> = tiled
            .iter_depth_first()
            .filter_map(|(id, node)| {
                (id != tiled.root
                    && matches!(node, TreeNode::Split { .. })
                    && tiled.leaf_ids_in(id).len() > 1)
                    .then_some(id)
            })
            .collect();
        let Some(id) = nodes.get(index % nodes.len().max(1)).copied() else {
            return;
        };
        if self.tiled.leaf_ids_in(id).len() < 2 {
            return;
        }
        if let Some((subtree, old_parent)) = self.tiled.detach_subtree(id) {
            self.tiled.finish_subtree_detach(old_parent);
            let rect = Rectangle::new((100., 100.).into(), (800., 600.).into());
            self.resident = Some(Self::float(subtree, rect));
        }
    }

    fn check(&self) {
        self.tiled.check_invariants();
        check_geometry(&self.tiled);
        if let Some((tree, _)) = &self.resident {
            tree.check_invariants();
            assert!(!tree.is_empty());
            check_geometry(tree);
            let tiled_ids: HashSet<_> = self.tiled.iter_depth_first().map(|(id, _)| id).collect();
            assert!(tree
                .iter_depth_first()
                .all(|(id, _)| !tiled_ids.contains(&id)));
        }
    }
}

fn run_resident_operations(ops: Vec<FloatingOp>) {
    let mut state = ResidentState::new();
    for op in ops {
        state.apply(op);
        state.check();
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

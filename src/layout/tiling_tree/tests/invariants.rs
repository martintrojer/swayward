use super::*;

/// Records `id` in the side table `name`. Panics on a table it does not know, so a table added
/// to `side_tables!` fails the side-table tests until it is covered here.
fn seed_side_table(t: &mut TilingTree<TestWindow>, name: &str, id: NodeId) {
    match name {
        "focus_history" => t.focus_history.push(id),
        "ipc_stale_nodes" => {
            t.ipc_stale_nodes.insert(id);
        }
        "pending_modes" => {
            t.pending_modes.insert(id, PendingMode::default());
        }
        "mapped_under_fullscreen" => {
            t.mapped_under_fullscreen.insert(id);
        }
        "moved_under_fullscreen" => {
            t.moved_under_fullscreen.insert(id, Rectangle::default());
        }
        "fullscreen_layout_wrappers" => {
            t.fullscreen_layout_wrappers.insert(id);
        }
        "pre_layout_ipc_rects" => {
            t.pre_layout_ipc_rects.insert(id, Rectangle::default());
        }
        "tab_indicators" => {
            t.tab_indicators
                .insert(id, TabIndicator::new(t.options.layout.tab_indicator));
        }
        "tab_active" => {
            t.tab_active.insert(id, id);
        }
        "last_entered_by" => {
            t.last_entered_by.insert(id, id);
        }
        other => panic!("seed_side_table does not cover side table {other}"),
    }
}

fn side_table_names(t: &mut TilingTree<TestWindow>) -> Vec<&'static str> {
    side_tables!(t, &).iter().map(|(name, _)| *name).collect()
}

#[test]
fn invariant_rejects_stale_and_duplicate_node_side_state() {
    let names = side_table_names(&mut tree((1920., 1080.), 0.));
    for name in names {
        let mut t = tree((1920., 1080.), 0.);
        // The node counter is process-global and proptests allocate from it concurrently, so a
        // small literal id can belong to this tree. The counter never reaches u64::MAX.
        seed_side_table(&mut t, name, NodeId(u64::MAX));
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                t.check_invariants();
            }))
            .is_err(),
            "a stale id in {name} must fail the invariants"
        );
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
    for name in side_table_names(&mut t) {
        seed_side_table(&mut t, name, leaf);
    }

    t.remove_tile_node(leaf);

    for (name, table) in side_tables!(t, &) {
        assert!(
            table.ids().all(|id| id != leaf),
            "{name} still holds the removed node"
        );
    }
}

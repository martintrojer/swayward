//! Tiling-tree moves and focus.

use super::*;

#[test]
fn moving_subtree_to_node_cleans_source_after_attachment() {
    let mut layout = Layout::default();
    Op::AddOutput(1).apply(&mut layout);
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("source".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams::new(0),
    }
    .apply(&mut layout);
    let (source, node) = layout.tiling_target_for_window(&0).unwrap();
    layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("target".into()))
        .unwrap();
    Op::AddWindow {
        params: TestWindowParams::new(1),
    }
    .apply(&mut layout);
    let (target, target_node) = layout.tiling_target_for_window(&1).unwrap();
    layout.active_monitor().unwrap().workspace_switch = None;

    layout
        .move_tiling_subtree_to_node(source, node, target, target_node)
        .unwrap();

    assert_eq!(layout.window_workspace_id(&0), Some(target));
    assert!(layout.find_workspace_by_id(source).is_none());
}

#[test]
fn focus_parent_then_move_left_keeps_focus_on_a_live_node() {
    // CI 35986895235 shrank `random_operations_dont_panic` to this sequence
    // (proptest cc acc67c75). Moving a focused parent container left after a
    // column move left the tree's focus pointing at a removed node.
    let layout = check_ops([
        Op::AddWindow {
            params: TestWindowParams::new(2),
        },
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams::new(1),
        },
        Op::SetFocusedDisplay(ColumnDisplay::Normal),
        Op::MoveLeft,
        Op::FocusDownOrLeft,
        Op::FocusParent,
        Op::MoveWindowInDirection(tiling_tree::Direction::Left),
    ]);
    assert!(matches!(
        layout.focus().map(|window| *window.id()),
        Some(1 | 2)
    ));
    assert_eq!(layout.windows().count(), 2);
}

#[test]
fn singleton_move_after_floating_close_keeps_the_parent_live() {
    // CI 36310511080 shrank `random_operations_dont_panic` to this sequence
    // (proptest cc 16b75ae3). A directional move of the only window read a
    // parent node that an earlier layout change had already removed. The
    // seed is in proptest-regressions/layout/tests.txt, recovered from the
    // run log after the artifact expired.
    let mut floating = TestWindowParams::new(5);
    floating.is_floating = true;
    let mut options = Options::default();
    options.layout.default_orientation = swayward_config::DefaultOrientation::Vertical;
    let layout = check_ops_with_options(
        options,
        [
            Op::AddWindow {
                params: TestWindowParams::new(3),
            },
            Op::AddOutput(1),
            Op::CenterWindow { id: None },
            Op::AddWindow { params: floating },
            Op::MoveFocusedToWorkspaceUp(false),
            Op::ToggleWindowFloating { id: None },
            Op::SwapWindowHorizontal(false),
            Op::FocusParent,
            Op::CloseWindow(5),
            Op::SplitFocused(tiling_tree::Layout::SplitH),
            Op::MoveWindowDown,
        ],
    );
    assert!(!layout.has_window(&5));
    assert_eq!(layout.focus().map(|window| *window.id()), Some(3));
}

#[test]
fn adding_next_to_a_window_while_the_workspace_is_focused_does_not_panic() {
    // Seed a0956be7 (proptest-regressions/layout/tests.txt): after `focus parent` on a lone
    // floating window the workspace is focused and no window is active, which the NextTo
    // placement unwrapped.
    check_ops([
        Op::AddOutput(1),
        Op::AddWindow {
            params: TestWindowParams {
                is_floating: true,
                ..TestWindowParams::new(2)
            },
        },
        Op::ToggleWindowFloating { id: None },
        Op::ToggleWindowFloating { id: None },
        Op::FocusParent,
        Op::AddWindowNextTo {
            params: TestWindowParams::new(3),
            next_to_id: 2,
        },
    ]);
}

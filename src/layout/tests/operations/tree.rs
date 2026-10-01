//! sway container-tree commands: split, layout, focus parent/child, nest, swap.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::FocusFirstRootChild => layout.focus_first_root_child(),
        Op::FocusLastRootChild => layout.focus_last_root_child(),
        Op::FocusRightOrFirstRootChild => layout.focus_right_or_first_root_child(),
        Op::FocusLeftOrLastRootChild => layout.focus_left_or_last_root_child(),
        Op::FocusRootChild(index) => layout.focus_root_child(index),
        Op::MoveFocusedRootChildToFirst => layout.move_focused_root_child_to_first(),
        Op::MoveFocusedRootChildToLast => layout.move_focused_root_child_to_last(),
        Op::MoveFocusedRootChildToIndex(index) => layout.move_focused_root_child_to_index(index),
        Op::SplitFocused(tree_layout) => layout.split_focused(tree_layout),
        Op::SetFocusedLayout(tree_layout) => {
            layout.set_focused_layout(tree_layout);
        }
        Op::FocusParent => {
            if layout.focus().is_some() {
                layout.focus_parent();
            }
        }
        Op::FocusChild => {
            if layout.focus().is_some() {
                layout.focus_child();
            }
        }
        Op::NestOrUnnestWindowLeft { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.nest_or_unnest_window_left(id.as_ref());
        }
        Op::NestOrUnnestWindowRight { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.nest_or_unnest_window_right(id.as_ref());
        }
        Op::NestFocusedWindow => layout.nest_focused_window(),
        Op::UnnestFocusedWindow => layout.unnest_focused_window(),
        Op::SwapWindowHorizontal(right) => layout.swap_window_horizontal(right),
        Op::ToggleFocusedTabbedDisplay => layout.toggle_focused_tabbed_display(),
        Op::SetFocusedDisplay(display) => layout.set_focused_display(display),
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}

//! Floating windows and containers, and the scratchpad.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::CenterWindow { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.center_window(id.as_ref());
        }
        Op::ToggleWindowFloating { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.toggle_window_floating(id.as_ref());
        }
        Op::SetWindowFloating { id, floating } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.set_window_floating(id.as_ref(), floating);
        }
        Op::ToggleFocusedContainerFloating => {
            let target = layout.active_workspace().and_then(|workspace| {
                workspace
                    .focused_container_node()
                    .map(|node| (workspace.id(), node, workspace.tiling().contains(node)))
            });
            if let Some((workspace, node, floating)) = target {
                layout.set_container_floating(workspace, node, floating);
            }
        }
        Op::MoveFocusedToScratchpad => layout.move_to_scratchpad(None),
        Op::FocusFloating => {
            layout.focus_floating();
        }
        Op::FocusTiling => {
            layout.focus_tiling();
        }
        Op::SwitchFocusFloatingTiling => {
            layout.switch_focus_floating_tiling();
        }
        Op::MoveFloatingWindow { id, x, y, animate } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.move_floating_window(id.as_ref(), x, y, animate);
        }
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}

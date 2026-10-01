//! Directional and window focus, and directional window moves.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::FocusLeft => {
            layout.focus_left();
        }
        Op::FocusRight => {
            layout.focus_right();
        }
        Op::FocusWindowOrMonitorUp(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.focus_window_up_or_output(&output);
        }
        Op::FocusWindowOrMonitorDown(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.focus_window_down_or_output(&output);
        }
        Op::FocusLeftOrMonitorLeft(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.focus_left_or_output(&output);
        }
        Op::FocusRightOrMonitorRight(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.focus_right_or_output(&output);
        }
        Op::FocusWindowDown => {
            layout.focus_down();
        }
        Op::FocusWindowUp => {
            layout.focus_up();
        }
        Op::FocusDownOrLeft => layout.focus_down_or_left(),
        Op::FocusDownOrRight => layout.focus_down_or_right(),
        Op::FocusUpOrLeft => layout.focus_up_or_left(),
        Op::FocusUpOrRight => layout.focus_up_or_right(),
        Op::FocusWindow(id) => layout.activate_window(&id),
        Op::FocusWindowInParent(index) => layout.focus_window_in_parent(index),
        Op::FocusWindowTop => layout.focus_window_top(),
        Op::FocusWindowBottom => layout.focus_window_bottom(),
        Op::FocusWindowDownOrTop => layout.focus_window_down_or_top(),
        Op::FocusWindowUpOrBottom => layout.focus_window_up_or_bottom(),
        Op::MoveLeft => {
            layout.move_left();
        }
        Op::MoveRight => {
            layout.move_right();
        }
        Op::MoveLeftOrToMonitorLeft(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.move_left_or_to_output(&output);
        }
        Op::MoveRightOrToMonitorRight(id) => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.move_right_or_to_output(&output);
        }
        Op::MoveWindowDown => {
            layout.move_down();
        }
        Op::MoveWindowUp => {
            layout.move_up();
        }
        Op::MoveWindowInDirection(direction) => {
            if let Some(id) = layout.focus().map(|window| window.id().to_owned()) {
                layout.move_window_in_direction(&id, direction, 10.);
            }
        }
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}

//! Pointer-driven operations: interactive move and resize, drag and drop, gestures, overview.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::ViewOffsetGestureBegin {
            output_idx: id,
            workspace_idx,
            is_touchpad: normalize,
        } => {
            let name = format!("output{id}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };

            layout.view_offset_gesture_begin(&output, workspace_idx, normalize);
        }
        Op::ViewOffsetGestureUpdate {
            delta,
            timestamp,
            is_touchpad,
        } => {
            layout.view_offset_gesture_update(delta, timestamp, is_touchpad);
        }
        Op::ViewOffsetGestureEnd { is_touchpad } => {
            layout.view_offset_gesture_end(is_touchpad);
        }
        Op::OverviewGestureBegin => {
            layout.overview_gesture_begin();
        }
        Op::OverviewGestureUpdate { delta, timestamp } => {
            layout.overview_gesture_update(delta, timestamp);
        }
        Op::OverviewGestureEnd => {
            layout.overview_gesture_end();
        }
        Op::InteractiveMoveBegin {
            window,
            output_idx,
            px,
            py,
        } => {
            let name = format!("output{output_idx}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };
            layout.interactive_move_begin(window, &output, Point::from((px, py)));
        }
        Op::InteractiveMoveUpdate {
            window,
            dx,
            dy,
            output_idx,
            px,
            py,
        } => {
            let name = format!("output{output_idx}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };
            layout.interactive_move_update(
                &window,
                Point::from((dx, dy)),
                output,
                Point::from((px, py)),
            );
        }
        Op::InteractiveMoveEnd { window } => {
            layout.interactive_move_end(&window);
        }
        Op::DndUpdate { output_idx, px, py } => {
            let name = format!("output{output_idx}");
            let Some(output) = layout.outputs().find(|o| o.name() == name).cloned() else {
                return Applied::Done;
            };
            layout.dnd_update(output, Point::from((px, py)));
        }
        Op::DndEnd => {
            layout.dnd_end();
        }
        Op::InteractiveResizeBegin { window, edges } => {
            layout.interactive_resize_begin(window, edges);
        }
        Op::InteractiveResizeUpdate { window, dx, dy } => {
            layout.interactive_resize_update(&window, Point::from((dx, dy)));
        }
        Op::InteractiveResizeEnd { window } => {
            layout.interactive_resize_end(&window);
        }
        Op::ToggleOverview => {
            layout.toggle_overview();
        }
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}

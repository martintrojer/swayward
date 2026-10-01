//! Width and height commands: presets, fixed sizes, maximize.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::SwitchPresetTiledWidth => layout.toggle_width(true),
        Op::SwitchPresetTiledWidthBack => layout.toggle_width(false),
        Op::SwitchPresetWindowWidth { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.toggle_window_width(id.as_ref(), true);
        }
        Op::SwitchPresetWindowWidthBack { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.toggle_window_width(id.as_ref(), false);
        }
        Op::SwitchPresetWindowHeight { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.toggle_window_height(id.as_ref(), true);
        }
        Op::SwitchPresetWindowHeightBack { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.toggle_window_height(id.as_ref(), false);
        }
        Op::MaximizeFocusedTiling => layout.toggle_full_width(),
        Op::MaximizeWindowToEdges { id } => {
            let id = id.or_else(|| layout.focus().map(|win| *win.id()));
            let Some(id) = id else {
                return Applied::Done;
            };
            if !layout.has_window(&id) {
                return Applied::Done;
            }
            layout.toggle_maximized(&id);
        }
        Op::SetFocusedWidth(change) => layout.set_focused_width(change),
        Op::SetWindowWidth { id, change } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.set_window_width(id.as_ref(), change);
        }
        Op::SetWindowHeight { id, change } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.set_window_height(id.as_ref(), change);
        }
        Op::ResetWindowHeight { id } => {
            let id = id.filter(|id| layout.has_window(id));
            layout.reset_window_height(id.as_ref());
        }
        Op::ExpandFocusedToAvailableWidth => layout.expand_focused_to_available_width(),
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}

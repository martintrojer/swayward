//! Window lifecycle and client-side state: map, close, fullscreen, parents, sizes, commits.

use super::*;

pub(super) fn apply(op: Op, layout: &mut Layout<TestWindow>) -> Applied {
    match op {
        Op::AddWindow { mut params } => {
            if layout.has_window(&params.id) {
                return Applied::Done;
            }
            if let Some(parent_id) = params.parent_id {
                if parent_id_causes_loop(layout, params.id, parent_id) {
                    params.parent_id = None;
                }
            }

            let is_floating = params.is_floating;
            let win = TestWindow::new(params);
            layout.add_window(
                win,
                AddWindowTarget::Auto,
                None,
                is_floating,
                ActivateWindow::default(),
            );
        }
        Op::AddWindowNextTo {
            mut params,
            next_to_id,
        } => {
            let mut found_next_to = false;

            if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                let win_id = move_.tile.window().0.id;
                if win_id == params.id {
                    return Applied::Done;
                }
                if win_id == next_to_id {
                    found_next_to = true;
                }
            }

            match &mut layout.monitor_set {
                MonitorSet::Normal { monitors, .. } => {
                    for mon in monitors {
                        for ws in &mut mon.workspaces {
                            for win in ws.windows() {
                                if win.0.id == params.id {
                                    return Applied::Done;
                                }

                                if win.0.id == next_to_id {
                                    found_next_to = true;
                                }
                            }
                        }
                    }
                }
                MonitorSet::NoOutputs { workspaces, .. } => {
                    for ws in workspaces {
                        for win in ws.windows() {
                            if win.0.id == params.id {
                                return Applied::Done;
                            }

                            if win.0.id == next_to_id {
                                found_next_to = true;
                            }
                        }
                    }
                }
            }

            if !found_next_to {
                return Applied::Done;
            }

            if let Some(parent_id) = params.parent_id {
                if parent_id_causes_loop(layout, params.id, parent_id) {
                    params.parent_id = None;
                }
            }

            let is_floating = params.is_floating;
            let win = TestWindow::new(params);
            layout.add_window(
                win,
                AddWindowTarget::NextTo(&next_to_id),
                None,
                is_floating,
                ActivateWindow::default(),
            );
        }
        Op::AddWindowToNamedWorkspace {
            mut params,
            ws_name,
        } => {
            let ws_name = format!("ws{ws_name}");
            let mut ws_id = None;

            if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                if move_.tile.window().0.id == params.id {
                    return Applied::Done;
                }
            }

            match &mut layout.monitor_set {
                MonitorSet::Normal { monitors, .. } => {
                    for mon in monitors {
                        for ws in &mut mon.workspaces {
                            for win in ws.windows() {
                                if win.0.id == params.id {
                                    return Applied::Done;
                                }
                            }

                            if ws
                                .name
                                .as_ref()
                                .is_some_and(|name| name.eq_ignore_ascii_case(&ws_name))
                            {
                                ws_id = Some(ws.id());
                            }
                        }
                    }
                }
                MonitorSet::NoOutputs { workspaces, .. } => {
                    for ws in workspaces {
                        for win in ws.windows() {
                            if win.0.id == params.id {
                                return Applied::Done;
                            }
                        }

                        if ws
                            .name
                            .as_ref()
                            .is_some_and(|name| name.eq_ignore_ascii_case(&ws_name))
                        {
                            ws_id = Some(ws.id());
                        }
                    }
                }
            }

            let Some(ws_id) = ws_id else {
                return Applied::Done;
            };

            if let Some(parent_id) = params.parent_id {
                if parent_id_causes_loop(layout, params.id, parent_id) {
                    params.parent_id = None;
                }
            }

            let is_floating = params.is_floating;
            let win = TestWindow::new(params);
            layout.add_window(
                win,
                AddWindowTarget::Workspace(ws_id),
                None,
                is_floating,
                ActivateWindow::default(),
            );
        }
        Op::CloseWindow(id) => {
            layout.remove_window(&id, Transaction::new());
        }
        Op::FullscreenWindow(id) => {
            if !layout.has_window(&id) {
                return Applied::Done;
            }
            layout.toggle_fullscreen(&id);
        }
        Op::SetFullscreenWindow {
            window,
            is_fullscreen,
        } => {
            if !layout.has_window(&window) {
                return Applied::Done;
            }
            layout.set_fullscreen(&window, is_fullscreen);
        }
        Op::ToggleWindowedFullscreen(id) => {
            if !layout.has_window(&id) {
                return Applied::Done;
            }
            layout.toggle_windowed_fullscreen(&id);
        }
        Op::SetParent {
            id,
            mut new_parent_id,
        } => {
            if !layout.has_window(&id) {
                return Applied::Done;
            }

            if let Some(parent_id) = new_parent_id {
                if parent_id_causes_loop(layout, id, parent_id) {
                    new_parent_id = None;
                }
            }

            let mut update = false;

            if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                if move_.tile.window().0.id == id {
                    move_.tile.window().0.parent_id.set(new_parent_id);
                    update = true;
                }
            }

            match &mut layout.monitor_set {
                MonitorSet::Normal { monitors, .. } => {
                    'outer: for mon in monitors {
                        for ws in &mut mon.workspaces {
                            for win in ws.windows() {
                                if win.0.id == id {
                                    win.0.parent_id.set(new_parent_id);
                                    update = true;
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
                MonitorSet::NoOutputs { workspaces, .. } => {
                    'outer: for ws in workspaces {
                        for win in ws.windows() {
                            if win.0.id == id {
                                win.0.parent_id.set(new_parent_id);
                                update = true;
                                break 'outer;
                            }
                        }
                    }
                }
            }

            if update {
                if let Some(new_parent_id) = new_parent_id {
                    layout.descendants_added(&new_parent_id);
                }
            }
        }
        Op::SetForcedSize { id, size } => {
            for (_mon, win) in layout.windows() {
                if win.0.id == id {
                    win.0.forced_size.set(size);
                    return Applied::Done;
                }
            }
        }
        Op::Communicate(id) => {
            let mut update = false;

            if let Some(InteractiveMoveState::Moving(move_)) = &layout.interactive_move {
                if move_.tile.window().0.id == id {
                    if move_.tile.window().communicate() {
                        update = true;
                    }

                    if update {
                        // The model has no Wayland serial; `None` is the explicit synthetic
                        // update used by this randomized layout test.
                        layout.update_window(&id, None);
                    }
                    return Applied::Done;
                }
            }

            match &mut layout.monitor_set {
                MonitorSet::Normal { monitors, .. } => {
                    'outer: for mon in monitors {
                        for ws in &mut mon.workspaces {
                            for win in ws.windows() {
                                if win.0.id == id {
                                    if win.communicate() {
                                        update = true;
                                    }
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
                MonitorSet::NoOutputs { workspaces, .. } => {
                    'outer: for ws in workspaces {
                        for win in ws.windows() {
                            if win.0.id == id {
                                if win.communicate() {
                                    update = true;
                                }
                                break 'outer;
                            }
                        }
                    }
                }
            }

            if update {
                // The model has no Wayland serial; `None` is the explicit synthetic update
                // used by this randomized layout test.
                layout.update_window(&id, None);
            }
        }
        other => return Applied::NotMine(Box::new(other)),
    }
    Applied::Done
}

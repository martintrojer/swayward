//! Helpers for keeping track of the event stream state.
//!
//! 1. Create an [`EventStreamState`] using `Default::default()`, or any individual state part if
//!    you only care about part of the state.
//! 2. Connect to swayward's legacy `$SWAYWARD_SOCKET` endpoint and request an event stream.
//! 3. Pass every [`Event`] to [`EventStreamStatePart::apply`] on your state.
//! 4. Read the fields of the state as needed.

use std::collections::hash_map::Entry;
use std::collections::HashMap;

use crate::legacy::{Event, Workspace};
use crate::{KeyboardLayouts, Window};

/// Part of the state communicated via the event stream.
pub trait EventStreamStatePart {
    /// Returns a sequence of events that replicates this state from default initialization.
    fn replicate(&self) -> Vec<Event>;

    /// Applies the event to this state.
    ///
    /// Returns `None` after applying the event, and `Some(event)` if the event is ignored by this
    /// part of the state. Updates and removals for unknown object ids are consumed as no-ops, since
    /// event delivery can start between an object's initial snapshot and its next update.
    fn apply(&mut self, event: Event) -> Option<Event>;
}

/// The full state communicated over the event stream.
///
/// Different parts of the state are not guaranteed to be consistent across every single event
/// sent by swayward. For example, you may receive the first [`Event::WindowOpenedOrChanged`] for a
/// just-opened window *after* an [`Event::WorkspaceActiveWindowChanged`] for that window. Between
/// these two events, the workspace active window id refers to a window that does not yet exist in
/// the windows state part.
#[derive(Debug, Default)]
pub struct EventStreamState {
    /// State of workspaces.
    pub workspaces: WorkspacesState,

    /// State of windows.
    pub windows: WindowsState,

    /// State of the keyboard layouts.
    pub keyboard_layouts: KeyboardLayoutsState,
}

/// The workspaces state communicated over the event stream.
#[derive(Debug, Default)]
pub struct WorkspacesState {
    /// Map from a workspace id to the workspace.
    pub workspaces: HashMap<u64, Workspace>,
}

/// The windows state communicated over the event stream.
#[derive(Debug, Default)]
pub struct WindowsState {
    /// Map from a window id to the window.
    pub windows: HashMap<u64, Window>,
}

/// The keyboard layout state communicated over the event stream.
#[derive(Debug, Default)]
pub struct KeyboardLayoutsState {
    /// Configured keyboard layouts.
    pub keyboard_layouts: Option<KeyboardLayouts>,
}

impl EventStreamStatePart for EventStreamState {
    fn replicate(&self) -> Vec<Event> {
        let mut events = Vec::new();
        events.extend(self.workspaces.replicate());
        events.extend(self.windows.replicate());
        events.extend(self.keyboard_layouts.replicate());
        events
    }

    fn apply(&mut self, event: Event) -> Option<Event> {
        let event = self.workspaces.apply(event)?;
        let event = self.windows.apply(event)?;
        let event = self.keyboard_layouts.apply(event)?;
        Some(event)
    }
}

impl EventStreamStatePart for WorkspacesState {
    fn replicate(&self) -> Vec<Event> {
        let workspaces = self.workspaces.values().cloned().collect();
        vec![Event::WorkspacesChanged { workspaces }]
    }

    fn apply(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::WorkspacesChanged { workspaces } => {
                self.workspaces = workspaces.into_iter().map(|ws| (ws.id, ws)).collect();
            }
            Event::WorkspaceEmptied { current } => {
                self.workspaces.retain(|_, workspace| {
                    workspace.name.as_deref() != current.name.as_deref()
                        || !matches!(
                            &current.properties,
                            crate::NodeProperties::Workspace(properties)
                                if workspace.output.as_ref() == Some(&properties.output)
                        )
                });
            }
            Event::WorkspaceInitialized { .. }
            | Event::WorkspaceRenamed { .. }
            | Event::WorkspaceMoved { .. }
            | Event::WorkspaceFocusChanged { .. }
            | Event::OutputChanged
            | Event::Shutdown { .. }
            | Event::Tick { .. } => {}
            Event::WorkspaceUrgencyChanged { id, current } => {
                if let Some(ws) = self.workspaces.get_mut(&id) {
                    ws.is_urgent = current.urgent;
                }
            }
            Event::WorkspaceActivated { id, focused } => {
                if let Some(output) = self.workspaces.get(&id).map(|ws| ws.output.clone()) {
                    for ws in self.workspaces.values_mut() {
                        let got_activated = ws.id == id;
                        if ws.output == output {
                            ws.is_active = got_activated;
                        }

                        if focused {
                            ws.is_focused = got_activated;
                        }
                    }
                }
            }
            Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => {
                if let Some(ws) = self.workspaces.get_mut(&workspace_id) {
                    ws.active_window_id = active_window_id;
                }
            }
            event => return Some(event),
        }
        None
    }
}

impl WindowsState {
    fn replace(&mut self, windows: Vec<Window>) {
        self.windows = windows.into_iter().map(|win| (win.id, win)).collect();
    }

    fn upsert(&mut self, window: Window) {
        let (id, is_focused) = match self.windows.entry(window.id) {
            Entry::Occupied(mut entry) => {
                let entry = entry.get_mut();
                *entry = window;
                (entry.id, entry.is_focused)
            }
            Entry::Vacant(entry) => {
                let entry = entry.insert(window);
                (entry.id, entry.is_focused)
            }
        };

        if is_focused {
            for win in self.windows.values_mut() {
                if win.id != id {
                    win.is_focused = false;
                }
            }
        }
    }

    fn focus(&mut self, id: Option<u64>) {
        for win in self.windows.values_mut() {
            win.is_focused = Some(win.id) == id;
        }
    }

    fn set_focus_timestamp(&mut self, id: u64, focus_timestamp: Option<crate::Timestamp>) {
        if let Some(win) = self.windows.get_mut(&id) {
            win.focus_timestamp = focus_timestamp;
        }
    }

    fn set_urgency(&mut self, id: u64, urgent: bool) {
        if let Some(win) = self.windows.get_mut(&id) {
            win.is_urgent = urgent;
        }
    }

    fn update_layouts(&mut self, changes: Vec<(u64, crate::WindowLayout)>) {
        for (id, update) in changes {
            if let Some(win) = self.windows.get_mut(&id) {
                win.layout = update;
            }
        }
    }
}

impl EventStreamStatePart for WindowsState {
    fn replicate(&self) -> Vec<Event> {
        let windows = self.windows.values().cloned().collect();
        vec![Event::WindowsChanged { windows }]
    }

    fn apply(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::WindowsChanged { windows } => self.replace(windows),
            Event::SwayWindowChanged { .. } | Event::WindowMoved { .. } => {}
            Event::WindowOpenedOrChanged { window } => self.upsert(window),
            Event::WindowClosed { id } => {
                self.windows.remove(&id);
            }
            Event::WindowFocusChanged { id } => self.focus(id),
            Event::WindowFocusTimestampChanged {
                id,
                focus_timestamp,
            } => self.set_focus_timestamp(id, focus_timestamp),
            Event::WindowUrgencyChanged { id, urgent } => self.set_urgency(id, urgent),
            Event::WindowLayoutsChanged { changes } => self.update_layouts(changes),
            event => return Some(event),
        }
        None
    }
}

impl EventStreamStatePart for KeyboardLayoutsState {
    fn replicate(&self) -> Vec<Event> {
        if let Some(keyboard_layouts) = self.keyboard_layouts.clone() {
            vec![Event::KeyboardLayoutsChanged { keyboard_layouts }]
        } else {
            vec![]
        }
    }

    fn apply(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::KeyboardLayoutsChanged { keyboard_layouts } => {
                self.keyboard_layouts = Some(keyboard_layouts);
            }
            Event::KeyboardLayoutSwitched { idx } => {
                let Some(kb) = self.keyboard_layouts.as_mut() else {
                    return Some(Event::KeyboardLayoutSwitched { idx });
                };
                kb.current_idx = idx;
            }
            event => return Some(event),
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_updates_before_the_snapshot_are_consumed_without_panicking() {
        let mut state = WorkspacesState::default();

        assert!(state
            .apply(Event::WorkspaceActivated {
                id: 42,
                focused: true,
            })
            .is_none());
        assert!(state
            .apply(Event::WorkspaceActiveWindowChanged {
                workspace_id: 42,
                active_window_id: Some(7),
            })
            .is_none());
        assert!(state.workspaces.is_empty());
    }

    #[test]
    fn unknown_window_updates_are_idempotent() {
        let mut state = WindowsState::default();

        assert!(state.apply(Event::WindowClosed { id: 42 }).is_none());
        assert!(state.apply(Event::WindowClosed { id: 42 }).is_none());
        assert!(state
            .apply(Event::WindowLayoutsChanged {
                changes: vec![(
                    42,
                    crate::WindowLayout {
                        pos_in_scrolling_layout: None,
                        tile_size: (100., 100.),
                        window_size: (100, 100),
                        tile_pos_in_workspace_view: None,
                        window_offset_in_tile: (0., 0.),
                    },
                )],
            })
            .is_none());
        assert!(state.windows.is_empty());
    }

    #[test]
    fn a_layout_switch_before_the_snapshot_is_left_unconsumed() {
        let mut state = KeyboardLayoutsState::default();

        assert!(matches!(
            state.apply(Event::KeyboardLayoutSwitched { idx: 1 }),
            Some(Event::KeyboardLayoutSwitched { idx: 1 })
        ));
        assert!(state.keyboard_layouts.is_none());
    }

    /// The active layout index used to be narrowed to `u8` on the way out of
    /// the compositor, so a keymap with more than 256 layouts reported the
    /// wrong one. Sway serializes `xkb_active_layout_index` as a plain integer
    /// (`sway/sway/ipc-json.c:1196-1197`), so the index must survive intact.
    #[test]
    fn a_layout_index_above_the_byte_range_is_not_truncated() {
        let mut state = KeyboardLayoutsState::default();
        state.apply(Event::KeyboardLayoutsChanged {
            keyboard_layouts: KeyboardLayouts {
                names: (0..300).map(|i| format!("layout{i}")).collect(),
                current_idx: 0,
            },
        });

        state.apply(Event::KeyboardLayoutSwitched { idx: 300 });

        assert_eq!(
            state
                .keyboard_layouts
                .as_ref()
                .expect("layouts were set")
                .current_idx,
            300,
            "the switched-to index must not wrap into the byte range"
        );
    }
}

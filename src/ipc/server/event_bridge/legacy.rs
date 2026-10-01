use super::*;

impl State {
    pub(crate) fn ipc_input_changed(
        &mut self,
        change: &'static str,
        device: crate::input::IpcInputDevice,
    ) {
        let Some(server) = &self.swayward.ipc_server else {
            return;
        };
        let input = describe_input(&self.swayward, &device);
        server.send_event(Event::SwayInputChanged {
            change: change.into(),
            input,
        });
    }

    fn ipc_keyboard_input_changed(&mut self, change: &'static str) {
        let devices = self
            .swayward
            .ipc_input_devices
            .values()
            .filter(|device| device.device_type == "keyboard")
            .cloned()
            .collect::<Vec<_>>();
        for device in devices {
            self.ipc_input_changed(change, device);
        }
    }

    pub fn ipc_keyboard_layouts_changed(&mut self) {
        let Some(keyboard_layouts) = keyboard_layouts(self) else {
            if let Some(server) = &self.swayward.ipc_server {
                server
                    .event_stream_state
                    .borrow_mut()
                    .keyboard_layouts
                    .keyboard_layouts = None;
            }
            return;
        };

        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        {
            let mut event_state = server.event_stream_state.borrow_mut();
            let state = &mut event_state.keyboard_layouts;
            let event = Event::KeyboardLayoutsChanged { keyboard_layouts };
            state.apply(event.clone());
            server.send_event(event);
        }
        self.ipc_keyboard_input_changed("xkb_keymap");
    }

    pub fn ipc_refresh_keyboard_layout_index(&mut self) {
        let Some(keyboard_layouts) = keyboard_layouts(self) else {
            return;
        };
        let idx = keyboard_layouts.current_idx;

        let Some(server) = &self.swayward.ipc_server else {
            return;
        };

        {
            let mut event_state = server.event_stream_state.borrow_mut();
            let state = &mut event_state.keyboard_layouts;
            if state
                .keyboard_layouts
                .as_ref()
                .is_none_or(|layouts| layouts.current_idx == idx)
            {
                return;
            }
            let event = Event::KeyboardLayoutSwitched { idx };
            state.apply(event.clone());
            server.send_event(event);
        }
        self.ipc_keyboard_input_changed("xkb_layout");
    }

    pub fn ipc_refresh_layout(&mut self) {
        if self
            .swayward
            .ipc_server
            .as_ref()
            .is_none_or(|server| !server.has_event_streams())
        {
            return;
        }
        self.ipc_initialize_event_state();
    }

    pub(crate) fn ipc_initialize_event_state(&mut self) {
        let previous_tree = self.swayward.ipc_server.as_ref().and_then(|server| {
            serde_json::from_str(&server.query_state.borrow().event_baseline_tree).ok()
        });
        self.ipc_refresh_workspaces();
        if let Some(server) = &self.swayward.ipc_server {
            let tree = describe_tree_with_power(
                &self.swayward.layout,
                &self.swayward.global_space,
                &self.swayward.marks_by_window,
                &self.swayward.marks_by_container,
                &self.swayward.output_power,
            );
            server.query_state.borrow_mut().event_baseline_tree = serde_json::to_string(&tree)
                .unwrap_or_else(|_| r#"{"success":false,"error":"serialization failed"}"#.into());
        }
        self.ipc_refresh_windows(previous_tree.as_ref());
    }
}

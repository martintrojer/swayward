impl Dispatch<ZwlrOutputManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrOutputManagerV1,
        event: zwlr_output_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Head { head } => {
                state.output_heads.push(OutputHead {
                    proxy: head,
                    name: None,
                    modes: Vec::new(),
                });
            }
            zwlr_output_manager_v1::Event::Done { serial } => {
                state.output_manager_serials.push(serial);
            }
            zwlr_output_manager_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(State, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for State {
    fn event(
        state: &mut Self,
        head: &ZwlrOutputHeadV1,
        event: zwlr_output_head_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let output = state
            .output_heads
            .iter_mut()
            .find(|output| output.proxy == *head)
            .unwrap();
        match event {
            zwlr_output_head_v1::Event::Name { name } => output.name = Some(name),
            zwlr_output_head_v1::Event::Mode { mode } => output.modes.push(mode),
            zwlr_output_head_v1::Event::Description { .. }
            | zwlr_output_head_v1::Event::PhysicalSize { .. }
            | zwlr_output_head_v1::Event::Enabled { .. }
            | zwlr_output_head_v1::Event::CurrentMode { .. }
            | zwlr_output_head_v1::Event::Position { .. }
            | zwlr_output_head_v1::Event::Transform { .. }
            | zwlr_output_head_v1::Event::Scale { .. }
            | zwlr_output_head_v1::Event::Make { .. }
            | zwlr_output_head_v1::Event::Model { .. }
            | zwlr_output_head_v1::Event::SerialNumber { .. }
            | zwlr_output_head_v1::Event::AdaptiveSync { .. }
            | zwlr_output_head_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(State, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrOutputModeV1,
        event: zwlr_output_mode_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_mode_v1::Event::Size { .. }
            | zwlr_output_mode_v1::Event::Refresh { .. }
            | zwlr_output_mode_v1::Event::Preferred
            | zwlr_output_mode_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrOutputConfigurationV1,
        event: zwlr_output_configuration_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let result = match event {
            zwlr_output_configuration_v1::Event::Succeeded => OutputConfigurationResult::Succeeded,
            zwlr_output_configuration_v1::Event::Failed => OutputConfigurationResult::Failed,
            zwlr_output_configuration_v1::Event::Cancelled => OutputConfigurationResult::Cancelled,
            _ => unreachable!(),
        };
        state.output_configuration_results.push(result);
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrOutputConfigurationHeadV1,
        _event: zwlr_output_configuration_head_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

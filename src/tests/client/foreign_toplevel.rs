impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.foreign_toplevels.push(ForeignToplevel {
                    handle: toplevel,
                    title: None,
                });
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(
        State,
        ZwlrForeignToplevelManagerV1,
        [zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ())]
    );
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let toplevel = state
            .foreign_toplevels
            .iter_mut()
            .find(|toplevel| toplevel.handle == *handle)
            .unwrap();
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                toplevel.title = Some(title);
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::Closed => (),
            zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::State { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::Done
            | zwlr_foreign_toplevel_handle_v1::Event::Parent { .. } => (),
            _ => unreachable!(),
        }
    }
}

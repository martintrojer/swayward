impl Dispatch<XdgToplevelTagManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgToplevelTagManagerV1,
        _event: <XdgToplevelTagManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &WlOutput,
        event: <WlOutput as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Geometry { .. } => (),
            wl_output::Event::Mode { .. } => (),
            wl_output::Event::Done => (),
            wl_output::Event::Scale { .. } => (),
            wl_output::Event::Name { name } => {
                *state.outputs.get_mut(output).unwrap() = name;
            }
            wl_output::Event::Description { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_seat::Event::Capabilities { capabilities }
                if capabilities
                    .into_result()
                    .is_ok_and(|caps| caps.contains(wl_seat::Capability::Keyboard))
                    && state.keyboard.is_none() =>
            {
                state.keyboard = Some(proxy.get_keyboard(_qhandle, ()))
            }
            wl_seat::Event::Capabilities { .. } => (),
            wl_seat::Event::Name { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Enter { serial, .. } = event {
            state.keyboard_enter_serial = Some(serial);
        }
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlCompositor,
        _event: <WlCompositor as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgActivationV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgActivationV1,
        _event: xdg_activation_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgActivationTokenV1, Arc<std::sync::Mutex<Option<String>>>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        token: &Arc<std::sync::Mutex<Option<String>>>,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_activation_token_v1::Event::Done { token: value } => {
                *token.lock().unwrap() = Some(value);
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpKeyboardShortcutsInhibitManagerV1,
        _event: zwp_keyboard_shortcuts_inhibit_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwpKeyboardShortcutsInhibitorV1,
        event: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => {
                state.shortcut_inhibitor_events.push(true)
            }
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => {
                state.shortcut_inhibitor_events.push(false)
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _state: &mut Self,
        xdg_wm_base: &XdgWmBase,
        event: <XdgWmBase as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_wm_base::Event::Ping { serial } => {
                xdg_wm_base.pong(serial);
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.workspace_groups.push(WorkspaceGroup {
                    handle: workspace_group,
                    outputs: Vec::new(),
                    workspaces: Vec::new(),
                    removed: false,
                });
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                state.ext_workspaces.push(ExtWorkspace {
                    handle: workspace,
                    id: None,
                    name: None,
                    removed: false,
                });
            }
            ext_workspace_manager_v1::Event::Done | ext_workspace_manager_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_group_handle_v1::Event::OutputEnter { output } => state
                .workspace_groups
                .iter_mut()
                .find(|candidate| candidate.handle == *group)
                .unwrap()
                .outputs
                .push(output),
            ext_workspace_group_handle_v1::Event::OutputLeave { output } => state
                .workspace_groups
                .iter_mut()
                .find(|candidate| candidate.handle == *group)
                .unwrap()
                .outputs
                .retain(|candidate| candidate != &output),
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                state
                    .workspace_membership_events
                    .push((workspace.clone(), group.clone(), true));
                state
                    .workspace_groups
                    .iter_mut()
                    .find(|candidate| candidate.handle == *group)
                    .unwrap()
                    .workspaces
                    .push(workspace);
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                state
                    .workspace_membership_events
                    .push((workspace.clone(), group.clone(), false));
                state
                    .workspace_groups
                    .iter_mut()
                    .find(|candidate| candidate.handle == *group)
                    .unwrap()
                    .workspaces
                    .retain(|candidate| candidate != &workspace);
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                state
                    .workspace_groups
                    .iter_mut()
                    .find(|candidate| candidate.handle == *group)
                    .unwrap()
                    .removed = true;
            }
            ext_workspace_group_handle_v1::Event::Capabilities { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        workspace: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let workspace = state
            .ext_workspaces
            .iter_mut()
            .find(|candidate| candidate.handle == *workspace)
            .unwrap();
        match event {
            ext_workspace_handle_v1::Event::Id { id } => workspace.id = Some(id),
            ext_workspace_handle_v1::Event::Name { name } => workspace.name = Some(name),
            ext_workspace_handle_v1::Event::Removed => workspace.removed = true,
            ext_workspace_handle_v1::Event::Coordinates { .. }
            | ext_workspace_handle_v1::Event::State { .. }
            | ext_workspace_handle_v1::Event::Capabilities { .. } => (),
            _ => unreachable!(),
        }
    }
}

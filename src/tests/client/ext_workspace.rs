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

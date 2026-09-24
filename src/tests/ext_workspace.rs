use smithay::reexports::wayland_server::Resource as _;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::Proxy as _;

use super::*;

fn map_window(f: &mut Fixture, client: client::ClientId, title: &str) -> WlSurface {
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.set_title(title);
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(200, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    surface
}

fn workspace(f: &mut Fixture, client: client::ClientId, name: &str) -> super::client::ExtWorkspace {
    let workspace = f
        .client(client)
        .state
        .ext_workspaces
        .iter()
        .find(|workspace| !workspace.removed && workspace.name.as_deref() == Some(name))
        .unwrap();
    super::client::ExtWorkspace {
        handle: workspace.handle.clone(),
        id: workspace.id.clone(),
        name: workspace.name.clone(),
        removed: workspace.removed,
    }
}

fn group_for_output(
    f: &mut Fixture,
    client: client::ClientId,
    output: &str,
) -> smithay::reexports::wayland_protocols::ext::workspace::v1::client::ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1{
    let state = &f.client(client).state;
    state
        .workspace_groups
        .iter()
        .find(|group| {
            !group.removed
                && group.outputs.iter().any(|candidate| {
                    state.outputs.get(candidate).map(String::as_str) == Some(output)
                })
        })
        .unwrap()
        .handle
        .clone()
}

#[test]
fn mapping_a_window_does_not_create_an_extra_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    f.double_roundtrip(client);

    assert_eq!(
        f.client(client)
            .state
            .ext_workspaces
            .iter()
            .filter(|workspace| !workspace.removed)
            .count(),
        1
    );
    assert_eq!(
        f.client(client).state.ext_workspaces[0].name.as_deref(),
        Some("1")
    );

    map_window(&mut f, client, "window");
    assert_eq!(
        f.client(client)
            .state
            .ext_workspaces
            .iter()
            .filter(|workspace| !workspace.removed)
            .count(),
        1
    );
}

#[test]
fn workspace_names_come_from_sway_identity() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    assert!(crate::command::execute(f.niri_state(), "workspace 7")[0].success);
    f.double_roundtrip(client);

    assert_eq!(workspace(&mut f, client, "7").name.as_deref(), Some("7"));
}

#[test]
fn activate_uses_the_existing_cross_output_workspace_focus_path() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    f.niri_focus_output(1);
    assert!(crate::command::execute(f.niri_state(), "workspace source")[0].success);
    map_window(&mut f, client, "first");
    f.niri_focus_output(2);
    assert!(crate::command::execute(f.niri_state(), "workspace destination")[0].success);
    map_window(&mut f, client, "second");
    f.double_roundtrip(client);

    let source = workspace(&mut f, client, "source").handle;
    let manager = f
        .client(client)
        .state
        .ext_workspace_manager
        .clone()
        .unwrap();
    source.activate();
    manager.commit();
    f.double_roundtrip(client);

    let output = f.niri_output(1);
    assert_eq!(f.swayward().layout.active_output(), Some(&output));
    assert_eq!(
        f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_display_name(0),
        "source"
    );
}

#[test]
fn activate_workspace_with_a_hidden_scratchpad_window_keeps_it_hidden() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let window = f
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| {
            mapped.toplevel().wl_surface().id().protocol_id() == surface.id().protocol_id()
        })
        .unwrap()
        .1
        .window
        .clone();
    f.swayward().layout.move_to_scratchpad(Some(&window));
    f.double_roundtrip(client);

    let workspace = workspace(&mut f, client, "1").handle;
    let manager = f
        .client(client)
        .state
        .ext_workspace_manager
        .clone()
        .unwrap();
    workspace.activate();
    manager.commit();
    f.double_roundtrip(client);

    assert!(f.swayward().layout.is_scratchpad_hidden(&window));
}

#[test]
fn assignment_leaves_the_old_group_before_entering_the_new_group() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    f.niri_focus_output(1);
    assert!(crate::command::execute(f.niri_state(), "workspace source")[0].success);
    map_window(&mut f, client, "window");
    f.double_roundtrip(client);

    let workspace = workspace(&mut f, client, "source").handle;
    let target = group_for_output(&mut f, client, "headless-2");
    f.client(client).state.workspace_membership_events.clear();
    let manager = f
        .client(client)
        .state
        .ext_workspace_manager
        .clone()
        .unwrap();
    workspace.assign(&target);
    manager.commit();
    f.double_roundtrip(client);

    let output = f
        .swayward()
        .layout
        .workspaces()
        .find(|(_, _, candidate)| candidate.sway_display_name(0) == "source")
        .unwrap()
        .0
        .unwrap()
        .output()
        .clone();
    assert_eq!(output, f.niri_output(2));
    let events = f
        .client(client)
        .state
        .workspace_membership_events
        .iter()
        .filter(|(candidate, _, _)| candidate == &workspace)
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert!(!events[0].2);
    assert!(events[1].2);
    assert_ne!(events[0].1, events[1].1);
    let groups = f
        .client(client)
        .state
        .workspace_groups
        .iter()
        .filter(|group| !group.removed && group.workspaces.contains(&workspace))
        .count();
    assert_eq!(groups, 1);
}

#[test]
fn assignment_to_a_removed_group_is_ignored() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    f.niri_focus_output(1);
    assert!(crate::command::execute(f.niri_state(), "workspace source")[0].success);
    map_window(&mut f, client, "window");
    f.double_roundtrip(client);

    let workspace = workspace(&mut f, client, "source").handle;
    let removed_group = group_for_output(&mut f, client, "headless-2");
    let removed = f.niri_output(2);
    f.swayward().remove_output(&removed);
    f.double_roundtrip(client);
    let removed = f
        .client(client)
        .state
        .workspace_groups
        .iter()
        .find(|group| group.handle == removed_group)
        .unwrap();
    assert!(removed.removed);
    assert!(removed.workspaces.is_empty());

    let manager = f
        .client(client)
        .state
        .ext_workspace_manager
        .clone()
        .unwrap();
    workspace.assign(&removed_group);
    manager.commit();
    f.double_roundtrip(client);
    let output = f.niri_output(1);
    assert_eq!(f.swayward().layout.active_output(), Some(&output));
}

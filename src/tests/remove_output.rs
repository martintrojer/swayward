use super::*;

fn map_window(f: &mut Fixture, client: client::ClientId) -> smithay::desktop::Window {
    let window = f.client(client).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.swayward().layout.focus().unwrap().window.clone()
}

#[test]
fn removing_output_discards_empty_workspace_instead_of_moving_it() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    f.niri_focus_output(2);
    assert!(crate::command::execute(f.niri_state(), "workspace empty")[0].success);
    f.swayward()
        .layout
        .active_workspace_mut()
        .unwrap()
        .set_persistent_name("empty".into());
    let removed_workspace = f.swayward().layout.active_workspace().unwrap().id();
    let removed = f.niri_output(2);

    f.swayward().remove_output(&removed);

    assert!(f
        .swayward()
        .layout
        .workspaces()
        .all(|(_, _, workspace)| workspace.id() != removed_workspace));
    assert_eq!(f.swayward().layout.workspaces().count(), 1);
}

#[test]
fn removing_output_evacuates_sticky_window_to_surviving_active_workspace() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let destination = f.niri_output(1);
    assert!(crate::command::execute(f.niri_state(), "workspace destination")[0].success);
    let destination_workspace = f.swayward().layout.active_workspace().unwrap().id();
    f.niri_focus_output(2);
    let client = f.add_client();
    let mapped_window = map_window(&mut f, client);
    assert!(crate::command::execute(f.niri_state(), "floating enable, sticky enable")[0].success);
    assert!(crate::command::execute(f.niri_state(), "workspace empty")[0].success);
    let removed = f.niri_output(2);

    f.swayward().remove_output(&removed);

    let mapped_output = f.swayward().layout.windows().find_map(|(monitor, window)| {
        (window.window == mapped_window).then(|| monitor.map(|monitor| monitor.output()))
    });
    assert_eq!(mapped_output.flatten(), Some(&destination));
    assert_eq!(f.swayward().layout.windows().count(), 1);
    let workspace = f
        .swayward()
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.has_window(&mapped_window))
        .unwrap()
        .2;
    assert_eq!(workspace.id(), destination_workspace);
    assert_eq!(workspace.sway_name().as_deref(), Some("destination"));
}

#[test]
fn set_fullscreen_on_removed_output_does_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));

    let id = f.add_client();

    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    // Grab the second output's wl_output proxy on the client side.
    let wl_output = f.client(id).output("headless-2");

    // Remove the output on the niri side. Its wl_output global is disabled but not yet
    // destroyed, so the client's wl_output resource is still valid and usable.
    let output = f.niri_output(2);
    f.swayward().remove_output(&output);

    // Request fullscreen on the now-removed wl_output. niri must not panic.
    let window = f.client(id).window(&surface);
    window.set_fullscreen(Some(&wl_output));
    f.double_roundtrip(id);
}

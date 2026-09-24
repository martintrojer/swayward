use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use smithay::reexports::wayland_server::Resource as _;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::Proxy as _;

use super::*;

fn window_id(f: &mut Fixture, surface: &WlSurface) -> smithay::desktop::Window {
    let id = surface.id().protocol_id();
    f.swayward()
        .layout
        .windows()
        .find(|(_, window)| window.toplevel().wl_surface().id().protocol_id() == id)
        .unwrap()
        .1
        .window
        .clone()
}

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

#[test]
fn requests_drive_the_mapped_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    let first = map_window(&mut f, client, "first");
    let second = map_window(&mut f, client, "second");
    let seat = f.client(client).state.seat.clone().unwrap();
    let first_window = window_id(&mut f, &first);

    f.client(client)
        .foreign_toplevel("first")
        .handle
        .activate(&seat);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().layout.focus().unwrap().window, first_window);

    f.client(client)
        .foreign_toplevel("first")
        .handle
        .set_maximized();
    f.double_roundtrip(client);
    assert!(f
        .client(client)
        .window(&first)
        .configures_received
        .last()
        .unwrap()
        .1
        .states
        .contains(&xdg_toplevel::State::Maximized));
    f.client(client)
        .foreign_toplevel("first")
        .handle
        .unset_maximized();
    f.double_roundtrip(client);
    assert!(!f
        .client(client)
        .window(&first)
        .configures_received
        .last()
        .unwrap()
        .1
        .states
        .contains(&xdg_toplevel::State::Maximized));

    f.client(client)
        .foreign_toplevel("first")
        .handle
        .set_minimized();
    f.double_roundtrip(client);
    assert!(f.swayward().layout.is_scratchpad_hidden(&first_window));
    f.client(client)
        .foreign_toplevel("first")
        .handle
        .unset_minimized();
    f.double_roundtrip(client);
    assert!(!f.swayward().layout.is_scratchpad_hidden(&first_window));
    assert_eq!(f.swayward().layout.focus().unwrap().window, first_window);

    f.client(client)
        .foreign_toplevel("first")
        .handle
        .set_fullscreen(None);
    f.double_roundtrip(client);
    assert!(f
        .client(client)
        .window(&first)
        .configures_received
        .last()
        .unwrap()
        .1
        .states
        .contains(&xdg_toplevel::State::Fullscreen));
    f.client(client)
        .foreign_toplevel("first")
        .handle
        .unset_fullscreen();
    f.double_roundtrip(client);
    assert!(!f
        .client(client)
        .window(&first)
        .configures_received
        .last()
        .unwrap()
        .1
        .states
        .contains(&xdg_toplevel::State::Fullscreen));

    let output = f.client(client).output("headless-2");
    f.client(client)
        .foreign_toplevel("first")
        .handle
        .set_fullscreen(Some(&output));
    f.double_roundtrip(client);
    let mapped_output = f
        .swayward()
        .layout
        .windows()
        .find_map(|(monitor, window)| {
            (window.window == first_window).then(|| monitor.unwrap().output().clone())
        })
        .unwrap();
    assert_eq!(mapped_output, f.niri_output(2));

    f.client(client).foreign_toplevel("second").handle.close();
    f.double_roundtrip(client);
    assert!(f.client(client).window(&second).close_requested);
}

#[test]
fn activate_restores_a_minimized_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let window = window_id(&mut f, &surface);
    let seat = f.client(client).state.seat.clone().unwrap();
    let handle = f.client(client).foreign_toplevel("window").handle.clone();

    handle.set_minimized();
    f.double_roundtrip(client);
    assert!(f.swayward().layout.is_scratchpad_hidden(&window));

    handle.activate(&seat);
    f.double_roundtrip(client);
    assert!(!f.swayward().layout.is_scratchpad_hidden(&window));
    assert_eq!(f.swayward().layout.focus().unwrap().window, window);
}

#[test]
fn closed_window_and_removed_output_requests_are_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let handle = f.client(client).foreign_toplevel("window").handle.clone();
    let seat = f.client(client).state.seat.clone().unwrap();
    let removed_output = f.client(client).output("headless-2");

    let removed = f.niri_output(2);
    f.swayward().remove_output(&removed);
    handle.set_fullscreen(Some(&removed_output));
    f.double_roundtrip(client);
    let only_output = f.niri_output(1);
    let window = window_id(&mut f, &surface);
    let mapped_output = f
        .swayward()
        .layout
        .windows()
        .find_map(|(monitor, mapped)| {
            (mapped.window == window).then(|| monitor.unwrap().output().clone())
        })
        .unwrap();
    assert_eq!(mapped_output, only_output);

    f.client(client).window(&surface).attach_null();
    f.client(client).window(&surface).commit();
    f.double_roundtrip(client);
    handle.activate(&seat);
    handle.close();
    handle.set_fullscreen(None);
    handle.unset_fullscreen();
    handle.set_maximized();
    handle.unset_maximized();
    handle.set_minimized();
    handle.unset_minimized();
    handle.set_rectangle(&surface, 0, 0, 1, 1);
    f.double_roundtrip(client);
}

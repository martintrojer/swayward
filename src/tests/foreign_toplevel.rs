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

fn last_states(
    f: &mut Fixture,
    client: client::ClientId,
    surface: &WlSurface,
) -> Vec<xdg_toplevel::State> {
    f.client(client)
        .window(surface)
        .configures_received
        .last()
        .unwrap()
        .1
        .states
        .clone()
}

fn mapped_output(f: &mut Fixture, window: &smithay::desktop::Window) -> smithay::output::Output {
    f.swayward()
        .layout
        .windows()
        .find_map(|(monitor, mapped)| {
            (&mapped.window == window).then(|| monitor.unwrap().output().clone())
        })
        .unwrap()
}

/// Sway focuses the view on `activate` (sway/sway/tree/view.c:733-750).
#[test]
fn activate_focuses_the_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let first = map_window(&mut f, client, "first");
    map_window(&mut f, client, "second");
    let seat = f.client(client).state.seat.clone().unwrap();
    let first_window = window_id(&mut f, &first);
    assert_ne!(f.swayward().layout.focus().unwrap().window, first_window);

    let handle = f.client(client).foreign_toplevel("first").handle.clone();
    handle.activate(&seat);
    f.double_roundtrip(client);
    assert_eq!(f.swayward().layout.focus().unwrap().window, first_window);
}

/// Sway toggles workspace fullscreen and, with an output argument, first
/// moves the view to that output's active workspace
/// (sway/sway/tree/view.c:753-791).
#[test]
fn fullscreen_and_fullscreen_on_output_follow_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.add_output(2, (1280, 720));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let window = window_id(&mut f, &surface);
    let handle = f.client(client).foreign_toplevel("window").handle.clone();

    handle.set_fullscreen(None);
    f.double_roundtrip(client);
    assert!(last_states(&mut f, client, &surface).contains(&xdg_toplevel::State::Fullscreen));
    handle.unset_fullscreen();
    f.double_roundtrip(client);
    assert!(!last_states(&mut f, client, &surface).contains(&xdg_toplevel::State::Fullscreen));

    let output = f.client(client).output("headless-2");
    handle.set_fullscreen(Some(&output));
    f.double_roundtrip(client);
    assert_eq!(mapped_output(&mut f, &window), f.niri_output(2));
    assert!(last_states(&mut f, client, &surface).contains(&xdg_toplevel::State::Fullscreen));
}

/// Sway closes the view on `close` (sway/sway/tree/view.c:794-799).
#[test]
fn close_sends_xdg_close() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let first = map_window(&mut f, client, "first");
    let second = map_window(&mut f, client, "second");

    f.client(client).foreign_toplevel("second").handle.close();
    f.double_roundtrip(client);
    assert!(f.client(client).window(&second).close_requested);
    assert!(!f.client(client).window(&first).close_requested);
}

/// Known divergence: sway wires only `activate`, `fullscreen` and `close` on
/// the wlr foreign-toplevel handle (sway/sway/tree/view.c:881-893), so
/// `set_maximized` is a no-op there. swayward inherits niri's maximize
/// mapping. This pins the current behaviour until that is decided; it is not
/// evidence of sway compatibility.
#[test]
fn maximize_request_maximizes_unlike_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let handle = f.client(client).foreign_toplevel("window").handle.clone();

    handle.set_maximized();
    f.double_roundtrip(client);
    assert!(last_states(&mut f, client, &surface).contains(&xdg_toplevel::State::Maximized));
    handle.unset_maximized();
    f.double_roundtrip(client);
    assert!(!last_states(&mut f, client, &surface).contains(&xdg_toplevel::State::Maximized));
}

/// Known divergence: sway has no foreign-toplevel minimize handler
/// (sway/sway/tree/view.c:881-893), so `set_minimized` is a no-op there.
/// swayward hides the window in the scratchpad. This pins the current
/// behaviour until that is decided; it is not evidence of sway compatibility.
#[test]
fn minimize_request_hides_in_scratchpad_unlike_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let window = window_id(&mut f, &surface);
    let handle = f.client(client).foreign_toplevel("window").handle.clone();

    handle.set_minimized();
    f.double_roundtrip(client);
    assert!(f.swayward().layout.is_scratchpad_hidden(&window));
    handle.unset_minimized();
    f.double_roundtrip(client);
    assert!(!f.swayward().layout.is_scratchpad_hidden(&window));
    assert_eq!(f.swayward().layout.focus().unwrap().window, window);
}

/// Sway's activate handler shows a scratchpad-hidden view before focusing it
/// (sway/sway/tree/view.c:741-743). The window is hidden with the
/// `scratchpad` layout operation rather than a minimize request, so this
/// holds whatever happens to the minimize mapping.
#[test]
fn activate_shows_a_scratchpad_hidden_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let window = window_id(&mut f, &surface);
    let seat = f.client(client).state.seat.clone().unwrap();
    let handle = f.client(client).foreign_toplevel("window").handle.clone();

    f.swayward().layout.move_to_scratchpad(Some(&window));
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
    assert_eq!(mapped_output(&mut f, &window), only_output);

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

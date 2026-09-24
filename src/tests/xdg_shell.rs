use smithay::backend::input::{ButtonState, InputTime};
use smithay::input::pointer::ButtonEvent;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::zxdg_toplevel_decoration_v1::Mode;
use smithay::reexports::wayland_server::Resource as _;
use smithay::utils::SERIAL_COUNTER;
use smithay::wayland::shell::xdg::XdgShellHandler as _;
use wayland_client::Proxy as _;

use super::client::ClientId;
use super::*;
use crate::input::move_grab::MoveGrab;
use crate::input::resize_grab::ResizeGrab;
use crate::layout::tiling_tree::IpcNode;
use crate::layout::LayoutElement as _;
use crate::swayward::State;

#[derive(Clone, Copy)]
enum WindowState {
    Tiled,
    Floating,
    Fullscreen,
}

fn set_up(state: WindowState) -> (Fixture, ClientId) {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.commit();
    fixture.roundtrip(client);

    let window = fixture.client(client).state.windows.last_mut().unwrap();
    window.attach_new_buffer();
    window.set_size(400, 300);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    if matches!(state, WindowState::Tiled) {
        let window = fixture.client(client).create_window();
        window.commit();
        fixture.roundtrip(client);
        let window = fixture.client(client).state.windows.last_mut().unwrap();
        window.attach_new_buffer();
        window.set_size(400, 300);
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    } else {
        fixture.swayward().layout.toggle_window_floating(None);
        fixture.double_roundtrip(client);
        fixture
            .client(client)
            .state
            .windows
            .last_mut()
            .unwrap()
            .ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
    if matches!(state, WindowState::Fullscreen) {
        let window = fixture
            .swayward()
            .layout
            .windows()
            .next()
            .unwrap()
            .1
            .window
            .clone();
        fixture.swayward().layout.set_fullscreen(&window, true);
    }

    fixture.niri_state().move_cursor((200., 150.).into());
    (fixture, client)
}

#[test]
fn initial_decoration_mode_honours_open_floating_rule() {
    let config = swayward_config::Config::parse_mem(
        r#"
window-rule {
    open-floating true
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    let client = fixture.add_client();
    fixture.client(client).create_window();
    fixture
        .client(client)
        .decorate_last_window(Mode::ClientSide);
    fixture
        .client(client)
        .state
        .windows
        .last()
        .unwrap()
        .commit();
    fixture.double_roundtrip(client);

    assert_eq!(
        fixture
            .client(client)
            .state
            .windows
            .last()
            .unwrap()
            .decoration_modes
            .last(),
        Some(&Mode::ClientSide),
        "a window predicted to float must receive its initial client-side mode"
    );
}

#[test]
fn decoration_mode_follows_sway_tiling_policy() {
    let mut fixture = Fixture::with_config(swayward_config::Config::parse_mem("").unwrap());
    fixture.add_output(1, (1280, 720));

    let client = fixture.add_client();
    assert!(fixture
        .client(client)
        .state
        .xdg_decoration_manager
        .is_some());
    let window = fixture.client(client).create_window();
    window.commit();
    fixture.roundtrip(client);
    fixture
        .client(client)
        .decorate_last_window(Mode::ClientSide);
    fixture.double_roundtrip(client);
    assert_eq!(
        fixture
            .client(client)
            .state
            .windows
            .last()
            .unwrap()
            .decoration_modes
            .last(),
        Some(&Mode::ServerSide),
        "tiled windows must use server-side decorations"
    );

    let window = fixture.client(client).state.windows.last_mut().unwrap();
    window.attach_new_buffer();
    window.set_size(400, 300);
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    let outcome = crate::command::execute(fixture.niri_state(), "border csd");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("This window doesn't support client side decorations")
    );

    fixture.swayward().layout.toggle_window_floating(None);
    fixture.swayward().layout.refresh(true);
    fixture
        .client(client)
        .request_decoration_mode(Mode::ClientSide);
    fixture.double_roundtrip(client);
    assert_eq!(
        fixture
            .client(client)
            .state
            .windows
            .last()
            .unwrap()
            .decoration_modes
            .last(),
        Some(&Mode::ClientSide),
        "floating windows must receive the mode they request"
    );
}

fn start_pointer_grab(
    fixture: &mut Fixture,
) -> (
    smithay::input::pointer::PointerHandle<State>,
    smithay::utils::Serial,
) {
    let serial = SERIAL_COUNTER.next_serial();
    let pointer = fixture.swayward().seat.get_pointer().unwrap();
    pointer.button(
        fixture.niri_state(),
        &ButtonEvent {
            button: 0x110,
            state: ButtonState::Pressed,
            serial,
            time: InputTime::from_millis(1),
        },
    );
    (pointer, serial)
}

fn toplevel_and_seat(
    fixture: &mut Fixture,
) -> (
    smithay::wayland::shell::xdg::ToplevelSurface,
    smithay::reexports::wayland_server::protocol::wl_seat::WlSeat,
) {
    let toplevel = fixture
        .swayward()
        .layout
        .windows()
        .next()
        .unwrap()
        .1
        .toplevel()
        .clone();
    let wayland_client = fixture
        .swayward()
        .display_handle
        .get_client(toplevel.wl_surface().id())
        .unwrap();
    let seat = fixture
        .swayward()
        .seat
        .client_seats(&wayland_client)
        .remove(0);
    (toplevel, seat)
}

fn release_pointer(fixture: &mut Fixture, pointer: smithay::input::pointer::PointerHandle<State>) {
    pointer.button(
        fixture.niri_state(),
        &ButtonEvent {
            button: 0x110,
            state: ButtonState::Released,
            serial: SERIAL_COUNTER.next_serial(),
            time: InputTime::from_millis(2),
        },
    );
}

fn request_move(fixture: &mut Fixture) -> bool {
    let (pointer, serial) = start_pointer_grab(fixture);
    let (toplevel, seat) = toplevel_and_seat(fixture);
    fixture.niri_state().move_request(toplevel, seat, serial);

    let is_move = pointer
        .with_grab(|_, grab| grab.is::<MoveGrab>())
        .unwrap_or(false);
    release_pointer(fixture, pointer);
    is_move
}

fn request_resize(fixture: &mut Fixture) -> bool {
    let (pointer, serial) = start_pointer_grab(fixture);
    let (toplevel, seat) = toplevel_and_seat(fixture);
    fixture.niri_state().resize_request(
        toplevel,
        seat,
        serial,
        smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge::BottomRight,
    );

    let is_resize = pointer
        .with_grab(|_, grab| grab.is::<ResizeGrab>())
        .unwrap_or(false);
    release_pointer(fixture, pointer);
    is_resize
}

fn map_window(
    f: &mut Fixture,
    client: ClientId,
    title: &str,
) -> wayland_client::protocol::wl_surface::WlSurface {
    let window = f.client(client).create_window();
    window.set_title(title);
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    surface
}

fn tree_shape(node: &IpcNode<smithay::desktop::Window>) -> String {
    match node {
        IpcNode::Split {
            layout, children, ..
        } => format!(
            "{layout:?}[{}]",
            children
                .iter()
                .map(tree_shape)
                .collect::<Vec<_>>()
                .join(",")
        ),
        IpcNode::Leaf { window, .. } => window
            .toplevel()
            .unwrap()
            .wl_surface()
            .id()
            .protocol_id()
            .to_string(),
    }
}

#[test]
fn remap_replaces_the_tree_entry_and_focuses_the_same_toplevel_once() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let first = map_window(&mut f, client, "first");
    let second = map_window(&mut f, client, "second");

    f.client(client).window(&second).attach_null();
    f.client(client).window(&second).commit();
    f.double_roundtrip(client);
    assert_eq!(f.swayward().layout.windows().count(), 1);
    assert_eq!(f.swayward().layout.focus().unwrap().title(), "first");

    f.client(client).window(&second).commit();
    f.double_roundtrip(client);
    let window = f.client(client).window(&second);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(f.swayward().layout.windows().count(), 2);
    assert!(f.swayward().unmapped_windows.is_empty());
    let remapped = f.swayward().layout.focus().unwrap().window.clone();
    assert_eq!(
        remapped.toplevel().unwrap().wl_surface().id().protocol_id(),
        second.id().protocol_id()
    );
    assert_eq!(
        f.swayward()
            .layout
            .windows()
            .filter(|(_, mapped)| mapped.window == remapped)
            .count(),
        1
    );

    // Keep both client handles live for the duration of the assertions.
    assert_ne!(first, second);
}

#[test]
fn fullscreen_round_trip_restores_the_original_nested_tree_position() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let _first = map_window(&mut f, client, "first");
    let second = map_window(&mut f, client, "second");
    let _third = map_window(&mut f, client, "third");
    f.swayward().layout.nest_or_unnest_window_left(None);
    let before = tree_shape(
        &f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree(),
    );

    f.client(client).window(&second).set_fullscreen(None);
    f.double_roundtrip(client);
    assert!(f
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| {
            mapped.toplevel().wl_surface().id().protocol_id() == second.id().protocol_id()
        })
        .unwrap()
        .1
        .pending_sizing_mode()
        .is_fullscreen());

    f.client(client).window(&second).unset_fullscreen();
    f.double_roundtrip(client);
    let mapped = f
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| {
            mapped.toplevel().wl_surface().id().protocol_id() == second.id().protocol_id()
        })
        .unwrap()
        .1;
    assert!(mapped.pending_sizing_mode().is_normal());
    let after = tree_shape(
        &f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .ipc_tiling_tree(),
    );

    assert_eq!(after, before);
}

#[test]
#[should_panic(expected = "Protocol error 4 on object xdg_wm_base")]
fn stale_configure_ack_is_rejected_by_the_xdg_protocol() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let surface = map_window(&mut f, client, "window");
    let stale_serial = f.client(client).window(&surface).configures_received[0].0;

    f.client(client).window(&surface).set_fullscreen(None);
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    window.ack_configure(stale_serial);
    window.commit();
    f.double_roundtrip(client);
}

#[test]
fn popup_reposition_after_parent_unmaps_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();

    let window = f.client(client).create_window();
    window.commit();
    let parent_surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&parent_surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let parent = f.client(client).window(&parent_surface).xdg_surface.clone();
    let popup = f.client(client).create_popup(&parent);
    let popup_surface = popup.surface.clone();
    popup.surface.commit();
    f.double_roundtrip(client);
    assert!(!f
        .client(client)
        .popup(&popup_surface)
        .configures_received
        .is_empty());

    let window = f.client(client).window(&parent_surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(client);

    let popup = f.client(client).popup(&popup_surface).xdg_popup.clone();
    f.client(client).state.reposition_popup(&popup, 42);
    f.client(client).connection.flush().unwrap();
    f.dispatch();
    f.client(client).dispatch_unchecked();

    assert!(f.client(client).connection.protocol_error().is_none());
    assert_eq!(f.client(client).popup(&popup_surface).repositioned, [42]);
}

#[test]
fn mutter_x11_interop_accepts_an_unmapped_surface() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let surface = f.client(client).create_window().surface.clone();

    let interop = f.client(client).state.mutter_x11_interop.clone().unwrap();
    interop.set_x11_parent(&surface, 42);
    f.client(client).connection.flush().unwrap();
    f.dispatch();
    f.client(client).dispatch_unchecked();

    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn client_interactive_move_requires_floating_and_non_fullscreen() {
    let (mut tiled, _) = set_up(WindowState::Tiled);
    assert!(!request_move(&mut tiled));

    let (mut floating, _) = set_up(WindowState::Floating);
    assert!(request_move(&mut floating));

    let (mut fullscreen, _) = set_up(WindowState::Fullscreen);
    assert!(!request_move(&mut fullscreen));
}

#[test]
fn client_interactive_resize_requires_floating() {
    let (mut tiled, _) = set_up(WindowState::Tiled);
    assert!(!request_resize(&mut tiled));

    let (mut floating, _) = set_up(WindowState::Floating);
    assert!(request_resize(&mut floating));

    let (mut fullscreen, _) = set_up(WindowState::Fullscreen);
    assert!(!request_resize(&mut fullscreen));
}

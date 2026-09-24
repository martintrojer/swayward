use smithay::output::Scale;
use smithay::reexports::wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1;
use wayland_client::protocol::wl_pointer;

use super::Fixture;

fn virtual_pointer(
    f: &mut Fixture,
    client: super::client::ClientId,
) -> zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1 {
    let state = &f.client(client).state;
    state
        .virtual_pointer_manager
        .as_ref()
        .unwrap()
        .create_virtual_pointer(state.seat.as_ref(), &state.qh, ())
}

#[test]
fn relative_and_absolute_motion_drive_the_real_pointer_path() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let pointer = virtual_pointer(&mut f, client);

    pointer.motion(1, 40., 30.);
    f.roundtrip(client);
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        (40., 30.).into()
    );

    pointer.motion_absolute(2, 3, 1, 4, 2);
    f.roundtrip(client);
    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        (600., 300.).into()
    );
}

#[test]
fn absolute_motion_uses_logical_output_geometry_at_fractional_scale() {
    let mut f = Fixture::new();
    f.add_output(1, (1200, 600));
    let output = f.niri_output(1);
    output.change_current_state(None, None, Some(Scale::Fractional(1.5)), None);
    f.swayward().output_resized(&output);
    let client = f.add_client();
    let pointer = virtual_pointer(&mut f, client);

    pointer.motion_absolute(1, 900, 450, 1200, 600);
    f.roundtrip(client);

    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        (600., 300.).into()
    );
}

#[test]
fn removed_output_mapping_falls_back_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    f.add_output_at(2, (800, 600), Some((800, 0)));
    let client = f.add_client();
    f.roundtrip(client);
    let (manager, seat, output, qh) = {
        let client = f.client(client);
        (
            client.state.virtual_pointer_manager.clone().unwrap(),
            client.state.seat.clone(),
            client.output("headless-2"),
            client.qh.clone(),
        )
    };
    let pointer = manager.create_virtual_pointer_with_output(seat.as_ref(), Some(&output), &qh, ());
    let removed = f
        .swayward()
        .layout
        .outputs()
        .find(|output| output.name() == "headless-2")
        .unwrap()
        .clone();
    f.swayward().remove_output(&removed);

    pointer.motion_absolute(1, 1, 1, 2, 2);
    f.roundtrip(client);

    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        (400., 300.).into()
    );
    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn zero_extent_absolute_motion_and_unknown_button_are_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let pointer = virtual_pointer(&mut f, client);

    pointer.motion_absolute(1, 1, 1, 0, 0);
    pointer.button(2, u32::MAX, wl_pointer::ButtonState::Pressed);
    pointer.button(3, u32::MAX, wl_pointer::ButtonState::Released);
    f.roundtrip(client);

    assert_eq!(
        f.swayward().seat.get_pointer().unwrap().current_location(),
        (0., 0.).into()
    );
    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn axis_frames_are_delivered_only_when_finished() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let pointer = virtual_pointer(&mut f, client);

    pointer.axis(1, wl_pointer::Axis::VerticalScroll, 1.);
    f.roundtrip(client);
    assert!(f.client(client).connection.protocol_error().is_none());

    pointer.frame();
    pointer.axis_discrete(2, wl_pointer::Axis::HorizontalScroll, 1., 1);
    pointer.axis_source(wl_pointer::AxisSource::WheelTilt);
    pointer.axis_stop(2, wl_pointer::Axis::VerticalScroll);
    pointer.frame();
    f.roundtrip(client);

    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn extreme_discrete_axis_value_does_not_panic() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let pointer = virtual_pointer(&mut f, client);

    pointer.axis_discrete(1, wl_pointer::Axis::VerticalScroll, 1., i32::MAX);
    pointer.frame();
    f.roundtrip(client);

    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn disconnect_with_an_unfinished_axis_frame_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let pointer = virtual_pointer(&mut f, client);

    pointer.axis(1, wl_pointer::Axis::VerticalScroll, 1.);
    pointer.destroy();
    f.roundtrip(client);

    assert!(f.client(client).connection.protocol_error().is_none());
}

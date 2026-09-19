use smithay::output::Scale;
use wayland_client::protocol::wl_shm;

use super::Fixture;

#[test]
fn advertises_fractionally_scaled_region_buffer() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let output = f.niri_output(1);
    output.change_current_state(None, None, Some(Scale::Fractional(1.5)), None);

    let client = f.add_client();
    f.double_roundtrip(client);
    let wl_output = f.client(client).output("headless-1");
    let frame = f
        .client(client)
        .capture_output(&wl_output, false, Some((11, 13, 101, 51)));
    f.roundtrip(client);

    let events = frame.events.lock().unwrap();
    assert_eq!(
        events.buffer,
        Some((wl_shm::Format::Xrgb8888, 152, 77, 608))
    );
    assert_eq!(events.linux_dmabuf, Some((0x34325258, 152, 77)));
    assert!(events.buffer_done);
    assert!(!events.failed);
}

#[test]
fn region_and_cursor_flag_reach_the_render_queue() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let output = f.niri_output(1);
    output.change_current_state(None, None, Some(Scale::Fractional(1.5)), None);
    let client = f.add_client();
    f.double_roundtrip(client);
    let wl_output = f.client(client).output("headless-1");
    let frame = f
        .client(client)
        .capture_output(&wl_output, true, Some((11, 13, 101, 51)));
    f.roundtrip(client);

    let buffer = f.client(client).create_shm_buffer(
        152,
        77,
        152 * 4,
        wl_shm::Format::Xrgb8888,
        152 * 77 * 4,
    );
    frame.proxy.copy_with_damage(&buffer);
    f.client(client).connection.flush().unwrap();
    f.dispatch();

    let mut checked = false;
    f.swayward().screencopy_state.with_queues_mut(|queued| {
        let screencopy = queued.split().1.unwrap();
        assert_eq!(screencopy.region_loc(), (17, 20).into());
        assert_eq!(screencopy.buffer_size(), (152, 77).into());
        assert!(screencopy.overlay_cursor());
        assert!(screencopy.with_damage());
        checked = true;
    });
    assert!(checked);
}

#[test]
fn removed_output_capture_fails_without_panicking() {
    let mut f = Fixture::new();
    f.add_output(1, (320, 240));
    let client = f.add_client();
    f.double_roundtrip(client);
    let wl_output = f.client(client).output("headless-1");
    let output = f.niri_output(1);
    f.swayward().remove_output(&output);

    let frame = f.client(client).capture_output(&wl_output, false, None);
    f.roundtrip(client);

    assert!(frame.events.lock().unwrap().failed);
}

#[test]
fn wrong_sized_buffer_is_rejected() {
    let mut f = Fixture::new();
    f.add_output(1, (320, 240));
    let client = f.add_client();
    f.double_roundtrip(client);
    let wl_output = f.client(client).output("headless-1");
    let frame = f.client(client).capture_output(&wl_output, false, None);
    f.roundtrip(client);

    let buffer = f.client(client).create_shm_buffer(
        319,
        240,
        319 * 4,
        wl_shm::Format::Xrgb8888,
        319 * 240 * 4,
    );
    frame.proxy.copy(&buffer);
    f.client(client).connection.flush().unwrap();
    f.dispatch();
    f.client(client).dispatch_unchecked();

    let error = f.client(client).connection.protocol_error().unwrap();
    assert_eq!(error.code, 1);
}

#[test]
fn larger_shm_pool_is_accepted() {
    let mut f = Fixture::new();
    f.add_output(1, (32, 24));
    f.niri_state().backend.headless().add_renderer().unwrap();
    let client = f.add_client();
    f.double_roundtrip(client);
    let wl_output = f.client(client).output("headless-1");
    let frame = f.client(client).capture_output(&wl_output, false, None);
    f.roundtrip(client);

    let expected_len = 32 * 24 * 4;
    let buffer = f.client(client).create_shm_buffer(
        32,
        24,
        32 * 4,
        wl_shm::Format::Xrgb8888,
        expected_len + 4096,
    );
    frame.proxy.copy(&buffer);
    f.client(client).connection.flush().unwrap();
    f.roundtrip(client);

    assert!(frame.events.lock().unwrap().ready);
}

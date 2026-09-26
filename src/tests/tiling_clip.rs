//! A tiled window whose surface is larger than its container is clipped to
//! the container, as sway does in `view_center_and_clip_surface`
//! (`sway/sway/tree/view.c:1032-1062`): tiled views always clip, with the
//! clip box sized to `con->current.content_width/height`. Without it a client
//! that will not shrink (Chrome below its minimum width, or any client that
//! has not yet answered a configure) paints over its neighbour.

use std::os::fd::OwnedFd;

use smithay::reexports::rustix::fs::{fstat, seek, SeekFrom};
use smithay::reexports::rustix::io::read;
use swayward_config::Config;
use wayland_client::protocol::wl_shm;
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::*;

const OUTPUT: (u16, u16) = (400, 200);

fn config() -> Config {
    Config::parse_mem(
        r#"
        animations { off; }
        layout {
            gaps 0
            default-border "none"
        }
        "#,
    )
    .unwrap()
}

fn map_window(f: &mut Fixture, id: ClientId, color: (u8, u8, u8)) -> WlSurface {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_color_buffer(color.0, color.1, color.2);
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    surface
}

/// Acks the latest configure and commits at the configured size, or at an
/// explicit size to model a client that ignores the configure.
fn commit_at(f: &mut Fixture, id: ClientId, surface: &WlSurface, size: Option<(u16, u16)>) {
    let window = f.client(id).window(surface);
    let configured = window.configures_received.last().unwrap().1.size;
    let (w, h) = size.unwrap_or((configured.0 as u16, configured.1 as u16));
    window.set_size(w, h);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
}

/// Captures the whole output and returns it as XRGB8888 rows.
fn capture(f: &mut Fixture, id: ClientId) -> (u32, Vec<u8>) {
    let wl_output = f.client(id).output("headless-1");
    let frame = f.client(id).capture_output(&wl_output, false, None);
    f.roundtrip(id);

    let (format, width, height, stride) = frame.events.lock().unwrap().buffer.unwrap();
    assert_eq!(format, wl_shm::Format::Xrgb8888);
    let (buffer, fd) =
        f.client(id)
            .create_readable_shm_buffer(width as i32, height as i32, stride as i32, format);
    frame.proxy.copy(&buffer);
    f.client(id).connection.flush().unwrap();
    f.roundtrip(id);
    f.roundtrip(id);
    {
        let events = frame.events.lock().unwrap();
        assert!(events.ready && !events.failed, "{events:?}");
    }
    (stride, read_all(&fd))
}

fn read_all(fd: &OwnedFd) -> Vec<u8> {
    let len = fstat(fd).unwrap().st_size as usize;
    let mut bytes = vec![0; len];
    seek(fd, SeekFrom::Start(0)).unwrap();
    let mut filled = 0;
    while filled < len {
        let n = read(fd, &mut bytes[filled..]).unwrap();
        assert!(n > 0);
        filled += n;
    }
    bytes
}

fn pixel(stride: u32, bytes: &[u8], x: u32, y: u32) -> (u8, u8, u8) {
    // XRGB8888 is little-endian: B, G, R, X in memory.
    let i = (y * stride + x * 4) as usize;
    (bytes[i + 2], bytes[i + 1], bytes[i])
}

#[test]
fn oversized_tiled_surface_does_not_paint_over_its_neighbour() {
    const RED: (u8, u8, u8) = (255, 0, 0);
    const BLUE: (u8, u8, u8) = (0, 0, 255);

    let mut f = Fixture::with_config(config());
    f.add_output(1, OUTPUT);
    f.niri_state().backend.headless().add_renderer().unwrap();
    let id = f.add_client();

    let left = map_window(&mut f, id, RED);
    let right = map_window(&mut f, id, BLUE);

    // Settle both into their 200-wide halves.
    commit_at(&mut f, id, &right, None);
    commit_at(&mut f, id, &left, None);
    assert_eq!(
        f.client(id)
            .window(&left)
            .configures_received
            .last()
            .unwrap()
            .1
            .size,
        (200, 200)
    );

    // Focus the left window so it renders above its neighbour, then have it
    // refuse to shrink below 300 logical pixels, as Chrome does.
    assert!(f.swayward().layout.focus_left());
    f.double_roundtrip(id);
    commit_at(&mut f, id, &right, None);
    commit_at(&mut f, id, &left, Some((300, 200)));

    let (stride, bytes) = capture(&mut f, id);
    assert_eq!(pixel(stride, &bytes, 100, 100), RED, "left slot");
    assert_eq!(
        pixel(stride, &bytes, 250, 100),
        BLUE,
        "the left window's surface overflows into the right slot"
    );
    assert_eq!(pixel(stride, &bytes, 350, 100), BLUE, "right slot");

    // Input follows the clip: the undrawn overflow does not take the pointer
    // from the neighbour drawn beneath it.
    let output = f.niri_output(1);
    let hit = |f: &mut Fixture, x: f64| {
        let layout = &f.swayward().layout;
        let (window, _) = layout.window_under(&output, (x, 100.).into()).unwrap();
        window.toplevel().wl_surface().clone()
    };
    let (left_server, right_server) = (hit(&mut f, 100.), hit(&mut f, 350.));
    assert_ne!(left_server, right_server);
    assert_eq!(
        hit(&mut f, 250.),
        right_server,
        "overflow steals the pointer"
    );
}

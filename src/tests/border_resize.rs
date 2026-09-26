//! A plain left drag on a window border resizes it, as sway does with no
//! modifier held (`sway/sway/input/seatop_default.c:396-410`). Only an edge
//! shared with a sibling counts for a tiled window; an edge against the
//! workspace is left alone (`find_resize_edge`, `:111-118`).

use smithay::reexports::wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;
use swayward_config::Config;
use wayland_client::protocol::wl_pointer::ButtonState;
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::*;

const OUTPUT: (u16, u16) = (400, 200);
const BTN_LEFT: u32 = 0x110;

fn config(border_resize: bool) -> Config {
    Config::parse_mem(&format!(
        r#"
        animations {{ off; }}
        input {{ border-resize {border_resize}; }}
        layout {{
            gaps 0
            default-border "pixel" width=4
            border {{ on; width 4; }}
        }}
        "#
    ))
    .unwrap()
}

fn map_window(f: &mut Fixture, id: ClientId) -> WlSurface {
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    surface
}

fn commit_configured(f: &mut Fixture, id: ClientId, surface: &WlSurface) {
    let window = f.client(id).window(surface);
    let size = window.configures_received.last().unwrap().1.size;
    window.set_size(size.0 as u16, size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
}

fn tile_widths(f: &mut Fixture) -> Vec<f64> {
    let workspace = f.swayward().layout.active_workspace().unwrap();
    let mut tiles: Vec<_> = workspace
        .tiles_with_render_positions()
        .map(|(tile, pos, _)| (pos.x, tile.tile_size().w))
        .collect();
    tiles.sort_by(|a, b| a.0.total_cmp(&b.0));
    tiles.into_iter().map(|(_, w)| w).collect()
}

struct Setup {
    f: Fixture,
    id: ClientId,
    pointer: ZwlrVirtualPointerV1,
    surfaces: [WlSurface; 2],
}

fn setup(border_resize: bool) -> Setup {
    let mut f = Fixture::with_config(config(border_resize));
    f.add_output(1, OUTPUT);
    let id = f.add_client();
    let left = map_window(&mut f, id);
    let right = map_window(&mut f, id);
    commit_configured(&mut f, id, &left);
    commit_configured(&mut f, id, &right);
    assert_eq!(tile_widths(&mut f), [200., 200.]);

    let pointer = {
        let state = &f.client(id).state;
        state
            .virtual_pointer_manager
            .as_ref()
            .unwrap()
            .create_virtual_pointer(state.seat.as_ref(), &state.qh, ())
    };
    Setup {
        f,
        id,
        pointer,
        surfaces: [left, right],
    }
}

/// Presses at `x`, drags by `dx`, releases, all at mid-height, and lets both
/// clients commit whatever size they were configured to.
fn drag(s: &mut Setup, x: u32, dx: i32) {
    let Setup {
        f,
        id,
        pointer,
        surfaces,
    } = s;
    let id = *id;
    let (w, h) = (u32::from(OUTPUT.0), u32::from(OUTPUT.1));
    pointer.motion_absolute(1, x, h / 2, w, h);
    pointer.frame();
    f.roundtrip(id);
    pointer.button(2, BTN_LEFT, ButtonState::Pressed);
    pointer.frame();
    f.roundtrip(id);
    pointer.motion(3, f64::from(dx), 0.);
    pointer.frame();
    f.roundtrip(id);
    pointer.button(4, BTN_LEFT, ButtonState::Released);
    pointer.frame();
    f.double_roundtrip(id);
    for surface in surfaces.iter() {
        commit_configured(f, id, surface);
    }
}

#[test]
fn dragging_a_shared_border_resizes_both_neighbours() {
    let mut s = setup(true);

    // The left window's right border covers x in [196, 200).
    drag(&mut s, 198, 50);

    assert_eq!(tile_widths(&mut s.f), [250., 150.]);
}

#[test]
fn an_outer_border_is_not_a_resize_handle() {
    let mut s = setup(true);

    // The left window's left border touches the workspace edge.
    drag(&mut s, 1, 50);

    assert_eq!(tile_widths(&mut s.f), [200., 200.]);
}

#[test]
fn border_resize_false_leaves_the_border_inert() {
    let mut s = setup(false);

    drag(&mut s, 198, 50);

    assert_eq!(tile_widths(&mut s.f), [200., 200.]);
}

//! A plain left drag on a window border resizes it, as sway does with no
//! modifier held (`sway/sway/input/seatop_default.c:396-410`). Only an edge
//! shared with a sibling counts for a tiled window; an edge against the
//! workspace is left alone (`find_resize_edge`, `:111-118`).
//!
//! `input { gap-resize }` extends the same drag to the gap between two tiled
//! windows, for borderless setups. Sway has no such handle.

use smithay::reexports::wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;
use swayward_config::Config;
use wayland_client::protocol::wl_pointer::ButtonState;
use wayland_client::protocol::wl_surface::WlSurface;

use super::client::ClientId;
use super::*;

const OUTPUT: (u16, u16) = (400, 200);
const BTN_LEFT: u32 = 0x110;

fn config(border_resize: bool) -> Config {
    config_from(&format!(
        r#"
        input {{ border-resize {border_resize}; }}
        layout {{
            gaps 0
            default-border "pixel" width=4
            border {{ on; width 4; }}
        }}
        "#
    ))
}

/// Borderless with a 10px gap, left and right edges at x 10 and 390, so the
/// shared gap spans x in [195, 205).
fn gap_config(gap_resize: bool) -> Config {
    config_from(&format!(
        r#"
        input {{ gap-resize {gap_resize}; }}
        layout {{
            gaps 10
            default-border "none"
            border {{ off; }}
        }}
        "#
    ))
}

fn config_from(extra: &str) -> Config {
    Config::parse_mem(&format!("animations {{ off; }}\n{extra}")).unwrap()
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
    let (serial, configure) = window.configures_received.last().unwrap();
    if window.last_acked_configure == Some(*serial) {
        return;
    }
    let size = configure.size;
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
    setup_with(config(border_resize), [200., 200.])
}

fn setup_with(config: Config, widths: [f64; 2]) -> Setup {
    let mut f = Fixture::with_config(config);
    f.add_output(1, OUTPUT);
    let id = f.add_client();
    let left = map_window(&mut f, id);
    let right = map_window(&mut f, id);
    commit_configured(&mut f, id, &left);
    commit_configured(&mut f, id, &right);
    assert_eq!(tile_widths(&mut f), widths);

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

#[test]
fn dragging_the_gap_between_tiles_resizes_both_neighbours() {
    let mut s = setup_with(gap_config(true), [185., 185.]);

    drag(&mut s, 200, 50);

    // The split is stored as fractions and each tile is rounded on its own,
    // so the left tile may land a pixel short.
    let widths = tile_widths(&mut s.f);
    assert_eq!(widths[1], 135., "{widths:?}");
    assert!((widths[0] - 235.).abs() <= 1., "{widths:?}");
}

#[test]
fn an_outer_gap_is_not_a_resize_handle() {
    let mut s = setup_with(gap_config(true), [185., 185.]);

    drag(&mut s, 5, 50);

    assert_eq!(tile_widths(&mut s.f), [185., 185.]);
}

#[test]
fn gap_resize_is_off_by_default() {
    let mut s = setup_with(gap_config(false), [185., 185.]);

    drag(&mut s, 200, 50);

    assert_eq!(tile_widths(&mut s.f), [185., 185.]);
}

/// `[tabbed: A B] | C`, with `visible` the shown tab and focused.
fn tabbed_setup(visible: &str) -> (Setup, [WlSurface; 3]) {
    let mut s = setup(true);
    let id = s.id;
    let f = &mut s.f;
    let [a, c] = s.surfaces.clone();
    f.client(id).window(&a).set_title("A");
    f.client(id).window(&c).set_title("C");
    assert!(f.swayward().layout.focus_left());
    assert!(crate::command::execute(f.niri_state(), "splith")[0].success);
    let b = map_window(f, id);
    f.client(id).window(&b).set_title("B");
    assert!(crate::command::execute(f.niri_state(), "layout tabbed")[0].success);
    if visible == "A" {
        assert!(crate::command::execute(f.niri_state(), "focus left")[0].success);
    }
    let all = [a, b, c];
    settle(f, id, &all);
    assert_eq!(focused_title(f).as_deref(), Some(visible));
    (s, all)
}

fn settle(f: &mut Fixture, id: ClientId, surfaces: &[WlSurface]) {
    for _ in 0..3 {
        for surface in surfaces {
            commit_configured(f, id, surface);
        }
    }
}

fn focused_title(f: &mut Fixture) -> Option<String> {
    use crate::layout::LayoutElement as _;
    f.swayward().layout.focus().map(|mapped| mapped.title())
}

/// C's width: the tab container and C share the output, so it measures
/// where the boundary between them sits.
fn right_width(f: &mut Fixture) -> f64 {
    use crate::layout::LayoutElement as _;
    let ws = f.swayward().layout.active_workspace().unwrap();
    ws.tiles_with_render_positions()
        .find(|(tile, _, _)| tile.window().title() == "C")
        .map(|(tile, _, _)| tile.tile_size().w)
        .unwrap()
}

#[test]
fn every_tab_resizes_from_the_border_it_shares_with_a_neighbour() {
    // The first tab once found its tab siblings as a "neighbour" across its
    // right edge, so the drag resized nothing. Sway only counts an exactly
    // parallel split (`sway/sway/commands/resize.c:45-64`).
    for visible in ["A", "B"] {
        let (mut s, all) = tabbed_setup(visible);
        drag(&mut s, 198, 50);
        settle(&mut s.f, s.id, &all);

        assert_eq!(right_width(&mut s.f), 150., "tab {visible} shown");
        assert_eq!(focused_title(&mut s.f).as_deref(), Some(visible));
    }
}

/// Recommits `surface` like a client drawing its own decorations: the buffer
/// is `margin` larger on every side than the window geometry, and the
/// default input region covers all of it.
fn commit_with_csd_margin(f: &mut Fixture, id: ClientId, surface: &WlSurface, margin: i32) {
    let window = f.client(id).window(surface);
    let (w, h) = window.configures_received.last().unwrap().1.size;
    window.set_size((w + margin * 2) as u16, (h + margin * 2) as u16);
    window.xdg_surface.set_window_geometry(margin, margin, w, h);
    window.commit();
    f.double_roundtrip(id);
}

#[test]
fn a_tiled_window_input_margin_does_not_cover_the_gap() {
    // Sway clips a tiled view to its geometry, input included
    // (`sway/sway/tree/view.c:1032-1062`), so the resize margin a client keeps
    // around its own decorations never reaches into the gap.
    let mut s = setup_with(gap_config(true), [185., 185.]);
    let left = s.surfaces[0].clone();
    commit_with_csd_margin(&mut s.f, s.id, &left, 10);

    let output = s.f.niri_output(1);
    let over_gap =
        s.f.swayward()
            .layout
            .window_under(&output, (200., 100.).into());
    assert!(over_gap.is_none(), "the margin takes the gap");

    // `drag` recommits both windows at their configured size after the
    // release, which only affects what happens afterwards.
    drag(&mut s, 200, 50);
    let widths = tile_widths(&mut s.f);
    assert_eq!(widths[1], 135., "{widths:?}");
}

#[test]
fn a_popup_over_the_gap_keeps_its_input() {
    // Only the toplevel is clipped: a menu that opens across the gap still
    // takes the click there, rather than the gap handle.
    let mut s = setup_with(gap_config(true), [185., 185.]);
    let right = s.surfaces[1].clone();
    let parent = s.f.client(s.id).window(&right).xdg_surface.clone();
    // A 100x100 popup centred on the right window's top-left corner, at
    // (205, 10), spans the gap at x in [195, 205) from y 10 down.
    let popup = s.f.client(s.id).create_popup(&parent).surface.clone();
    popup.commit();
    s.f.double_roundtrip(s.id);
    // The test client acks popup configures itself.
    let buffer = {
        let window = s.f.client(s.id).window(&right);
        window
            .spbm
            .create_u32_rgba_buffer(0, 0, 0, u32::MAX, &window.qh, ())
    };
    // Scale the single-pixel buffer up to the 100x100 the positioner asked for.
    let viewport = {
        let state = &s.f.client(s.id).state;
        state
            .viewporter
            .as_ref()
            .unwrap()
            .get_viewport(&popup, &state.qh, ())
    };
    viewport.set_destination(100, 100);
    popup.attach(Some(&buffer), 0, 0);
    popup.commit();
    s.f.double_roundtrip(s.id);

    let output = s.f.niri_output(1);
    let hit =
        s.f.swayward()
            .layout
            .window_under(&output, (200., 30.).into())
            .map(|(_, hit)| matches!(hit, crate::layout::HitType::Input { .. }));
    assert_eq!(hit, Some(true), "the popup lost its input over the gap");
}

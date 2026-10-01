use insta::assert_snapshot;
use swayward_config::Config;
use swayward_ipc::SizeChange;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::client::ClientId;
use super::*;

fn set_up() -> (Fixture, ClientId, WlSurface) {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));
    fixture.add_output(2, (1280, 720));
    let client = fixture.add_client();
    let surface = windows::map_window(&mut fixture, client, windows::WindowSpec::sized(100, 100));
    (fixture, client, surface)
}

#[test]
fn floating_honors_committed_xdg_min_max_size_and_zero_sentinels() {
    let (mut f, id, surface) = set_up();
    let window = f.client(id).window(&surface);
    window.set_min_size(300, 250);
    window.set_max_size(400, 350);
    window.commit();
    f.double_roundtrip(id);

    let _ = f.client(id).window(&surface).recent_configures();
    f.swayward().layout.toggle_window_floating(None);
    f.double_roundtrip(id);
    assert_snapshot!(
        f.client(id).window(&surface).format_recent_configures(),
        @"size: 300 × 250, bounds: 1920 × 1080, states: [Activated]"
    );

    let window = f.client(id).window(&surface);
    window.ack_last_and_commit();
    let _ = f.client(id).window(&surface).recent_configures();
    f.swayward()
        .layout
        .set_focused_width(SizeChange::SetFixed(500));
    f.swayward()
        .layout
        .set_window_height(None, SizeChange::SetFixed(450));
    f.double_roundtrip(id);
    assert_snapshot!(
        f.client(id).window(&surface).format_recent_configures(),
        @"size: 400 × 350, bounds: 1920 × 1080, states: [Activated]"
    );

    let window = f.client(id).window(&surface);
    window.ack_last_and_commit();
    window.set_min_size(0, 0);
    window.set_max_size(0, 0);
    window.commit();
    f.double_roundtrip(id);
    let _ = f.client(id).window(&surface).recent_configures();

    f.swayward()
        .layout
        .set_focused_width(SizeChange::SetFixed(500));
    f.swayward()
        .layout
        .set_window_height(None, SizeChange::SetFixed(200));
    f.double_roundtrip(id);
    assert_snapshot!(
        f.client(id).window(&surface).format_recent_configures(),
        @"size: 500 × 200, bounds: 1920 × 1080, states: [Activated]"
    );
}

#[test]
fn global_constraints_clamp_initial_floating_natural_size_before_client_hints() {
    let config = Config::parse_mem(
        r#"
layout {
    floating-minimum-size 60 40
    floating-maximum-size 100 90
}
window-rule {
    open-floating true
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.set_size(20, 20);
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.set_size(20, 20);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    assert_snapshot!(
        f.client(id).window(&surface).format_recent_configures(),
        @r###"
        size: 0 × 0, bounds: 1920 × 1080, states: []
        size: 60 × 40, bounds: 1920 × 1080, states: [Activated]
        "###
    );
}

#[test]
fn uncommitted_xdg_min_max_size_does_not_constrain_floating() {
    let (mut f, id, surface) = set_up();
    let window = f.client(id).window(&surface);
    window.set_min_size(300, 250);
    window.set_max_size(400, 350);

    let _ = f.client(id).window(&surface).recent_configures();
    f.swayward().layout.toggle_window_floating(None);
    f.double_roundtrip(id);
    assert_snapshot!(
        f.client(id).window(&surface).format_recent_configures(),
        @"size: 100 × 100, bounds: 1920 × 1080, states: [Activated]"
    );
}

#[test]
fn unmapping_focused_floating_restores_previous_tiling_focus() {
    let (mut f, id, _) = set_up();

    let second = f.client(id).create_window();
    let second_surface = second.surface.clone();
    second.commit();
    f.roundtrip(id);
    let second = f.client(id).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(id);

    let second_window = f.swayward().layout.focus().unwrap().window.clone();
    f.swayward().layout.activate_window(&second_window);
    let second_id = f.swayward().layout.focus().unwrap().id();

    let floating = f.client(id).create_window();
    let floating_surface = floating.surface.clone();
    floating.commit();
    f.roundtrip(id);
    let floating = f.client(id).window(&floating_surface);
    floating.attach_new_buffer();
    floating.ack_last_and_commit();
    f.double_roundtrip(id);
    f.swayward().layout.toggle_window_floating(None);
    f.double_roundtrip(id);

    f.client(id).window(&floating_surface).attach_null();
    f.client(id).window(&floating_surface).commit();
    f.double_roundtrip(id);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), second_id);
}

#[test]
fn floating_directional_focus_uses_nearest_center_and_wraps() {
    let (mut f, client, _) = set_up();
    let mut windows = Vec::new();
    for x in [100., 200., 300.] {
        f.swayward().layout.toggle_window_floating(None);
        f.swayward().layout.move_floating_window(
            None,
            swayward_ipc::PositionChange::SetFixed(x),
            swayward_ipc::PositionChange::SetFixed(100.),
            false,
        );
        windows.push(f.swayward().layout.focus().unwrap().id());
        if x < 300. {
            let window = f.client(client).create_window();
            let surface = window.surface.clone();
            window.commit();
            f.roundtrip(client);
            let window = f.client(client).window(&surface);
            window.attach_new_buffer();
            window.ack_last_and_commit();
            f.double_roundtrip(client);
        }
    }

    f.swayward().layout.focus_left();
    assert_eq!(f.swayward().layout.focus().unwrap().id(), windows[1]);
    f.swayward().layout.focus_left();
    assert_eq!(f.swayward().layout.focus().unwrap().id(), windows[0]);
    f.swayward().layout.focus_left();
    assert_eq!(f.swayward().layout.focus().unwrap().id(), windows[2]);
}

/// Two mapped windows must tile side by side through the real compositor, and a
/// directional focus move must land on the other one. This is the headless
/// equivalent of the manual two-terminal check: it drives real Wayland clients
/// through the real layout and checks the configured sizes, the rendered
/// positions and which window gains focus.
#[test]
fn two_windows_tile_side_by_side_and_focus_follows() {
    let (mut f, id, first_surface) = set_up();

    let second = f.client(id).create_window();
    let second_surface = second.surface.clone();
    second.commit();
    f.roundtrip(id);
    let second = f.client(id).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(id);
    f.client(id).window(&first_surface).ack_last_and_commit();
    f.roundtrip(id);

    let last_size = |f: &mut Fixture, surface: &WlSurface| {
        f.client(id)
            .window(surface)
            .configures_received
            .last()
            .unwrap()
            .1
            .size
    };
    // swayward's default gaps are 0, so two leaves split the 1920px output.
    assert_eq!(last_size(&mut f, &first_surface), (960, 1080));
    assert_eq!(last_size(&mut f, &second_surface), (960, 1080));

    let mut tiles = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .tiles_with_render_positions()
        .map(|(_, pos, _)| pos.x)
        .collect::<Vec<_>>();
    tiles.sort_by(f64::total_cmp);
    assert_eq!(tiles, [0., 960.], "the leaves must sit side by side");

    let focused = |f: &mut Fixture| {
        f.swayward()
            .layout
            .focus()
            .unwrap()
            .toplevel()
            .wl_surface()
            .id()
            .protocol_id()
    };
    assert_eq!(focused(&mut f), second_surface.id().protocol_id());
    f.swayward().layout.focus_left();
    assert_eq!(
        focused(&mut f),
        first_surface.id().protocol_id(),
        "focus_left must move focus to the left leaf"
    );
}

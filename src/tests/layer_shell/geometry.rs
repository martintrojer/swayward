use insta::assert_snapshot;
use smithay::desktop::layer_map_for_output;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    Anchor, KeyboardInteractivity,
};
use smithay::reexports::wayland_server::Resource as _;
use smithay::utils::Rectangle;
use wayland_client::Proxy as _;

use super::*;
use crate::tests::client::{ClientId, LayerConfigureProps, LayerMargin};

fn focused_surface_id(f: &mut Fixture) -> u32 {
    f.swayward()
        .keyboard_focus
        .surface()
        .unwrap()
        .id()
        .protocol_id()
}

fn map_layer(
    f: &mut Fixture,
    client: ClientId,
    layer_kind: Layer,
    namespace: &str,
    props: LayerConfigureProps,
) -> wayland_client::protocol::wl_surface::WlSurface {
    let layer = f.client(client).create_layer(None, layer_kind, namespace);
    let surface = layer.surface.clone();
    layer.set_configure_props(props);
    layer.commit();
    f.double_roundtrip(client);

    let layer = f.client(client).layer(&surface);
    let size = layer.configures_received.last().unwrap().1.size;
    layer.attach_new_buffer();
    layer.set_size(size.0 as u16, size.1 as u16);
    layer.ack_last_and_commit();
    f.double_roundtrip(client);
    surface
}

#[test]
fn exclusive_layers_shrink_tiling_before_non_exclusive_layers_are_arranged() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let all = Anchor::Left | Anchor::Right | Anchor::Top | Anchor::Bottom;

    let neutral = map_layer(
        &mut f,
        client,
        Layer::Top,
        "neutral",
        LayerConfigureProps {
            anchor: Some(all),
            exclusive_zone: Some(0),
            ..Default::default()
        },
    );
    let negative = map_layer(
        &mut f,
        client,
        Layer::Background,
        "negative",
        LayerConfigureProps {
            anchor: Some(all),
            exclusive_zone: Some(-1),
            ..Default::default()
        },
    );
    map_layer(
        &mut f,
        client,
        Layer::Bottom,
        "bottom-dock",
        LayerConfigureProps {
            anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
            size: Some((0, 30)),
            exclusive_zone: Some(30),
            ..Default::default()
        },
    );
    map_layer(
        &mut f,
        client,
        Layer::Overlay,
        "top-dock",
        LayerConfigureProps {
            anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
            size: Some((0, 40)),
            exclusive_zone: Some(40),
            ..Default::default()
        },
    );

    let output = f.niri_output(1);
    let map = layer_map_for_output(&output);
    let geometry = |namespace| {
        map.layers()
            .find(|layer| layer.namespace() == namespace)
            .and_then(|layer| map.layer_geometry(layer))
            .unwrap()
    };
    assert_eq!(geometry("top-dock").loc.y, 0);
    assert_eq!(geometry("bottom-dock").loc.y, 40);
    drop(map);

    let area = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .working_area();
    assert_eq!(area.loc, (0., 70.).into());
    assert_eq!(area.size, (1280., 730.).into());
    assert_eq!(
        f.client(client)
            .layer(&neutral)
            .configures_received
            .last()
            .unwrap()
            .1
            .size,
        (1280, 730)
    );
    assert_eq!(
        f.client(client)
            .layer(&negative)
            .configures_received
            .last()
            .unwrap()
            .1
            .size,
        (1280, 800)
    );
}

#[test]
fn explicit_move_position_is_not_clamped_to_exclusive_zones_like_sway() {
    let cases = [
        (
            "top",
            Anchor::Left | Anchor::Right | Anchor::Top,
            (0, 30),
            (0., -10_000.),
        ),
        (
            "bottom",
            Anchor::Left | Anchor::Right | Anchor::Bottom,
            (0, 30),
            (0., 10_000.),
        ),
        (
            "left",
            Anchor::Top | Anchor::Bottom | Anchor::Left,
            (40, 0),
            (-10_000., 0.),
        ),
        (
            "right",
            Anchor::Top | Anchor::Bottom | Anchor::Right,
            (40, 0),
            (10_000., 0.),
        ),
    ];

    for (edge, anchor, size, position) in cases {
        let mut config = swayward_config::Config::default();
        config.animations.off = true;
        let mut f = Fixture::with_config(config);
        f.add_output(1, (800, 600));
        let client = f.add_client();
        map_layer(
            &mut f,
            client,
            Layer::Top,
            "bar",
            LayerConfigureProps {
                anchor: Some(anchor),
                size: Some(size),
                exclusive_zone: Some(if edge == "left" || edge == "right" {
                    40
                } else {
                    30
                }),
                ..Default::default()
            },
        );

        let window = f.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.set_size(200, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        f.swayward().layout.toggle_window_floating(None);
        f.double_roundtrip(client);
        let window = f.client(client).window(&surface);
        window.set_size(200, 100);
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        f.swayward().layout.move_floating_window(
            None,
            swayward_ipc::PositionChange::SetFixed(position.0),
            swayward_ipc::PositionChange::SetFixed(position.1),
            false,
        );
        let id = f.swayward().layout.focus().unwrap().window.clone();
        assert!(f
            .swayward()
            .layout
            .set_window_border(&id, swayward_ipc::command::BorderStyle::Pixel, Some(12))
            .is_ok());
        // Sway honours an explicit `move position` verbatim. Only the pointer
        // form corrects into bounds, and then against the output box rather
        // than the workspace (`sway/sway/commands/move.c:757-771`); the
        // coordinate forms call `container_floating_move_to` unclamped
        // (:709, :775, :831, :917), which itself performs no bounds check
        // (`sway/sway/tree/container.c:1127-1159`).
        //
        // niri clamped every floating position into the working area instead.
        // Asserting that here encoded niri's rule, so a request a sway client
        // is entitled to make was silently overridden.
        let workspace = f.swayward().layout.active_workspace().unwrap();
        let (tile, pos, _) = workspace.tiles_with_render_positions().next().unwrap();
        let size = tile.tile_size();
        let moved_where_asked = match edge {
            "top" | "bottom" => pos.y.abs() > 1000.,
            "left" | "right" => pos.x.abs() > 1000.,
            _ => unreachable!(),
        };
        assert!(
            moved_where_asked,
            "explicit move position must not be clamped for the {edge} case, \
             got {pos:?} with size {size:?}"
        );
    }
}

#[test]
fn layer_anchors_produce_sway_geometry() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let cases = [
        ("top", Anchor::Top, (100, 50), (350, 0, 100, 50)),
        ("bottom", Anchor::Bottom, (100, 50), (350, 550, 100, 50)),
        ("left", Anchor::Left, (100, 50), (0, 275, 100, 50)),
        ("right", Anchor::Right, (100, 50), (700, 275, 100, 50)),
        (
            "top-left",
            Anchor::Top | Anchor::Left,
            (100, 50),
            (0, 0, 100, 50),
        ),
        (
            "bottom-right",
            Anchor::Bottom | Anchor::Right,
            (100, 50),
            (700, 550, 100, 50),
        ),
        (
            "top-edge",
            Anchor::Top | Anchor::Left | Anchor::Right,
            (0, 50),
            (0, 0, 800, 50),
        ),
        (
            "left-edge",
            Anchor::Top | Anchor::Bottom | Anchor::Left,
            (100, 0),
            (0, 0, 100, 600),
        ),
        (
            "opposing-horizontal",
            Anchor::Left | Anchor::Right,
            (0, 50),
            (0, 275, 800, 50),
        ),
        (
            "fill",
            Anchor::Top | Anchor::Right | Anchor::Bottom | Anchor::Left,
            (0, 0),
            (0, 0, 800, 600),
        ),
    ];

    for (namespace, anchor, size, _) in cases {
        map_layer(
            &mut f,
            client,
            Layer::Top,
            namespace,
            LayerConfigureProps {
                anchor: Some(anchor),
                size: Some(size),
                exclusive_zone: Some(-1),
                ..Default::default()
            },
        );
    }

    let output = f.niri_output(1);
    let map = layer_map_for_output(&output);
    for (namespace, _, _, (x, y, width, height)) in cases {
        let layer = map
            .layers()
            .find(|layer| layer.namespace() == namespace)
            .unwrap();
        assert_eq!(
            map.layer_geometry(layer).unwrap(),
            Rectangle::new((x, y).into(), (width, height).into()),
            "wrong geometry for {namespace}"
        );
    }
}

#[test]
fn exclusive_zone_is_released_when_a_layer_unmaps() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let surface = map_layer(
        &mut f,
        client,
        Layer::Top,
        "bar",
        LayerConfigureProps {
            anchor: Some(Anchor::Top | Anchor::Left | Anchor::Right),
            size: Some((0, 30)),
            exclusive_zone: Some(30),
            ..Default::default()
        },
    );
    assert_eq!(
        f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .working_area(),
        Rectangle::new((0., 30.).into(), (800., 570.).into())
    );

    let layer = f.client(client).layer(&surface);
    layer.attach_null();
    layer.commit();
    f.double_roundtrip(client);

    assert_eq!(
        f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .working_area(),
        Rectangle::new((0., 0.).into(), (800., 600.).into())
    );
}

#[test]
fn reconfiguring_a_dock_without_changing_its_zone_preserves_tiled_geometry() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    let client = f.add_client();
    let surface = map_layer(
        &mut f,
        client,
        Layer::Top,
        "dock",
        LayerConfigureProps {
            anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
            size: Some((0, 30)),
            exclusive_zone: Some(30),
            ..Default::default()
        },
    );
    let window = f.client(client).create_window();
    window.commit();
    let window_surface = window.surface.clone();
    f.double_roundtrip(client);
    let window = f.client(client).window(&window_surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    let _ = f
        .client(client)
        .window(&window_surface)
        .recent_configures()
        .count();

    let before = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .working_area();

    let layer = f.client(client).layer(&surface);
    layer.set_configure_props(LayerConfigureProps {
        size: Some((0, 40)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(client);

    let after = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .working_area();
    assert_eq!(after, before);
    assert_eq!(
        f.client(client)
            .window(&window_surface)
            .recent_configures()
            .count(),
        0
    );

    let layer = f.client(client).layer(&surface);
    layer.set_configure_props(LayerConfigureProps {
        exclusive_zone: Some(40),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(client);
    let changed = f
        .swayward()
        .layout
        .active_workspace()
        .unwrap()
        .working_area();
    assert_eq!(changed.loc.y, 40.);
    assert_eq!(changed.size.h, 760.);
    assert!(f
        .client(client)
        .window(&window_surface)
        .recent_configures()
        .next()
        .is_some());
}

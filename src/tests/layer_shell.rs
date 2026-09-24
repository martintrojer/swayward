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
fn floating_windows_do_not_overlap_exclusive_layers() {
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

#[test]
fn criteria_do_not_match_layer_surfaces() {
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

    let outcome = crate::command::execute(f.niri_state(), r#"[title="dock"] kill"#);
    assert!(!outcome[0].success);
    assert_eq!(outcome[0].error.as_deref(), Some("No matching node."));
    assert!(!f.client(client).layer(&surface).close_requested);
}

#[test]
fn layer_surface_size_and_exclusive_zone_reconfigure_without_moving_outputs() {
    let mut f = Fixture::new();
    f.add_output(1, (1280, 800));
    f.add_output(2, (1024, 768));
    let client = f.add_client();
    let output = f.niri_output(1);
    f.roundtrip(client);
    let wl_output = f.client(client).output(&output.name());
    let layer = f
        .client(client)
        .create_layer(Some(&wl_output), Layer::Top, "dock");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 30)),
        exclusive_zone: Some(30),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(client);
    let layer = f.client(client).layer(&surface);
    let size = layer.configures_received.last().unwrap().1.size;
    layer.attach_new_buffer();
    layer.set_size(size.0 as u16, size.1 as u16);
    layer.ack_last_and_commit();
    f.double_roundtrip(client);

    let map = layer_map_for_output(&output);
    let layer = map
        .layers()
        .find(|layer| layer.namespace() == "dock")
        .unwrap();
    assert_eq!(map.layer_geometry(layer).unwrap().size, (1280, 30).into());
    drop(map);
    assert_eq!(
        f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .working_area(),
        Rectangle::new((0., 30.).into(), (1280., 770.).into())
    );

    let layer = f.client(client).layer(&surface);
    layer.set_configure_props(LayerConfigureProps {
        size: Some((0, 40)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(client);
    let layer = f.client(client).layer(&surface);
    let size = layer.configures_received.last().unwrap().1.size;
    layer.set_size(size.0 as u16, size.1 as u16);
    layer.ack_last_and_commit();
    f.double_roundtrip(client);

    let map = layer_map_for_output(&output);
    let layer = map
        .layers()
        .find(|layer| layer.namespace() == "dock")
        .unwrap();
    assert_eq!(map.layer_geometry(layer).unwrap().size, (1280, 40).into());
    drop(map);
    assert_eq!(
        f.swayward()
            .layout
            .active_workspace()
            .unwrap()
            .working_area(),
        Rectangle::new((0., 30.).into(), (1280., 770.).into())
    );

    drop(wl_output);
    let second = f.niri_output(2);
    assert!(layer_map_for_output(&second)
        .layers()
        .all(|layer| layer.namespace() != "dock"));
}

#[test]
fn keyboard_interactivity_controls_focus_and_unmap_restores_window_focus() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();

    let window = f.client(client).create_window();
    window.commit();
    let window_surface = window.surface.clone();
    f.double_roundtrip(client);
    let window = f.client(client).window(&window_surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    assert_eq!(
        focused_surface_id(&mut f),
        window_surface.id().protocol_id()
    );

    let none = map_layer(
        &mut f,
        client,
        Layer::Overlay,
        "none",
        LayerConfigureProps {
            size: Some((100, 50)),
            kb_interactivity: Some(KeyboardInteractivity::None),
            ..Default::default()
        },
    );
    assert_eq!(
        focused_surface_id(&mut f),
        window_surface.id().protocol_id()
    );

    let on_demand = map_layer(
        &mut f,
        client,
        Layer::Overlay,
        "on-demand",
        LayerConfigureProps {
            size: Some((100, 50)),
            kb_interactivity: Some(KeyboardInteractivity::OnDemand),
            ..Default::default()
        },
    );
    assert_eq!(focused_surface_id(&mut f), on_demand.id().protocol_id());

    let exclusive = map_layer(
        &mut f,
        client,
        Layer::Overlay,
        "exclusive",
        LayerConfigureProps {
            size: Some((100, 50)),
            kb_interactivity: Some(KeyboardInteractivity::Exclusive),
            ..Default::default()
        },
    );
    assert_eq!(focused_surface_id(&mut f), exclusive.id().protocol_id());

    let layer = f.client(client).layer(&exclusive);
    layer.attach_null();
    layer.commit();
    f.double_roundtrip(client);
    assert_eq!(focused_surface_id(&mut f), on_demand.id().protocol_id());

    let layer = f.client(client).layer(&on_demand);
    layer.attach_null();
    layer.commit();
    f.double_roundtrip(client);
    assert_eq!(
        focused_surface_id(&mut f),
        window_surface.id().protocol_id()
    );
    assert_ne!(focused_surface_id(&mut f), none.id().protocol_id());
}

#[test]
fn layers_are_arranged_in_sway_order_and_runtime_changes_restack() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let background = map_layer(
        &mut f,
        client,
        Layer::Background,
        "background",
        LayerConfigureProps {
            size: Some((100, 50)),
            ..Default::default()
        },
    );
    for (layer, namespace) in [
        (Layer::Overlay, "overlay"),
        (Layer::Bottom, "bottom"),
        (Layer::Top, "top"),
    ] {
        map_layer(
            &mut f,
            client,
            layer,
            namespace,
            LayerConfigureProps {
                size: Some((100, 50)),
                ..Default::default()
            },
        );
    }

    let output = f.niri_output(1);
    let namespaces = layer_map_for_output(&output)
        .layers()
        .map(|layer| layer.namespace().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(namespaces, ["overlay", "top", "bottom", "background"]);

    let layer = f.client(client).layer(&background);
    layer.set_configure_props(LayerConfigureProps {
        layer: Some(Layer::Overlay),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(client);
    let output = f.niri_output(1);
    let namespaces = layer_map_for_output(&output)
        .layers()
        .map(|layer| layer.namespace().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(namespaces, ["overlay", "background", "top", "bottom"]);
}

#[test]
fn requests_after_layer_surfaces_output_is_removed_are_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    f.add_output(2, (1024, 768));
    let client = f.add_client();
    f.double_roundtrip(client);

    let wl_output = f.client(client).output("headless-2");
    let layer = f
        .client(client)
        .create_layer(Some(&wl_output), Layer::Top, "removed-output-layer");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 30)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(client);

    let removed = f.niri_output(2);
    f.swayward().remove_output(&removed);
    f.double_roundtrip(client);
    assert!(f.client(client).layer(&surface).close_requested);

    let layer = f.client(client).layer(&surface);
    layer.set_configure_props(LayerConfigureProps {
        size: Some((0, 40)),
        exclusive_zone: Some(40),
        layer: Some(Layer::Overlay),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(client);

    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn layer_popup_is_constrained_and_parent_unmap_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (800, 600));
    let client = f.add_client();
    let parent_surface = map_layer(
        &mut f,
        client,
        Layer::Overlay,
        "popup-parent",
        LayerConfigureProps {
            anchor: Some(Anchor::Top | Anchor::Right | Anchor::Bottom | Anchor::Left),
            size: Some((0, 0)),
            ..Default::default()
        },
    );

    let parent = f
        .client(client)
        .layer(&parent_surface)
        .layer_surface
        .clone();
    let popup = f.client(client).create_layer_popup(&parent, (0, 0));
    let popup_surface = popup.surface.clone();
    popup.surface.commit();
    f.double_roundtrip(client);
    assert_eq!(
        f.client(client)
            .popup(&popup_surface)
            .configures_received
            .last(),
        Some(&(0, 0, 100, 100))
    );

    let popup = f.client(client).popup(&popup_surface).xdg_popup.clone();
    f.client(client)
        .state
        .reposition_popup_at(&popup, 42, Some((800, 600)));
    f.client(client).connection.flush().unwrap();
    f.double_roundtrip(client);
    let popup = f.client(client).popup(&popup_surface);
    assert_eq!(popup.repositioned, [42]);
    assert_eq!(
        popup.configures_received.last(),
        Some(&(700, 500, 100, 100))
    );

    let parent = f.client(client).layer(&parent_surface);
    parent.attach_null();
    parent.commit();
    f.double_roundtrip(client);

    let popup = f.client(client).popup(&popup_surface).xdg_popup.clone();
    f.client(client)
        .state
        .reposition_popup_at(&popup, 43, Some((750, 550)));
    f.client(client).connection.flush().unwrap();
    f.dispatch();
    f.client(client).dispatch_unchecked();
    assert!(f.client(client).connection.protocol_error().is_none());
}

#[test]
fn simple_top_anchor() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    let layer = f.client(id).create_layer(None, Layer::Top, "");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 50)),
        ..Default::default()
    });
    layer.commit();
    f.roundtrip(id);

    let layer = f.client(id).layer(&surface);
    layer.attach_new_buffer();
    layer.set_size(100, 100);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    assert_snapshot!(layer.format_recent_configures(), @"size: 1920 × 50");
}

#[test]
fn margin_overflow() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    let layer = f.client(id).create_layer(None, Layer::Top, "");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top | Anchor::Bottom),
        margin: Some(LayerMargin {
            top: i32::MAX,
            right: i32::MAX,
            bottom: i32::MAX,
            left: i32::MAX,
        }),
        exclusive_zone: Some(i32::MAX),
        ..Default::default()
    });
    layer.commit();
    f.roundtrip(id);

    let layer = f.client(id).layer(&surface);
    layer.attach_new_buffer();
    layer.set_size(100, 100);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    assert_snapshot!(layer.format_recent_configures(), @"size: 0 × 0");

    // Add a second one for good measure.
    let layer = f.client(id).create_layer(None, Layer::Top, "");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top | Anchor::Bottom),
        margin: Some(LayerMargin {
            top: i32::MAX,
            right: i32::MAX,
            bottom: i32::MAX,
            left: i32::MAX,
        }),
        exclusive_zone: Some(i32::MAX),
        ..Default::default()
    });
    layer.commit();
    f.roundtrip(id);

    let layer = f.client(id).layer(&surface);
    layer.attach_new_buffer();
    layer.set_size(100, 100);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    assert_snapshot!(layer.format_recent_configures(), @"size: 0 × 0");
}

#[test]
fn unmap_through_null_buffer() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    let layer = f.client(id).create_layer(None, Layer::Top, "");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 50)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    assert_snapshot!(layer.format_recent_configures(), @"size: 1920 × 50");

    layer.attach_new_buffer();
    layer.set_size(100, 100);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // No new configure since nothing changed.
    assert_snapshot!(layer.format_recent_configures(), @"");

    // Unmap by attaching a null buffer. This moves the surface back to pre-initial-commit stage.
    layer.attach_null();
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // Configures must be empty because we haven't done an initial commit yet.
    assert_snapshot!(layer.format_recent_configures(), @"");

    // Do the initial commit again.
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 100)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // This is the new initial configure.
    assert_snapshot!(layer.format_recent_configures(), @"size: 1920 × 100");

    layer.attach_new_buffer();
    layer.set_size(100, 100);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    assert_snapshot!(layer.format_recent_configures(), @"");
}

#[test]
fn multiple_commits_before_mapping() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();

    let layer = f.client(id).create_layer(None, Layer::Top, "");
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 50)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    assert_snapshot!(layer.format_recent_configures(), @"size: 1920 × 50");

    // Change something that won't cause a configure.
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 50)),
        kb_interactivity: Some(KeyboardInteractivity::OnDemand),
        ..Default::default()
    });
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // No new configure since the size hasn't changed.
    assert_snapshot!(layer.format_recent_configures(), @"");

    // Change something that will cause a configure.
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 100)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // Configure with new size.
    assert_snapshot!(layer.format_recent_configures(), @"size: 1920 × 100");

    // Map.
    layer.attach_new_buffer();
    layer.set_size(100, 100);
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // No new configure since nothing changed.
    assert_snapshot!(layer.format_recent_configures(), @"");

    // Unmap by attaching a null buffer. This moves the surface back to pre-initial-commit stage.
    layer.attach_null();
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // Configures must be empty because we haven't done an initial commit yet.
    assert_snapshot!(layer.format_recent_configures(), @"");

    // Same configure props as before, but since we unmapped, we should get a new initial
    // configure (that will happen to match the previous configure we had got while mapped).
    let surface = layer.surface.clone();
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 100)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    assert_snapshot!(layer.format_recent_configures(), @"size: 1920 × 100");

    // Change something that won't cause a configure.
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 100)),
        kb_interactivity: Some(KeyboardInteractivity::OnDemand),
        ..Default::default()
    });
    layer.ack_last_and_commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // No new configure since the size hasn't changed.
    assert_snapshot!(layer.format_recent_configures(), @"");

    // Change something that will cause a configure.
    layer.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
        size: Some((0, 50)),
        ..Default::default()
    });
    layer.commit();
    f.double_roundtrip(id);

    let layer = f.client(id).layer(&surface);
    // Configure with new size.
    assert_snapshot!(layer.format_recent_configures(), @"size: 1920 × 50");
}

#[test]
fn workspace_rect_excludes_exclusive_zones() {
    // Sway reports a workspace's rect as the usable area, not the whole
    // output: its own GET_TREE example has the output at y=0 h=1080 and the
    // workspace at y=23 h=1057 under a 23px bar (sway/sway-ipc.7.scd).
    // Scripts size floating windows from this rect, so reporting the full
    // output places them under the bar.
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
            anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
            size: Some((0, 30)),
            exclusive_zone: Some(30),
            ..Default::default()
        },
    );

    let swayward = f.swayward();
    let tree = crate::ipc::tree::describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let tree = serde_json::to_value(&tree).unwrap();

    fn find<'a>(node: &'a serde_json::Value, kind: &str) -> Option<&'a serde_json::Value> {
        if node["type"] == kind && node["name"] != "__i3" && node["name"] != "__i3_scratch" {
            return Some(node);
        }
        for key in ["nodes", "floating_nodes"] {
            for child in node[key].as_array().into_iter().flatten() {
                if let Some(found) = find(child, kind) {
                    return Some(found);
                }
            }
        }
        None
    }

    let output = find(&tree, "output").expect("an output node");
    let workspace = find(&tree, "workspace").expect("a workspace node");
    assert_eq!(output["rect"]["y"], 0, "output keeps the full rect");
    assert_eq!(output["rect"]["height"], 600);
    assert_eq!(
        workspace["rect"]["y"], 30,
        "workspace rect must start below the bar"
    );
    assert_eq!(
        workspace["rect"]["height"], 570,
        "workspace rect must exclude the bar"
    );
}

#[test]
fn a_full_height_floating_window_stays_below_the_bar() {
    // A window taller than the usable area makes the upper bound smaller than
    // the lower one. Applying the maximum last then dragged the window back
    // over the top bar, which is what `move absolute position` from a
    // preset-size script produced. Bars on both edges are needed to reach it:
    // with only a top bar the upper bound stays comfortably large.
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
            anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
            size: Some((0, 30)),
            exclusive_zone: Some(30),
            ..Default::default()
        },
    );
    map_layer(
        &mut f,
        client,
        Layer::Top,
        "dock",
        LayerConfigureProps {
            anchor: Some(Anchor::Left | Anchor::Right | Anchor::Bottom),
            size: Some((0, 30)),
            exclusive_zone: Some(30),
            ..Default::default()
        },
    );

    let window = f.client(client).create_window();
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);
    f.swayward().layout.toggle_window_floating(None);
    f.double_roundtrip(client);
    let window = f.client(client).window(&surface);
    window.set_size(800, 600);
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    f.swayward().layout.move_floating_window(
        None,
        swayward_ipc::PositionChange::SetFixed(0.),
        swayward_ipc::PositionChange::SetFixed(0.),
        false,
    );

    let workspace = f.swayward().layout.active_workspace().unwrap();
    let (_, pos, _) = workspace.tiles_with_render_positions().next().unwrap();
    assert!(
        pos.y >= 30.,
        "a full-height floating window was pulled back over the bar at {pos:?}"
    );
}

#[test]
fn opening_the_overview_takes_focus_from_an_on_demand_layer() {
    // compute_focus checks Layer::Top before the overview, so a bar holding
    // on-demand keyboard focus swallowed every key once the overview opened:
    // no bind fired at all, while the mouse still worked. Clicking a waybar
    // module is enough to grant that focus.
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
            anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top),
            size: Some((0, 30)),
            exclusive_zone: Some(30),
            kb_interactivity: Some(KeyboardInteractivity::OnDemand),
            ..Default::default()
        },
    );

    // Grant the bar on-demand focus, as clicking one of its modules does.
    let mapped = f
        .swayward()
        .mapped_layer_surfaces
        .keys()
        .next()
        .cloned()
        .expect("the bar is mapped");
    f.swayward().layer_shell_on_demand_focus = Some(mapped);
    f.niri_state().update_keyboard_focus();
    assert!(
        matches!(
            f.swayward().keyboard_focus,
            crate::swayward::KeyboardFocus::LayerShell { .. }
        ),
        "the bar should hold focus before the overview opens, got {:?}",
        f.swayward().keyboard_focus
    );

    f.niri_state().handle_bind(swayward_config::Bind {
        key: swayward_config::Key {
            trigger: swayward_config::Trigger::Keysym(smithay::input::keyboard::Keysym::o),
            modifiers: swayward_config::Modifiers::COMPOSITOR,
        },
        action: swayward_config::Action::ToggleOverview,
        mouse_regions: swayward_config::MouseRegions::empty(),
        input_device: "*".into(),
        group: None,
        release: false,
        repeat: false,
        cooldown: None,
        allow_when_locked: false,
        allow_inhibiting: true,
        hotkey_overlay_title: None,
    });
    f.niri_state().update_keyboard_focus();

    assert!(
        f.swayward().keyboard_focus.is_overview(),
        "the overview must take focus from the bar, got {:?}",
        f.swayward().keyboard_focus
    );
}

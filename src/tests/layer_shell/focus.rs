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


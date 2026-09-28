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

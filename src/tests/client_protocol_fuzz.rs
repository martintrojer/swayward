use proptest::prelude::*;

use super::Fixture;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expected {
    Valid,
    ProtocolError { interface: &'static str, code: u32 },
}

#[derive(Clone, Debug)]
enum Op {
    Create,
    SetTitle(u8),
    SetAppId(u8),
    SetMinSize(i16, i16),
    SetMaxSize(i16, i16),
    SetParent(u8),
    ClearParent,
    SetFullscreen,
    UnsetFullscreen,
    SetMaximized,
    UnsetMaximized,
    InitialCommit,
    AckAndMap,
    Unmap,
    DestroyRole,
    CreatePopup,
    RepositionPopup(u8),
    DestroyPopup,
    CreateLayer(u8),
    ConfigureLayer(u8, i16),
    MapLayer,
    UnmapLayer,
    DestroyLayer,
    CreateSubsurface,
    MoveSubsurface(i16, i16),
    DestroySubsurface,
    InvalidAck,
    InvalidDestroySurfaceFirst,
    Command(&'static str),
}

impl Op {
    fn expected(&self) -> Expected {
        match self {
            Self::InvalidAck => Expected::ProtocolError {
                interface: "xdg_wm_base",
                code: 4,
            },
            Self::InvalidDestroySurfaceFirst => Expected::ProtocolError {
                interface: "xdg_wm_base",
                code: 0,
            },
            _ => Expected::Valid,
        }
    }
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Create),
        any::<u8>().prop_map(Op::SetTitle),
        any::<u8>().prop_map(Op::SetAppId),
        (0..=i16::MAX, 0..=i16::MAX).prop_map(|(w, h)| Op::SetMinSize(w, h)),
        (0..=i16::MAX, 0..=i16::MAX).prop_map(|(w, h)| Op::SetMaxSize(w, h)),
        any::<u8>().prop_map(Op::SetParent),
        Just(Op::ClearParent),
        Just(Op::SetFullscreen),
        Just(Op::UnsetFullscreen),
        Just(Op::SetMaximized),
        Just(Op::UnsetMaximized),
        Just(Op::InitialCommit),
        Just(Op::AckAndMap),
        Just(Op::Unmap),
        Just(Op::DestroyRole),
        Just(Op::CreatePopup),
        any::<u8>().prop_map(Op::RepositionPopup),
        Just(Op::DestroyPopup),
        any::<u8>().prop_map(Op::CreateLayer),
        (any::<u8>(), any::<i16>()).prop_map(|(anchor, zone)| Op::ConfigureLayer(anchor, zone)),
        Just(Op::MapLayer),
        Just(Op::UnmapLayer),
        Just(Op::DestroyLayer),
        Just(Op::CreateSubsurface),
        (any::<i16>(), any::<i16>()).prop_map(|(x, y)| Op::MoveSubsurface(x, y)),
        Just(Op::DestroySubsurface),
        Just(Op::InvalidAck),
        Just(Op::InvalidDestroySurfaceFirst),
        prop::sample::select(
            &[
                "focus left",
                "focus right",
                "move left",
                "move right",
                "floating toggle",
                "fullscreen toggle",
                "kill",
            ][..]
        )
        .prop_map(Op::Command),
    ]
}

fn check_ops(ops: Vec<Op>) {
    let mut fixture = Fixture::new();
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    let mut layer_mapped = false;

    for op in ops {
        let expected = op.expected();
        match op {
            Op::Create => {
                fixture.client(client).create_window();
            }
            Op::SetTitle(value) => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.set_title(&format!("title-{value}"));
                }
            }
            Op::SetAppId(value) => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.xdg_toplevel.set_app_id(format!("app-{value}"));
                }
            }
            Op::SetMinSize(width, height) => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.set_min_size(i32::from(width), i32::from(height));
                }
            }
            Op::SetMaxSize(width, height) => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.set_max_size(i32::from(width), i32::from(height));
                }
            }
            Op::SetParent(parent) => {
                let state = &fixture.client(client).state;
                if state.windows.len() > 1 {
                    let parent = usize::from(parent) % (state.windows.len() - 1);
                    let toplevel = state.windows[parent].xdg_toplevel.clone();
                    state.windows.last().unwrap().set_parent(Some(&toplevel));
                }
            }
            Op::ClearParent => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.set_parent(None);
                }
            }
            Op::SetFullscreen => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.set_fullscreen(None);
                }
            }
            Op::UnsetFullscreen => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.unset_fullscreen();
                }
            }
            Op::SetMaximized => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.set_maximized();
                }
            }
            Op::UnsetMaximized => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.unset_maximized();
                }
            }
            Op::InitialCommit => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.commit();
                }
            }
            Op::AckAndMap => {
                if let Some(window) = fixture.client(client).state.windows.last_mut() {
                    if window
                        .configures_received
                        .last()
                        .is_some_and(|(serial, _)| Some(*serial) != window.last_acked_configure)
                    {
                        window.attach_new_buffer();
                        window.ack_last_and_commit();
                    }
                }
            }
            Op::Unmap => {
                if let Some(window) = fixture.client(client).state.windows.last() {
                    window.attach_null();
                    window.commit();
                }
            }
            Op::DestroyRole => {
                if let Some(window) = fixture.client(client).state.windows.pop() {
                    window.destroy_role();
                }
            }
            Op::CreatePopup => {
                let parent = fixture
                    .client(client)
                    .state
                    .windows
                    .last()
                    .map(|window| window.xdg_surface.clone());
                if let Some(parent) = parent {
                    let popup = fixture.client(client).create_popup(&parent);
                    popup.surface.commit();
                }
            }
            Op::RepositionPopup(token) => {
                let popup = fixture
                    .client(client)
                    .state
                    .popups
                    .last()
                    .map(|popup| popup.xdg_popup.clone());
                if let Some(popup) = popup {
                    fixture
                        .client(client)
                        .state
                        .reposition_popup(&popup, u32::from(token));
                }
            }
            Op::DestroyPopup => {
                if let Some(popup) = fixture.client(client).state.popups.pop() {
                    popup.xdg_popup.destroy();
                    popup.xdg_surface.destroy();
                }
            }
            Op::CreateLayer(layer) => {
                use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
                if fixture.client(client).state.layers.is_empty() {
                    let layer = match layer % 4 {
                        0 => Layer::Background,
                        1 => Layer::Bottom,
                        2 => Layer::Top,
                        _ => Layer::Overlay,
                    };
                    let layer = fixture.client(client).create_layer(None, layer, "fuzz");
                    layer.set_configure_props(super::client::LayerConfigureProps {
                        size: Some((100, 100)),
                        ..Default::default()
                    });
                    layer_mapped = false;
                }
            }
            Op::ConfigureLayer(anchor, zone) => {
                use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity};
                if let Some(layer) = fixture.client(client).state.layers.last() {
                    let anchors = [
                        Anchor::empty(),
                        Anchor::Top,
                        Anchor::Bottom,
                        Anchor::Left | Anchor::Right | Anchor::Top,
                    ];
                    layer.set_configure_props(super::client::LayerConfigureProps {
                        anchor: Some(anchors[usize::from(anchor) % anchors.len()]),
                        exclusive_zone: Some(i32::from(zone)),
                        kb_interactivity: Some(if anchor % 2 == 0 {
                            KeyboardInteractivity::None
                        } else {
                            KeyboardInteractivity::Exclusive
                        }),
                        ..Default::default()
                    });
                }
            }
            Op::MapLayer => {
                if let Some(layer) = fixture.client(client).state.layers.last_mut() {
                    if !layer_mapped && layer.configures_received.len() > layer.configures_looked_at
                    {
                        layer.attach_new_buffer();
                        layer.ack_last_and_commit();
                        layer.configures_looked_at = layer.configures_received.len();
                        layer_mapped = true;
                    } else if layer.configures_received.is_empty() {
                        layer.commit();
                    }
                }
            }
            Op::UnmapLayer => {
                if layer_mapped {
                    if let Some(layer) = fixture.client(client).state.layers.last_mut() {
                        layer.attach_null();
                        if layer.configures_received.len() > layer.configures_looked_at {
                            layer.ack_last();
                            layer.configures_looked_at = layer.configures_received.len();
                        }
                        layer.commit();
                        layer_mapped = false;
                    }
                }
            }
            Op::DestroyLayer => {
                if let Some(layer) = fixture.client(client).state.layers.pop() {
                    layer.layer_surface.destroy();
                    layer.surface.destroy();
                    layer_mapped = false;
                }
            }
            Op::CreateSubsurface => {
                let parent = fixture
                    .client(client)
                    .state
                    .windows
                    .last()
                    .map(|window| window.surface.clone());
                if let Some(parent) = parent {
                    fixture.client(client).state.create_subsurface(&parent);
                }
            }
            Op::MoveSubsurface(x, y) => {
                if let Some((surface, subsurface)) = fixture.client(client).state.subsurfaces.last()
                {
                    subsurface.set_position(i32::from(x), i32::from(y));
                    surface.commit();
                }
            }
            Op::DestroySubsurface => {
                if let Some((surface, subsurface)) = fixture.client(client).state.subsurfaces.pop()
                {
                    subsurface.destroy();
                    surface.destroy();
                }
            }
            Op::InvalidAck => {
                if fixture.client(client).state.windows.is_empty() {
                    fixture.client(client).create_window();
                }
                let window = fixture.client(client).state.windows.last_mut().unwrap();
                window.ack_configure(u32::MAX);
                window.commit();
            }
            Op::InvalidDestroySurfaceFirst => {
                if fixture.client(client).state.windows.is_empty() {
                    fixture.client(client).create_window();
                }
                fixture
                    .client(client)
                    .state
                    .windows
                    .last()
                    .unwrap()
                    .xdg_surface
                    .destroy();
            }
            Op::Command(command) => {
                let _ = crate::command::execute(fixture.niri_state(), command);
            }
        }
        if fixture.client(client).connection.flush().is_err() {
            continue;
        }
        fixture.dispatch();
        fixture.client(client).dispatch_unchecked();
        let error = fixture.client(client).connection.protocol_error();
        match expected {
            Expected::Valid => {
                assert!(
                    error.is_none(),
                    "valid operation disconnected client: {error:?}"
                );
                fixture.swayward().layout.verify_invariants();
            }
            Expected::ProtocolError { interface, code } => {
                let error = error.expect("invalid operation was silently accepted");
                assert_eq!(
                    error.object_interface, interface,
                    "wrong protocol error: {error:?}"
                );
                assert_eq!(error.code, code, "wrong protocol error: {error:?}");
                return;
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_none() {
            0
        } else {
            ProptestConfig::default().cases
        },
        max_shrink_iters: 10_000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_wayland_client_lifecycles_preserve_compositor_invariants(
        ops in prop::collection::vec(op(), 1..80),
    ) {
        check_ops(ops);
    }
}

#[test]
fn client_unfullscreen_of_mapped_child_after_fullscreen_toggle() {
    check_ops(vec![
        Op::Create,
        Op::Create,
        Op::SetParent(0),
        Op::Unmap,
        Op::AckAndMap,
        Op::Command("fullscreen toggle"),
        Op::UnsetFullscreen,
    ]);
}

#[test]
fn client_unfullscreen_after_unset_maximized_and_fullscreen_toggle() {
    check_ops(vec![
        Op::Create,
        Op::Create,
        Op::Create,
        Op::InitialCommit,
        Op::SetParent(0),
        Op::AckAndMap,
        Op::UnsetFullscreen,
        Op::UnsetMaximized,
        Op::Command("fullscreen toggle"),
        Op::UnsetFullscreen,
    ]);
}

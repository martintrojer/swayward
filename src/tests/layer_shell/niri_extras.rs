/// Sway has no overview. This test protects swayward's retained niri-extra
/// overview from a layer surface holding on-demand keyboard focus.
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

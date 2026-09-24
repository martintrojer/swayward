use wayland_client::protocol::wl_output::Transform;

use super::client::OutputConfigurationResult;
use super::Fixture;

#[test]
fn test_does_not_apply_output_changes() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.niri_state().refresh_ipc_outputs();
    let client = f.add_client();
    f.roundtrip(client);

    let (manager, serial, head) = {
        let state = &f.client(client).state;
        (
            state.output_manager.clone().unwrap(),
            *state.output_manager_serials.last().unwrap(),
            state.output_heads[0].proxy.clone(),
        )
    };
    let qh = f.client(client).qh.clone();
    let config = manager.create_configuration(serial, &qh, ());
    config.disable_head(&head);
    config.test();
    f.roundtrip(client);

    assert_eq!(
        f.client(client).state.output_configuration_results,
        [OutputConfigurationResult::Succeeded]
    );
    assert!(f.swayward().config.borrow().outputs.0.is_empty());
}

#[test]
fn apply_can_disable_the_last_output() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.niri_state().refresh_ipc_outputs();
    let client = f.add_client();
    f.roundtrip(client);

    let (manager, serial, head) = {
        let state = &f.client(client).state;
        (
            state.output_manager.clone().unwrap(),
            *state.output_manager_serials.last().unwrap(),
            state.output_heads[0].proxy.clone(),
        )
    };
    let qh = f.client(client).qh.clone();
    let config = manager.create_configuration(serial, &qh, ());
    config.disable_head(&head);
    config.apply();
    f.roundtrip(client);

    assert_eq!(
        f.client(client).state.output_configuration_results,
        [OutputConfigurationResult::Succeeded]
    );
    assert!(f.swayward().config.borrow().outputs.0[0].off);
}

#[test]
fn applying_an_outdated_configuration_is_cancelled() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.niri_state().refresh_ipc_outputs();
    let client = f.add_client();
    f.roundtrip(client);

    let (manager, serial, head) = {
        let state = &f.client(client).state;
        (
            state.output_manager.clone().unwrap(),
            *state.output_manager_serials.last().unwrap(),
            state.output_heads[0].proxy.clone(),
        )
    };
    let qh = f.client(client).qh.clone();
    let config = manager.create_configuration(serial, &qh, ());
    config.disable_head(&head);
    f.roundtrip(client);
    let output = f.niri_output(1);
    f.swayward().remove_output(&output);
    f.niri_state().backend.headless().retain_ipc_outputs(&[]);
    f.swayward().ipc_outputs_changed = true;
    f.niri_state().refresh_ipc_outputs();
    f.roundtrip(client);
    assert_eq!(
        f.client(client).state.output_configuration_results,
        [OutputConfigurationResult::Cancelled]
    );

    config.apply();
    f.roundtrip(client);
    assert_eq!(
        f.client(client).state.output_configuration_results,
        [OutputConfigurationResult::Cancelled]
    );
}

#[test]
fn removed_head_cannot_be_enabled_by_a_new_configuration() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.niri_state().refresh_ipc_outputs();
    let client = f.add_client();
    f.roundtrip(client);

    let (manager, head) = {
        let state = &f.client(client).state;
        (
            state.output_manager.clone().unwrap(),
            state.output_heads[0].proxy.clone(),
        )
    };
    let output = f.niri_output(1);
    f.swayward().remove_output(&output);
    f.niri_state().backend.headless().retain_ipc_outputs(&[]);
    f.swayward().ipc_outputs_changed = true;
    f.niri_state().refresh_ipc_outputs();
    f.roundtrip(client);

    let serial = *f
        .client(client)
        .state
        .output_manager_serials
        .last()
        .unwrap();
    let qh = f.client(client).qh.clone();
    let config = manager.create_configuration(serial, &qh, ());
    config.enable_head(&head, &qh, ());
    config.apply();
    f.double_roundtrip(client);

    assert_eq!(
        f.client(client).state.output_configuration_results,
        [OutputConfigurationResult::Succeeded]
    );
    assert!(f.swayward().config.borrow().outputs.0.is_empty());
}

#[test]
fn removed_mode_is_rejected_before_it_can_configure_a_head() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    {
        let outputs = f.niri_state().backend.headless().ipc_outputs();
        let mut outputs = outputs.lock().unwrap();
        outputs
            .values_mut()
            .next()
            .unwrap()
            .modes
            .push(swayward_ipc::Mode {
                width: 1280,
                height: 720,
                refresh_rate: 60_000,
                is_preferred: false,
            });
    }
    f.niri_state().refresh_ipc_outputs();
    let client = f.add_client();
    f.roundtrip(client);

    let (manager, head, removed_mode) = {
        let state = &f.client(client).state;
        (
            state.output_manager.clone().unwrap(),
            state.output_heads[0].proxy.clone(),
            state.output_heads[0].modes[1].clone(),
        )
    };
    let outputs = f.niri_state().backend.headless().ipc_outputs();
    let mut outputs = outputs.lock().unwrap();
    let output = outputs.values_mut().next().unwrap();
    output.modes = vec![swayward_ipc::Mode {
        width: 1920,
        height: 1080,
        refresh_rate: 60_000,
        is_preferred: true,
    }];
    output.current_mode = Some(0);
    drop(outputs);
    f.swayward().ipc_outputs_changed = true;
    f.niri_state().refresh_ipc_outputs();
    f.roundtrip(client);

    let serial = *f
        .client(client)
        .state
        .output_manager_serials
        .last()
        .unwrap();
    let qh = f.client(client).qh.clone();
    let config = manager.create_configuration(serial, &qh, ());
    let config_head = config.enable_head(&head, &qh, ());
    config_head.set_mode(&removed_mode);
    f.client(client).connection.flush().unwrap();
    f.dispatch();
    f.client(client).dispatch_unchecked();

    assert!(f.client(client).connection.protocol_error().is_some());
    assert!(f.swayward().config.borrow().outputs.0.is_empty());
}

#[test]
fn apply_uses_the_persistent_output_config_path() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    f.niri_state().refresh_ipc_outputs();
    let client = f.add_client();
    f.roundtrip(client);

    let (manager, serial, head, mode) = {
        let state = &f.client(client).state;
        (
            state.output_manager.clone().unwrap(),
            *state.output_manager_serials.last().unwrap(),
            state.output_heads[0].proxy.clone(),
            state.output_heads[0].modes[0].clone(),
        )
    };
    let qh = f.client(client).qh.clone();
    let config = manager.create_configuration(serial, &qh, ());
    let head_config = config.enable_head(&head, &qh, ());
    head_config.set_mode(&mode);
    head_config.set_scale(1.5);
    head_config.set_transform(Transform::_90);
    head_config.set_position(200, 300);
    config.apply();
    f.roundtrip(client);

    assert_eq!(
        f.client(client).state.output_configuration_results,
        [OutputConfigurationResult::Succeeded]
    );
    {
        let config = f.swayward().config.borrow();
        let output = &config.outputs.0[0];
        assert_eq!(output.name, "headless-1");
        assert_eq!(output.scale.unwrap().0, 1.5);
        assert_eq!(output.transform, swayward_ipc::Transform::_90);
        assert_eq!(
            output.position,
            Some(swayward_config::Position { x: 200, y: 300 })
        );
        assert_eq!(output.mode.as_ref().unwrap().mode.width, 1920);
        assert_eq!(output.mode.as_ref().unwrap().mode.height, 1080);
    }

    let smithay_output = f.niri_output(1);
    assert_eq!(smithay_output.current_scale().fractional_scale(), 1.5);
    assert_eq!(
        smithay_output.current_transform(),
        smithay::utils::Transform::_90
    );
}

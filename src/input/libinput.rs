use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum PointerSetting {
    SendEvents(input::SendEventsMode),
    NaturalScroll(bool),
    AccelSpeed(f64),
    LeftHanded(bool),
    MiddleEmulation(bool),
    AccelProfile(input::AccelProfile),
    ScrollMethod(input::ScrollMethod),
    ScrollButton(u32),
    ScrollButtonLock(input::ScrollButtonLockState),
}

impl PointerSetting {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::SendEvents(_) => "send-events",
            Self::NaturalScroll(_) => "natural-scroll",
            Self::AccelSpeed(_) => "accel-speed",
            Self::LeftHanded(_) => "left-handed",
            Self::MiddleEmulation(_) => "middle-emulation",
            Self::AccelProfile(_) => "accel-profile",
            Self::ScrollMethod(_) => "scroll-method",
            Self::ScrollButton(_) => "scroll-button",
            Self::ScrollButtonLock(_) => "scroll-button-lock",
        }
    }

    fn apply(self, device: &mut input::Device) -> input::DeviceConfigResult {
        match self {
            Self::SendEvents(value) => device.config_send_events_set_mode(value),
            Self::NaturalScroll(value) => device.config_scroll_set_natural_scroll_enabled(value),
            Self::AccelSpeed(value) => device.config_accel_set_speed(value),
            Self::LeftHanded(value) => device.config_left_handed_set(value),
            Self::MiddleEmulation(value) => device.config_middle_emulation_set_enabled(value),
            Self::AccelProfile(value) => device.config_accel_set_profile(value),
            Self::ScrollMethod(value) => device.config_scroll_set_method(value),
            Self::ScrollButton(value) => device.config_scroll_set_button(value),
            Self::ScrollButtonLock(value) => device.config_scroll_set_button_lock(value),
        }
    }
}

pub(super) fn apply_pointer_settings(
    settings: impl IntoIterator<Item = PointerSetting>,
    mut apply: impl FnMut(PointerSetting) -> input::DeviceConfigResult,
) -> Vec<(&'static str, input::DeviceConfigError)> {
    settings
        .into_iter()
        .filter_map(|setting| apply(setting).err().map(|error| (setting.name(), error)))
        .collect()
}

fn report_libinput_failure(
    device_name: &str,
    setting: &'static str,
    result: input::DeviceConfigResult,
) {
    if let Err(error) = result {
        warn!(
            device = device_name,
            setting,
            ?error,
            "failed to apply libinput setting"
        );
    }
}

fn apply_common_pointer_settings(
    device: &mut input::Device,
    settings: impl IntoIterator<Item = PointerSetting>,
) {
    let device_name = device.name().into_owned();
    for (setting, error) in apply_pointer_settings(settings, |setting| setting.apply(device)) {
        warn!(
            device = device_name.as_str(),
            setting,
            ?error,
            "failed to apply libinput setting"
        );
    }
}

struct CommonPointerConfig {
    send_events: input::SendEventsMode,
    natural_scroll: bool,
    accel_speed: f64,
    left_handed: bool,
    middle_emulation: bool,
    accel_profile: Option<input::AccelProfile>,
    scroll_method: Option<input::ScrollMethod>,
    scroll_button: Option<u32>,
    scroll_button_lock: bool,
}

fn common_pointer_settings(config: CommonPointerConfig) -> Vec<PointerSetting> {
    let mut settings = vec![
        PointerSetting::SendEvents(config.send_events),
        PointerSetting::NaturalScroll(config.natural_scroll),
        PointerSetting::AccelSpeed(config.accel_speed),
        PointerSetting::LeftHanded(config.left_handed),
        PointerSetting::MiddleEmulation(config.middle_emulation),
    ];
    if let Some(accel_profile) = config.accel_profile {
        settings.push(PointerSetting::AccelProfile(accel_profile));
    }
    if let Some(scroll_method) = config.scroll_method {
        settings.push(PointerSetting::ScrollMethod(scroll_method));
        if scroll_method == input::ScrollMethod::OnButtonDown {
            if let Some(button) = config.scroll_button {
                settings.push(PointerSetting::ScrollButton(button));
            }
            settings.push(PointerSetting::ScrollButtonLock(
                if config.scroll_button_lock {
                    input::ScrollButtonLockState::Enabled
                } else {
                    input::ScrollButtonLockState::Disabled
                },
            ));
        }
    }
    settings
}

pub fn apply_libinput_settings(config: &swayward_config::Input, device: &mut input::Device) {
    let device_name = device.name().into_owned();

    // According to Mutter code, this setting is specific to touchpads.
    let is_touchpad = device.config_tap_finger_count() > 0;
    if is_touchpad {
        let c = &config.touchpad;
        let send_events = if c.off {
            input::SendEventsMode::DISABLED
        } else if c.disabled_on_external_mouse {
            input::SendEventsMode::DISABLED_ON_EXTERNAL_MOUSE
        } else {
            input::SendEventsMode::ENABLED
        };
        let accel_profile = c
            .accel_profile
            .map(Into::into)
            .or_else(|| device.config_accel_default_profile());
        let scroll_method = c
            .scroll_method
            .map(Into::into)
            .or_else(|| device.config_scroll_default_method());
        apply_common_pointer_settings(
            device,
            common_pointer_settings(CommonPointerConfig {
                send_events,
                natural_scroll: c.natural_scroll,
                accel_speed: c.accel_speed.0,
                left_handed: c.left_handed,
                middle_emulation: c.middle_emulation,
                accel_profile,
                scroll_method,
                scroll_button: c.scroll_button,
                scroll_button_lock: c.scroll_button_lock,
            }),
        );

        for (setting, result) in [
            ("tap", device.config_tap_set_enabled(c.tap)),
            ("disable-while-typing", device.config_dwt_set_enabled(c.dwt)),
            (
                "disable-while-trackpointing",
                device.config_dwtp_set_enabled(c.dwtp),
            ),
            (
                "drag-lock",
                device.config_tap_set_drag_lock_enabled(if c.drag_lock {
                    input::DragLockState::EnabledTimeout
                } else {
                    input::DragLockState::Disabled
                }),
            ),
            (
                "drag",
                device.config_tap_set_drag_enabled(
                    c.drag
                        .unwrap_or_else(|| device.config_tap_default_drag_enabled()),
                ),
            ),
        ] {
            report_libinput_failure(&device_name, setting, result);
        }

        if let Some(button_map) = c
            .tap_button_map
            .map(Into::into)
            .or_else(|| device.config_tap_default_button_map())
        {
            report_libinput_failure(
                &device_name,
                "tap-button-map",
                device.config_tap_set_button_map(button_map),
            );
        }
        if let Some(click_method) = c
            .click_method
            .map(Into::into)
            .or_else(|| device.config_click_default_method())
        {
            report_libinput_failure(
                &device_name,
                "click-method",
                device.config_click_set_method(click_method),
            );
        }
    }

    // This is how Mutter tells apart mice.
    let mut is_trackball = false;
    let mut is_trackpoint = false;
    if let Some(udev_device) = unsafe { device.udev_device() } {
        is_trackball = udev_device.property_value("ID_INPUT_TRACKBALL").is_some();
        is_trackpoint = udev_device
            .property_value("ID_INPUT_POINTINGSTICK")
            .is_some();
    }

    let is_mouse = device.has_capability(input::DeviceCapability::Pointer)
        && !is_touchpad
        && !is_trackball
        && !is_trackpoint;

    macro_rules! apply_pointer_config {
        ($config:expr) => {{
            let c = $config;
            let accel_profile = c
                .accel_profile
                .map(Into::into)
                .or_else(|| device.config_accel_default_profile());
            let scroll_method = c
                .scroll_method
                .map(Into::into)
                .or_else(|| device.config_scroll_default_method());
            apply_common_pointer_settings(
                device,
                common_pointer_settings(CommonPointerConfig {
                    send_events: if c.off {
                        input::SendEventsMode::DISABLED
                    } else {
                        input::SendEventsMode::ENABLED
                    },
                    natural_scroll: c.natural_scroll,
                    accel_speed: c.accel_speed.0,
                    left_handed: c.left_handed,
                    middle_emulation: c.middle_emulation,
                    accel_profile,
                    scroll_method,
                    scroll_button: c.scroll_button,
                    scroll_button_lock: c.scroll_button_lock,
                }),
            );
        }};
    }

    if is_mouse {
        apply_pointer_config!(&config.mouse);
    }
    if is_trackball {
        apply_pointer_config!(&config.trackball);
    }
    if is_trackpoint {
        apply_pointer_config!(&config.trackpoint);
    }

    #[rustfmt::skip]
    const IDENTITY_MATRIX: [f32; 6] = [
        1., 0., 0.,
        0., 1., 0.,
    ];

    if device.has_capability(input::DeviceCapability::TabletTool) {
        let c = &config.tablet;
        report_libinput_failure(
            &device_name,
            "send-events",
            device.config_send_events_set_mode(if c.off {
                input::SendEventsMode::DISABLED
            } else {
                input::SendEventsMode::ENABLED
            }),
        );
        report_libinput_failure(
            &device_name,
            "calibration-matrix",
            device.config_calibration_set_matrix(
                c.calibration_matrix
                    .as_deref()
                    .and_then(|matrix| matrix.try_into().ok())
                    .or(device.config_calibration_default_matrix())
                    .unwrap_or(IDENTITY_MATRIX),
            ),
        );
        report_libinput_failure(
            &device_name,
            "left-handed",
            device.config_left_handed_set(c.left_handed),
        );
    }

    if device.has_capability(input::DeviceCapability::Touch) {
        let c = &config.touch;
        report_libinput_failure(
            &device_name,
            "send-events",
            device.config_send_events_set_mode(if c.off {
                input::SendEventsMode::DISABLED
            } else {
                input::SendEventsMode::ENABLED
            }),
        );
        report_libinput_failure(
            &device_name,
            "calibration-matrix",
            device.config_calibration_set_matrix(
                c.calibration_matrix
                    .as_deref()
                    .and_then(|matrix| matrix.try_into().ok())
                    .or(device.config_calibration_default_matrix())
                    .unwrap_or(IDENTITY_MATRIX),
            ),
        );
    }
}

pub fn mods_with_binds(mod_key: ModKey, binds: &Binds, triggers: &[Trigger]) -> HashSet<Modifiers> {
    let mut rv = HashSet::new();
    for bind in &binds.0 {
        if !triggers.contains(&bind.key.trigger) {
            continue;
        }

        let mut mods = bind.key.modifiers;
        if mods.contains(Modifiers::COMPOSITOR) {
            mods.remove(Modifiers::COMPOSITOR);
            mods.insert(mod_key.to_modifiers());
        }

        rv.insert(mods);
    }

    rv
}

pub fn mods_with_mouse_binds(mod_key: ModKey, binds: &Binds) -> HashSet<Modifiers> {
    mods_with_binds(
        mod_key,
        binds,
        &[
            Trigger::MouseLeft,
            Trigger::MouseRight,
            Trigger::MouseMiddle,
            Trigger::MouseBack,
            Trigger::MouseForward,
        ],
    )
}

pub fn mods_with_wheel_binds(mod_key: ModKey, binds: &Binds) -> HashSet<Modifiers> {
    mods_with_binds(
        mod_key,
        binds,
        &[
            Trigger::WheelScrollUp,
            Trigger::WheelScrollDown,
            Trigger::WheelScrollLeft,
            Trigger::WheelScrollRight,
        ],
    )
}

pub fn mods_with_finger_scroll_binds(mod_key: ModKey, binds: &Binds) -> HashSet<Modifiers> {
    mods_with_binds(
        mod_key,
        binds,
        &[
            Trigger::TouchpadScrollUp,
            Trigger::TouchpadScrollDown,
            Trigger::TouchpadScrollLeft,
            Trigger::TouchpadScrollRight,
        ],
    )
}

pub fn mods_with_tablet_stylus_binds(mod_key: ModKey, binds: &Binds) -> HashSet<Modifiers> {
    mods_with_binds(
        mod_key,
        binds,
        &[
            Trigger::TabletStylusButton1,
            Trigger::TabletStylusButton2,
            Trigger::TabletStylusButton3,
        ],
    )
}

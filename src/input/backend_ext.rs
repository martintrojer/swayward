use ::input as libinput;
use smithay::backend::input;
use smithay::backend::winit::WinitVirtualDevice;
use smithay::output::Output;
use smithay::wayland::virtual_keyboard::VirtualKeyboardDevice;

use crate::protocols::virtual_pointer::VirtualPointer;
use crate::swayward::State;

pub trait NiriInputBackend: input::InputBackend<Device = Self::NiriDevice> {
    type NiriDevice: NiriInputDevice;
}
impl<T: input::InputBackend> NiriInputBackend for T
where
    Self::Device: NiriInputDevice,
{
    type NiriDevice = Self::Device;
}

pub trait NiriInputDevice: input::Device {
    fn sway_identifier(&self) -> String {
        let (product, vendor) = self.usb_id().unwrap_or((0, 0));
        let name = self
            .name()
            .trim()
            .chars()
            .map(|ch| {
                if ch == ' ' || ch.is_control() {
                    '_'
                } else {
                    ch
                }
            })
            .collect::<String>();
        format!("{vendor}:{product}:{name}")
    }

    fn sway_libinput(&self) -> Option<serde_json::Value> {
        None
    }

    // FIXME: this should maybe be per-event, not per-device,
    // but it's not clear that this matters in practice?
    // it might be more obvious once we implement it for libinput
    fn output(&self, state: &State) -> Option<Output>;
}

impl NiriInputDevice for libinput::Device {
    fn sway_libinput(&self) -> Option<serde_json::Value> {
        use libinput::{AccelProfile, ScrollButtonLockState, ScrollMethod, SendEventsMode};

        let send_events = match self.config_send_events_mode() {
            SendEventsMode::ENABLED => "enabled",
            SendEventsMode::DISABLED_ON_EXTERNAL_MOUSE => "disabled_on_external_mouse",
            SendEventsMode::DISABLED => "disabled",
            _ => "unknown",
        };
        let mut value = serde_json::json!({"send_events": send_events});
        let object = value.as_object_mut().unwrap();
        if self.config_accel_is_available() {
            object.insert("accel_speed".into(), self.config_accel_speed().into());
            let profile = match self.config_accel_profile() {
                None => "none",
                Some(AccelProfile::Flat) => "flat",
                Some(AccelProfile::Adaptive) => "adaptive",
                Some(_) => "custom",
            };
            object.insert("accel_profile".into(), profile.into());
        }
        if self.config_scroll_has_natural_scroll() {
            object.insert(
                "natural_scroll".into(),
                if self.config_scroll_natural_scroll_enabled() {
                    "enabled"
                } else {
                    "disabled"
                }
                .into(),
            );
        }
        if self.config_left_handed_is_available() {
            object.insert(
                "left_handed".into(),
                if self.config_left_handed() {
                    "enabled"
                } else {
                    "disabled"
                }
                .into(),
            );
        }
        if self.config_middle_emulation_is_available() {
            object.insert(
                "middle_emulation".into(),
                if self.config_middle_emulation_enabled() {
                    "enabled"
                } else {
                    "disabled"
                }
                .into(),
            );
        }
        if !self.config_scroll_methods().is_empty() {
            let method = match self.config_scroll_method() {
                None | Some(ScrollMethod::NoScroll) => "none",
                Some(ScrollMethod::TwoFinger) => "two_finger",
                Some(ScrollMethod::Edge) => "edge",
                Some(ScrollMethod::OnButtonDown) => "on_button_down",
                Some(_) => "unknown",
            };
            object.insert("scroll_method".into(), method.into());
            if self
                .config_scroll_methods()
                .contains(&ScrollMethod::OnButtonDown)
            {
                object.insert("scroll_button".into(), self.config_scroll_button().into());
                object.insert(
                    "scroll_button_lock".into(),
                    match self.config_scroll_button_lock() {
                        ScrollButtonLockState::Enabled => "enabled",
                        ScrollButtonLockState::Disabled => "disabled",
                    }
                    .into(),
                );
            }
        }
        Some(value)
    }

    fn output(&self, _state: &State) -> Option<Output> {
        // FIXME: Allow specifying the output per-device?
        None
    }
}

impl NiriInputDevice for WinitVirtualDevice {
    fn output(&self, _state: &State) -> Option<Output> {
        // FIXME: we should be returning the single output that the winit backend creates,
        // but for now, that will cause issues because the output is normally upside down,
        // so we apply Transform::Flipped180 to it and that would also cause
        // the cursor position to be flipped, which is not what we want.
        //
        // instead, we just return None and rely on the fact that it has only one output.
        // doing so causes the cursor to be placed in *global* output coordinates,
        // which are not flipped, and happen to be what we want.
        None
    }
}

impl NiriInputDevice for VirtualKeyboardDevice {
    fn output(&self, _state: &State) -> Option<Output> {
        None
    }
}

impl NiriInputDevice for VirtualPointer {
    fn output(&self, _: &State) -> Option<Output> {
        self.output().cloned()
    }
}

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
        Some(describe_libinput_device(&RawLibinputDevice(self)))
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

/// The libinput configuration queries sway's GET_INPUTS serializer reads, as
/// raw libinput enum values so that unknown values serialize as sway does.
///
/// Raw values instead of the `input` crate's typed getters: those panic on an
/// enum value newer than the crate (`config_tap_drag_lock_enabled` on
/// LIBINPUT_CONFIG_DRAG_LOCK_ENABLED_STICKY), where sway prints "unknown" or
/// the sticky name.
pub(crate) trait LibinputQuery {
    fn send_events_mode(&self) -> u32;
    fn tap_finger_count(&self) -> i32;
    fn tap_enabled(&self) -> u32;
    fn tap_button_map(&self) -> u32;
    fn tap_drag_enabled(&self) -> u32;
    fn tap_drag_lock_enabled(&self) -> u32;
    fn accel(&self) -> Option<(f64, u32)>;
    fn natural_scroll(&self) -> Option<bool>;
    fn left_handed(&self) -> Option<bool>;
    fn click_methods(&self) -> u32;
    fn click_method(&self) -> u32;
    fn clickfinger_button_map(&self) -> u32;
    fn middle_emulation(&self) -> Option<u32>;
    fn scroll_methods(&self) -> u32;
    fn scroll_method(&self) -> u32;
    fn scroll_button(&self) -> u32;
    fn scroll_button_lock(&self) -> u32;
    fn dwt(&self) -> Option<u32>;
    fn dwtp(&self) -> Option<u32>;
    fn calibration_matrix(&self) -> Option<[f32; 6]>;
}

struct RawLibinputDevice<'a>(&'a libinput::Device);

impl RawLibinputDevice<'_> {
    fn raw(&self) -> *mut libinput::ffi::libinput_device {
        use libinput::AsRaw as _;
        self.0.as_raw_mut()
    }
}

// SAFETY (every method): `raw()` is a live libinput_device owned by the
// borrowed `input::Device`, and each call is a libinput getter that only reads
// device configuration.
impl LibinputQuery for RawLibinputDevice<'_> {
    fn send_events_mode(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_send_events_get_mode(self.raw()) }
    }
    fn tap_finger_count(&self) -> i32 {
        unsafe { libinput::ffi::libinput_device_config_tap_get_finger_count(self.raw()) }
    }
    fn tap_enabled(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_tap_get_enabled(self.raw()) }
    }
    fn tap_button_map(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_tap_get_button_map(self.raw()) }
    }
    fn tap_drag_enabled(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_tap_get_drag_enabled(self.raw()) }
    }
    fn tap_drag_lock_enabled(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_tap_get_drag_lock_enabled(self.raw()) }
    }
    fn accel(&self) -> Option<(f64, u32)> {
        unsafe {
            (libinput::ffi::libinput_device_config_accel_is_available(self.raw()) != 0).then(|| {
                (
                    libinput::ffi::libinput_device_config_accel_get_speed(self.raw()),
                    libinput::ffi::libinput_device_config_accel_get_profile(self.raw()),
                )
            })
        }
    }
    fn natural_scroll(&self) -> Option<bool> {
        unsafe {
            (libinput::ffi::libinput_device_config_scroll_has_natural_scroll(self.raw()) != 0).then(
                || {
                    libinput::ffi::libinput_device_config_scroll_get_natural_scroll_enabled(
                        self.raw(),
                    ) != 0
                },
            )
        }
    }
    fn left_handed(&self) -> Option<bool> {
        unsafe {
            (libinput::ffi::libinput_device_config_left_handed_is_available(self.raw()) != 0)
                .then(|| libinput::ffi::libinput_device_config_left_handed_get(self.raw()) != 0)
        }
    }
    fn click_methods(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_click_get_methods(self.raw()) }
    }
    fn click_method(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_click_get_method(self.raw()) }
    }
    fn clickfinger_button_map(&self) -> u32 {
        // libinput 1.26 added this getter, past the `input` crate's
        // libinput_1_21 bindings, and Ubuntu 24.04 (the CI image) ships 1.25,
        // so resolve it at runtime. Without it, libinput's behaviour is
        // LIBINPUT_CONFIG_CLICKFINGER_MAP_LRM (0).
        type Getter = unsafe extern "C" fn(*mut libinput::ffi::libinput_device) -> u32;
        static GETTER: std::sync::OnceLock<Option<Getter>> = std::sync::OnceLock::new();
        let getter = GETTER.get_or_init(|| {
            let symbol = unsafe {
                libc::dlsym(
                    libc::RTLD_DEFAULT,
                    c"libinput_device_config_click_get_clickfinger_button_map".as_ptr(),
                )
            };
            (!symbol.is_null())
                .then(|| unsafe { std::mem::transmute::<*mut libc::c_void, Getter>(symbol) })
        });
        getter.map_or(0, |getter| unsafe { getter(self.raw()) })
    }
    fn middle_emulation(&self) -> Option<u32> {
        unsafe {
            (libinput::ffi::libinput_device_config_middle_emulation_is_available(self.raw()) != 0)
                .then(|| {
                    libinput::ffi::libinput_device_config_middle_emulation_get_enabled(self.raw())
                })
        }
    }
    fn scroll_methods(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_scroll_get_methods(self.raw()) }
    }
    fn scroll_method(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_scroll_get_method(self.raw()) }
    }
    fn scroll_button(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_scroll_get_button(self.raw()) }
    }
    fn scroll_button_lock(&self) -> u32 {
        unsafe { libinput::ffi::libinput_device_config_scroll_get_button_lock(self.raw()) }
    }
    fn dwt(&self) -> Option<u32> {
        unsafe {
            (libinput::ffi::libinput_device_config_dwt_is_available(self.raw()) != 0)
                .then(|| libinput::ffi::libinput_device_config_dwt_get_enabled(self.raw()))
        }
    }
    fn dwtp(&self) -> Option<u32> {
        unsafe {
            (libinput::ffi::libinput_device_config_dwtp_is_available(self.raw()) != 0)
                .then(|| libinput::ffi::libinput_device_config_dwtp_get_enabled(self.raw()))
        }
    }
    fn calibration_matrix(&self) -> Option<[f32; 6]> {
        unsafe {
            if libinput::ffi::libinput_device_config_calibration_has_matrix(self.raw()) == 0 {
                return None;
            }
            let mut matrix = [0.0; 6];
            libinput::ffi::libinput_device_config_calibration_get_matrix(
                self.raw(),
                matrix.as_mut_ptr(),
            );
            Some(matrix)
        }
    }
}

fn state_name(value: u32, names: &[(u32, &'static str)]) -> &'static str {
    names
        .iter()
        .find_map(|(raw, name)| (*raw == value).then_some(*name))
        .unwrap_or("unknown")
}

/// Port of sway's describe_libinput_device (sway/sway/ipc-json.c:900-1137 at
/// 1.12): the same fields, guards and value names. Enum values are libinput's
/// (libinput.h); a value sway does not name is "unknown". Key order follows
/// serde_json's map, as every other swayward IPC object does.
pub(crate) fn describe_libinput_device(device: &impl LibinputQuery) -> serde_json::Value {
    const DISABLED_ENABLED: &[(u32, &str)] = &[(0, "disabled"), (1, "enabled")];
    let mut object = serde_json::Map::new();
    let mut insert = |key: &str, value: serde_json::Value| {
        object.insert(key.into(), value);
    };

    // ipc-json.c:903-916. ENABLED=0, DISABLED=1, DISABLED_ON_EXTERNAL_MOUSE=2.
    insert(
        "send_events",
        state_name(
            device.send_events_mode(),
            &[
                (0, "enabled"),
                (2, "disabled_on_external_mouse"),
                (1, "disabled"),
            ],
        )
        .into(),
    );

    // ipc-json.c:918-970, only when tapping is available.
    if device.tap_finger_count() > 0 {
        insert(
            "tap",
            state_name(device.tap_enabled(), DISABLED_ENABLED).into(),
        );
        insert(
            "tap_button_map",
            state_name(device.tap_button_map(), &[(0, "lrm"), (1, "lmr")]).into(),
        );
        insert(
            "tap_drag",
            state_name(device.tap_drag_enabled(), DISABLED_ENABLED).into(),
        );
        // DISABLED=0, ENABLED(_TIMEOUT)=1, ENABLED_STICKY=2; sway names the
        // sticky value when built against libinput >= 1.27.
        insert(
            "tap_drag_lock",
            state_name(
                device.tap_drag_lock_enabled(),
                &[(0, "disabled"), (1, "enabled"), (2, "enabled_sticky")],
            )
            .into(),
        );
    }

    // ipc-json.c:972-996. NONE=0, FLAT=1<<0, ADAPTIVE=1<<1, CUSTOM=1<<2.
    if let Some((speed, profile)) = device.accel() {
        insert("accel_speed", speed.into());
        insert(
            "accel_profile",
            state_name(
                profile,
                &[(0, "none"), (1, "flat"), (2, "adaptive"), (4, "custom")],
            )
            .into(),
        );
    }

    // ipc-json.c:998-1014.
    if let Some(enabled) = device.natural_scroll() {
        insert(
            "natural_scroll",
            if enabled { "enabled" } else { "disabled" }.into(),
        );
    }
    if let Some(enabled) = device.left_handed() {
        insert(
            "left_handed",
            if enabled { "enabled" } else { "disabled" }.into(),
        );
    }

    // ipc-json.c:1016-1044. NONE=0, BUTTON_AREAS=1<<0, CLICKFINGER=1<<1.
    if device.click_methods() != 0 {
        insert(
            "click_method",
            state_name(
                device.click_method(),
                &[(0, "none"), (1, "button_areas"), (2, "clickfinger")],
            )
            .into(),
        );
        insert(
            "clickfinger_button_map",
            state_name(device.clickfinger_button_map(), &[(0, "lrm"), (1, "lmr")]).into(),
        );
    }

    // ipc-json.c:1046-1058.
    if let Some(state) = device.middle_emulation() {
        insert(
            "middle_emulation",
            state_name(state, DISABLED_ENABLED).into(),
        );
    }

    // ipc-json.c:1060-1096. NO_SCROLL=0, 2FG=1<<0, EDGE=1<<1, ON_BUTTON_DOWN=1<<2.
    let scroll_methods = device.scroll_methods();
    if scroll_methods != 0 {
        insert(
            "scroll_method",
            state_name(
                device.scroll_method(),
                &[
                    (0, "none"),
                    (1, "two_finger"),
                    (2, "edge"),
                    (4, "on_button_down"),
                ],
            )
            .into(),
        );
        if scroll_methods & 4 != 0 {
            insert("scroll_button", device.scroll_button().into());
            insert(
                "scroll_button_lock",
                state_name(device.scroll_button_lock(), DISABLED_ENABLED).into(),
            );
        }
    }

    // ipc-json.c:1098-1122.
    if let Some(state) = device.dwt() {
        insert("dwt", state_name(state, DISABLED_ENABLED).into());
    }
    if let Some(state) = device.dwtp() {
        insert("dwtp", state_name(state, DISABLED_ENABLED).into());
    }

    // ipc-json.c:1124-1134: six doubles from the float matrix.
    if let Some(matrix) = device.calibration_matrix() {
        insert(
            "calibration_matrix",
            matrix
                .iter()
                .map(|value| f64::from(*value))
                .collect::<Vec<_>>()
                .into(),
        );
    }

    serde_json::Value::Object(object)
}

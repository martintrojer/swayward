#[derive(Debug)]
struct TestInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TestDevice {
    name: &'static str,
    keyboard: bool,
    libinput: bool,
    touchpad: bool,
}

impl TestDevice {
    fn keyboard(name: &'static str) -> Self {
        Self {
            name,
            keyboard: true,
            libinput: false,
            touchpad: false,
        }
    }

    fn pointer(name: &'static str) -> Self {
        Self {
            name,
            keyboard: false,
            libinput: false,
            touchpad: false,
        }
    }

    fn libinput_pointer(name: &'static str) -> Self {
        Self {
            name,
            keyboard: false,
            libinput: true,
            touchpad: false,
        }
    }

    /// A libinput pointer that supports tap-to-click, which sway types as a
    /// touchpad.
    fn touchpad(name: &'static str) -> Self {
        Self {
            name,
            keyboard: false,
            libinput: true,
            touchpad: true,
        }
    }
}

impl crate::input::backend_ext::NiriInputDevice for TestDevice {
    fn is_touchpad(&self) -> bool {
        self.touchpad
    }

    fn sway_libinput(&self) -> Option<Value> {
        self.libinput.then(|| {
            serde_json::json!({
                "send_events": "enabled",
                "accel_speed": 0.0,
                "accel_profile": "adaptive",
                "natural_scroll": "disabled",
                "left_handed": "disabled",
                "middle_emulation": "disabled",
                "scroll_method": "none",
                "scroll_button": 274,
                "scroll_button_lock": "disabled"
            })
        })
    }

    fn output(&self, _state: &crate::swayward::State) -> Option<smithay::output::Output> {
        None
    }
}

impl smithay::backend::input::Device for TestDevice {
    fn id(&self) -> String {
        self.name.into()
    }

    fn name(&self) -> String {
        self.name.into()
    }

    fn has_capability(&self, capability: smithay::backend::input::DeviceCapability) -> bool {
        capability
            == if self.keyboard {
                smithay::backend::input::DeviceCapability::Keyboard
            } else {
                smithay::backend::input::DeviceCapability::Pointer
            }
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        self.libinput.then_some((16518, 1133))
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

#[derive(Debug)]
struct TestKeyEvent {
    device: TestDevice,
    key: u32,
    count: u32,
    state: smithay::backend::input::KeyState,
}

#[derive(Debug)]
struct TestButtonEvent {
    device: TestDevice,
    button: u32,
    state: smithay::backend::input::ButtonState,
}

#[derive(Debug)]
struct TestSwitchEvent {
    device: TestDevice,
    switch: smithay::backend::input::Switch,
    state: smithay::backend::input::SwitchState,
}

/// Absolute pointer motion, so a test can drive an interactive drag.
///
/// Without this the conformance adapter could only teleport the cursor with
/// `Swayward::move_cursor`, which updates pointer contents but never reaches the
/// pointer grab, so `Layout::interactive_move_update` never ran and a dragged
/// window never moved.
#[derive(Debug)]
struct TestMotionAbsoluteEvent {
    device: TestDevice,
    x: f64,
    y: f64,
    output_size: smithay::utils::Size<f64, smithay::utils::Logical>,
}

#[derive(Debug)]
struct TestAxisEvent {
    device: TestDevice,
    horizontal_v120: f64,
    vertical_v120: f64,
}

impl smithay::backend::input::Event<TestInput> for TestKeyEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::Event<TestInput> for TestSwitchEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::SwitchToggleEvent<TestInput> for TestSwitchEvent {
    fn switch(&self) -> Option<smithay::backend::input::Switch> {
        Some(self.switch)
    }

    fn state(&self) -> smithay::backend::input::SwitchState {
        self.state
    }
}

impl smithay::backend::input::Event<TestInput> for TestButtonEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::Event<TestInput> for TestAxisEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::PointerAxisEvent<TestInput> for TestAxisEvent {
    fn amount(&self, _axis: smithay::backend::input::Axis) -> Option<f64> {
        None
    }

    fn amount_v120(&self, axis: smithay::backend::input::Axis) -> Option<f64> {
        Some(match axis {
            smithay::backend::input::Axis::Horizontal => self.horizontal_v120,
            smithay::backend::input::Axis::Vertical => self.vertical_v120,
        })
    }

    fn source(&self) -> smithay::backend::input::AxisSource {
        smithay::backend::input::AxisSource::Wheel
    }

    fn relative_direction(
        &self,
        _axis: smithay::backend::input::Axis,
    ) -> smithay::backend::input::AxisRelativeDirection {
        smithay::backend::input::AxisRelativeDirection::Identical
    }
}

impl smithay::backend::input::PointerButtonEvent<TestInput> for TestButtonEvent {
    fn button_code(&self) -> u32 {
        self.button
    }

    fn state(&self) -> smithay::backend::input::ButtonState {
        self.state
    }
}

impl smithay::backend::input::Event<TestInput> for TestMotionAbsoluteEvent {
    fn time(&self) -> smithay::backend::input::InputTime {
        smithay::backend::input::InputTime::from_millis(1)
    }

    fn device(&self) -> TestDevice {
        self.device
    }
}

impl smithay::backend::input::AbsolutePositionEvent<TestInput> for TestMotionAbsoluteEvent {
    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }

    fn x_transformed(&self, width: i32) -> f64 {
        self.x * f64::from(width) / self.output_size.w
    }

    fn y_transformed(&self, height: i32) -> f64 {
        self.y * f64::from(height) / self.output_size.h
    }
}

impl smithay::backend::input::PointerMotionAbsoluteEvent<TestInput> for TestMotionAbsoluteEvent {}

impl smithay::backend::input::KeyboardKeyEvent<TestInput> for TestKeyEvent {
    fn key_code(&self) -> smithay::backend::input::Keycode {
        self.key.into()
    }

    fn state(&self) -> smithay::backend::input::KeyState {
        self.state
    }

    fn count(&self) -> u32 {
        self.count
    }
}

impl smithay::backend::input::InputBackend for TestInput {
    type Device = TestDevice;
    type KeyboardKeyEvent = TestKeyEvent;
    type PointerAxisEvent = TestAxisEvent;
    type PointerButtonEvent = TestButtonEvent;
    type PointerMotionEvent = smithay::backend::input::UnusedEvent;
    type PointerMotionAbsoluteEvent = TestMotionAbsoluteEvent;
    type GestureSwipeBeginEvent = smithay::backend::input::UnusedEvent;
    type GestureSwipeUpdateEvent = smithay::backend::input::UnusedEvent;
    type GestureSwipeEndEvent = smithay::backend::input::UnusedEvent;
    type GesturePinchBeginEvent = smithay::backend::input::UnusedEvent;
    type GesturePinchUpdateEvent = smithay::backend::input::UnusedEvent;
    type GesturePinchEndEvent = smithay::backend::input::UnusedEvent;
    type GestureHoldBeginEvent = smithay::backend::input::UnusedEvent;
    type GestureHoldEndEvent = smithay::backend::input::UnusedEvent;
    type TouchDownEvent = smithay::backend::input::UnusedEvent;
    type TouchUpEvent = smithay::backend::input::UnusedEvent;
    type TouchMotionEvent = smithay::backend::input::UnusedEvent;
    type TouchCancelEvent = smithay::backend::input::UnusedEvent;
    type TouchFrameEvent = smithay::backend::input::UnusedEvent;
    type TabletToolAxisEvent = smithay::backend::input::UnusedEvent;
    type TabletToolProximityEvent = smithay::backend::input::UnusedEvent;
    type TabletToolTipEvent = smithay::backend::input::UnusedEvent;
    type TabletToolButtonEvent = smithay::backend::input::UnusedEvent;
    type SwitchToggleEvent = TestSwitchEvent;
    type SpecialEvent = ();
}

fn active_workspace_name(fixture: &mut Fixture) -> Option<String> {
    fixture
        .swayward()
        .layout
        .active_workspace()
        .and_then(|workspace| workspace.name().cloned())
}

pub(super) fn pointer_button(fixture: &mut Fixture, button: u32, pressed: bool) {
    pointer_button_from(
        fixture,
        TestDevice::pointer("test pointer"),
        button,
        pressed,
    );
}

fn pointer_button_from(fixture: &mut Fixture, device: TestDevice, button: u32, pressed: bool) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::PointerButton {
            event: TestButtonEvent {
                device,
                button,
                state: if pressed {
                    smithay::backend::input::ButtonState::Pressed
                } else {
                    smithay::backend::input::ButtonState::Released
                },
            },
        },
    );
}

/// Absolute pointer motion through the real input path, so a pointer grab sees
/// it and an interactive drag actually tracks the cursor.
pub(super) fn pointer_motion_absolute(fixture: &mut Fixture, x: f64, y: f64) {
    let output = fixture.swayward().global_space.outputs().next().cloned();
    let output_size = output
        .and_then(|output| {
            fixture
                .swayward()
                .global_space
                .output_geometry(&output)
                .map(|geo| geo.size.to_f64())
        })
        .unwrap_or_else(|| smithay::utils::Size::from((1920., 1080.)));
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::PointerMotionAbsolute {
            event: TestMotionAbsoluteEvent {
                device: TestDevice::pointer("test pointer"),
                x,
                y,
                output_size,
            },
        },
    );
}

pub(super) fn pointer_axis(fixture: &mut Fixture, horizontal_v120: f64, vertical_v120: f64) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::PointerAxis {
            event: TestAxisEvent {
                device: TestDevice::pointer("test pointer"),
                horizontal_v120,
                vertical_v120,
            },
        },
    );
}

pub(super) fn key_event(fixture: &mut Fixture, key: u32, pressed: bool) {
    key_event_from(fixture, TestDevice::keyboard("test keyboard"), key, pressed);
}

/// Press and release `Mod4+a` as a real key sequence.
fn press_mod_a(fixture: &mut Fixture) {
    key_event(fixture, 133, true);
    key_event(fixture, 38, true);
    key_event(fixture, 38, false);
    key_event(fixture, 133, false);
}

fn switch_event(
    fixture: &mut Fixture,
    switch: smithay::backend::input::Switch,
    state: smithay::backend::input::SwitchState,
) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::SwitchToggle {
            event: TestSwitchEvent {
                device: TestDevice::keyboard("test switch"),
                switch,
                state,
            },
        },
    );
}

fn key_event_from(fixture: &mut Fixture, device: TestDevice, key: u32, pressed: bool) {
    fixture.niri_state().process_input_event::<TestInput>(
        smithay::backend::input::InputEvent::Keyboard {
            event: TestKeyEvent {
                device,
                key,
                count: u32::from(pressed),
                state: if pressed {
                    smithay::backend::input::KeyState::Pressed
                } else {
                    smithay::backend::input::KeyState::Released
                },
            },
        },
    );
}

pub(super) fn type_key_chords(fixture: &mut Fixture, chords: &[&[u32]]) {
    for chord in chords {
        for &key in *chord {
            key_event(fixture, key, true);
        }
        for &key in chord.iter().rev() {
            key_event(fixture, key, false);
        }
    }
}

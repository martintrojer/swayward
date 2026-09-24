#[test]
fn run_command_returns_one_outcome_per_command_and_keeps_connection_alive() {
    let (mut fixture, socket) = ipc_fixture();
    let mut stream = UnixStream::connect(socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "focus left; frobnicate",
        ))
        .unwrap();

    let (msg_type, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(msg_type, MessageType::RunCommand as u32);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([
            {"success": true},
            {"success": false, "error": "Unknown/invalid command 'frobnicate'", "parse_error": true}
        ])
    );

    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::RunCommand, "nop"))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!([{"success": true}])
    );
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[test]
fn mark_event_matches_captured_sway_schema() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("event-one".into());
    window.set_title("event-one");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    assert!(crate::command::execute(fixture.niri_state(), "mark event-mark")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let expected: Value = serde_json::from_str(sway_fixture!("events/window.mark.json")).unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$window",
    );
}

#[test]
fn close_event_matches_captured_sway_schema_before_removal() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (800, 600));
    let client = fixture.add_client();
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id("event-one".into());
    window.set_title("event-one");
    window.commit();
    let surface = window.surface.clone();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);

    assert!(crate::command::execute(fixture.niri_state(), "floating enable")[0].success);
    fixture.niri_state().ipc_refresh_layout();
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["window"]"#,
        ))
        .unwrap();
    let _ = read_ipc_reply(&mut fixture, &mut subscriber);

    let window = fixture.client(client).window(&surface);
    window.attach_null();
    window.commit();
    fixture.double_roundtrip(client);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 3);
    let expected: Value = serde_json::from_str(sway_fixture!("events/window.close.json")).unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$window",
    );
}

#[test]
fn marks_round_trip_through_commands_get_marks_and_tree() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let id = fixture.add_client();
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-1".into());
    window.set_title("fixture-1");
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "mark testmark",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([{"success": true}])
    );

    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::GetMarks, ""))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!(["testmark"])
    );

    let swayward = fixture.swayward();
    let tree = serde_json::to_value(describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    ))
    .unwrap();
    let marked = find_json_node(&tree, "con", true).unwrap();
    let oracle: Value = serde_json::from_str(sway_fixture!("marked.tree.json")).unwrap();
    let expected = find_json_node(&oracle, "con", true).unwrap();
    assert_eq!(marked["marks"], expected["marks"]);

    let mut stream = UnixStream::connect(&socket).unwrap();
    stream
        .write_all(&swayward_ipc::wire::encode(
            MessageType::RunCommand,
            "mark --add second, mark --add --toggle testmark; [con_mark=second] unmark",
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert!(serde_json::from_str::<Vec<Value>>(&reply)
        .unwrap()
        .iter()
        .all(|outcome| outcome["success"] == true));
    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::GetMarks, ""))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    assert_eq!(
        serde_json::from_str::<Value>(&reply).unwrap(),
        serde_json::json!([])
    );
}

/// Sway's GET_MARKS walks the container tree and appends each container's
/// marks in the order it meets them (`sway/tree/root.c:246-260`,
/// `sway/ipc-server.c:604-610,825-834`). It never sorts, and it visits every
/// container, not only the ones holding a view.
///
/// Swayward sorted the list and read only the per-window map, so a mark set
/// on a split container was reported by GET_TREE and missing from GET_MARKS.
#[test]
fn get_marks_reports_container_marks_in_tree_order_like_sway() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let id = fixture.add_client();

    // Two windows, so there is a split container to mark.
    for index in 0..2 {
        let window = fixture.client(id).create_window();
        window.xdg_toplevel.set_app_id(format!("fixture-{index}"));
        window.set_title(&format!("fixture-{index}"));
        let surface = window.surface.clone();
        window.commit();
        fixture.roundtrip(id);
        let window = fixture.client(id).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(id);
    }

    let mut stream = UnixStream::connect(&socket).unwrap();
    let run = |fixture: &mut Fixture, stream: &mut UnixStream, command: &str| {
        stream
            .write_all(&swayward_ipc::wire::encode(
                MessageType::RunCommand,
                command,
            ))
            .unwrap();
        let (_, reply) = read_ipc_reply(fixture, stream);
        reply
    };

    // "zeta" is marked first but sorts last, so a sorted reply reorders it.
    // Split the second window vertically and add a third, so the tree holds a
    // real split container below the workspace. `focus parent` from a leaf of
    // that split reaches the container, not the workspace: sway rejects `mark`
    // on a workspace with "Only containers can have marks"
    // (`sway/commands/mark.c:20-23`).
    run(&mut fixture, &mut stream, "splitv");
    let window = fixture.client(id).create_window();
    window.xdg_toplevel.set_app_id("fixture-2".into());
    window.set_title("fixture-2");
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(id);
    let window = fixture.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(id);

    // "zeta" is marked first but sorts last, so a sorted reply reorders it.
    run(&mut fixture, &mut stream, "mark zeta");
    run(&mut fixture, &mut stream, "focus parent");
    run(&mut fixture, &mut stream, "mark alpha");

    stream
        .write_all(&swayward_ipc::wire::encode(MessageType::GetMarks, ""))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut stream);
    let marks: Vec<String> = serde_json::from_str(&reply).unwrap();

    assert!(
        marks.contains(&"alpha".to_string()),
        "a mark on a split container must appear in GET_MARKS, as sway walks \
         every container and not only views: {marks:?}"
    );
    assert_eq!(
        marks,
        vec!["alpha".to_string(), "zeta".to_string()],
        "GET_MARKS must follow sway's tree walk, which reaches the parent \
         before its children, rather than sorting: {marks:?}"
    );
}

#[derive(Debug)]
struct TestInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TestDevice {
    name: &'static str,
    keyboard: bool,
    libinput: bool,
}

impl TestDevice {
    fn keyboard(name: &'static str) -> Self {
        Self {
            name,
            keyboard: true,
            libinput: false,
        }
    }

    fn pointer(name: &'static str) -> Self {
        Self {
            name,
            keyboard: false,
            libinput: false,
        }
    }

    fn libinput_pointer(name: &'static str) -> Self {
        Self {
            name,
            keyboard: false,
            libinput: true,
        }
    }
}

impl crate::input::backend_ext::NiriInputDevice for TestDevice {
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

#[test]
fn mouse_input_device_binding_prefers_exact_device_and_wildcard_matches_another() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "rename workspace to wildcard-mouse"; }
            MouseLeft input-device="0:0:first_mouse" { command "rename workspace to exact-mouse"; }
            MouseRight input-device="0:0:first_mouse" { command "rename workspace to wrong-mouse"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("first mouse"),
            0x110,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("second mouse"),
            0x111,
            pressed,
        );
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong-mouse")
    );

    for pressed in [true, false] {
        pointer_button_from(
            &mut fixture,
            TestDevice::pointer("second mouse"),
            0x110,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard-mouse")
    );
}

#[test]
fn device_identifier_matches_sways_libinput_format() {
    use crate::input::backend_ext::NiriInputDevice as _;

    let device = TestDevice::keyboard("  keyboard with spaces  ");
    assert_eq!(device.sway_identifier(), "0:0:keyboard_with_spaces");
}

#[test]
fn input_device_binding_prefers_exact_device_and_wildcard_matches_another() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            x { command "rename workspace to wildcard"; }
            x input-device="0:0:first_keyboard" { command "rename workspace to exact"; }
            z input-device="0:0:first_keyboard" { command "rename workspace to wrong"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("first keyboard"),
            53,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact")
    );

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("second keyboard"),
            52,
            pressed,
        );
    }
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wrong")
    );

    for pressed in [true, false] {
        key_event_from(
            &mut fixture,
            TestDevice::keyboard("second keyboard"),
            53,
            pressed,
        );
    }
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );
}

fn set_xkb_layout(fixture: &mut Fixture, layout: u32) {
    let keyboard = fixture.swayward().seat.get_keyboard().unwrap();
    keyboard.with_xkb_state(fixture.niri_state(), |mut context| {
        context.set_layout(smithay::input::keyboard::Layout(layout));
    });
}

#[test]
fn group_binding_overrides_wildcard_only_in_its_active_group() {
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { xkb { layout "us,ru,us"; }; }; }
        binds {
            x { command "rename workspace to wildcard"; };
            Group2+x { command "rename workspace to exact"; };
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );

    set_xkb_layout(&mut fixture, 1);
    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("exact")
    );
    set_xkb_layout(&mut fixture, 2);
    type_key_chords(&mut fixture, &[&[53]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("wildcard")
    );
}

#[test]
fn translated_keysym_binding_fires_in_its_xkb_layout() {
    let config = swayward_config::Config::parse_mem(
        r#"input { keyboard { xkb { layout "us,ru"; }; }; }
        binds { Cyrillic_ze { command "rename workspace to cyrillic"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[33]]);
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("cyrillic")
    );

    set_xkb_layout(&mut fixture, 1);
    type_key_chords(&mut fixture, &[&[33]]);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("cyrillic")
    );
}

fn add_tiled_windows(fixture: &mut Fixture, client: super::client::ClientId, count: usize) {
    for _ in 0..count {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}

#[test]
fn titlebar_wheel_binding_takes_precedence_over_tab_focus() {
    let config = swayward_config::Config::parse_mem(
        r#"layout { gaps 0; }
        binds {
            WheelScrollDown mouse-regions="titlebar" { command "mark bound"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();
    add_tiled_windows(&mut fixture, client, 3);
    assert!(crate::command::execute(fixture.niri_state(), "layout tabbed")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "focus left")[0].success);
    let focused = fixture.swayward().layout.focus().unwrap().id();
    fixture.niri_state().move_cursor((100., 10.).into());
    pointer_axis(&mut fixture, 0., 120.);

    let swayward = fixture.swayward();
    assert!(swayward
        .marks_by_window
        .values()
        .chain(swayward.marks_by_container.values())
        .flatten()
        .any(|mark| mark == "bound"));
    assert_eq!(swayward.layout.focus().unwrap().id(), focused);
}

#[test]
fn pointer_button_binding_requires_the_configured_rendered_region() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace startup"; }
            MouseLeft mouse-regions="contents" { command "workspace clicked"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("clicked")
        .is_none());
}

/// Runtime key binding commands must mutate the exact table the keyboard path
/// reads: add must fire, unbind must stop firing, and reload must restore the
/// file-backed table rather than retaining runtime mutations.
#[test]
fn runtime_bindsym_fires_unbinds_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let added = crate::command::execute(
        fixture.niri_state(),
        "bindsym Mod4+a workspace runtime-bound",
    );
    assert!(added[0].success, "{added:?}");
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("runtime-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace unbound-check")[0].success);
    let removed = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+a");
    assert!(removed[0].success, "{removed:?}");
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("unbound-check")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace should-not-survive",
        )[0]
        .success
    );
    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-check")[0].success);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("reload-check")
    );
}

#[test]
fn runtime_bindcode_fires_and_unbindcode_stops_it() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(fixture.niri_state(), "bindcode 39 workspace code-bound",)[0]
            .success
    );
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("code-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "unbindcode 39")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace code-unbound")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("code-unbound")
    );
}

#[test]
fn runtime_bindsym_release_fires_only_on_key_release() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym --release a workspace released",
        )[0]
        .success
    );

    key_event(&mut fixture, 38, true);
    assert_ne!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("released")
    );
    key_event(&mut fixture, 38, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("released")
    );
}

/// Sway replaces an equal binding rather than appending a competing one
/// (`binding_upsert`, sway/sway/commands/bind.c:260-278).
#[test]
fn runtime_bindsym_duplicate_overwrites_the_old_command() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace first-command",
        )[0]
        .success
    );
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindsym Mod4+a workspace replacement-command",
        )[0]
        .success
    );
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);

    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("replacement-command")
    );
    assert_eq!(fixture.swayward().config.borrow().binds.0.len(), 1);
}

/// `unbind*` reports failure when no binding has the same key and flags
/// (`sway/sway/commands/bind.c:302-320`) and leaves the table alone.
#[test]
fn runtime_unbindsym_missing_binding_fails_without_mutation() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let before = fixture.swayward().config.borrow().binds.0.clone();

    let outcome = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+a");
    assert!(!outcome[0].success);
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("Could not find binding `Mod4+a` for the given flags")
    );
    assert_eq!(fixture.swayward().config.borrow().binds.0, before);
}

/// Top-level runtime binds target sway's current mode, not always the default
/// (`sway/sway/commands/bind.c:291-298`).
#[test]
fn runtime_bindsym_mutates_the_active_binding_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds { Mod4+a { command "workspace default-mode"; }; }
mode "resize" {
    Mod4+b { command "nop"; };
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+a workspace resize-mode",)[0]
            .success
    );

    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("resize-mode")
    );

    // The default table was not overwritten.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-mode")
    );
}

/// Runtime variable substitution happens before cmd_bindsym stores its command,
/// so redefining the variable later does not rewrite the captured binding.
#[test]
fn runtime_bindsym_captures_the_current_variable_value() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "set $dest captured")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+a workspace $dest",)[0].success
    );
    assert!(crate::command::execute(fixture.niri_state(), "set $dest later")[0].success);

    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("captured")
    );
}

/// A binding may mutate the binding table while it is itself being dispatched.
/// Sway does this safely because the table is a plain list; swayward holds it
/// behind a `RefCell`, so a live borrow across dispatch would panic rather than
/// misbehave. Drive the reentrant case through real key input to prove the
/// borrow is released before the command runs.
#[test]
fn a_binding_may_rebind_and_unbind_itself_while_dispatching() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Mod4+a { command "bindsym Mod4+b workspace chained"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    // Mod4+a adds Mod4+b from inside its own dispatch.
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 56, true);
    key_event(&mut fixture, 56, false);
    key_event(&mut fixture, 133, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("chained")
    );

    // A binding that removes itself takes effect from the next press onwards.
    assert!(
        crate::command::execute(fixture.niri_state(), "bindsym Mod4+c unbindsym Mod4+c",)[0]
            .success
    );
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 54, true);
    key_event(&mut fixture, 54, false);
    key_event(&mut fixture, 133, false);
    let outcome = crate::command::execute(fixture.niri_state(), "unbindsym Mod4+c");
    assert!(
        !outcome[0].success,
        "the self-unbinding bind should already be gone: {outcome:?}"
    );
}

#[test]
fn runtime_bindswitch_fires_unbinds_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch lid:on workspace switch-bound",
        )[0]
        .success
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-bound")
    );

    assert!(crate::command::execute(fixture.niri_state(), "unbindswitch lid:on")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace switch-unbound")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-unbound")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch lid:on workspace should-not-survive",
        )[0]
        .success
    );
    fixture
        .niri_state()
        .reload_config(Ok(swayward_config::Config::default()));
    assert!(crate::command::execute(fixture.niri_state(), "workspace switch-reload")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("switch-reload")
    );
}

#[test]
fn runtime_bindswitch_respects_mode_and_toggle_trigger() {
    let config =
        swayward_config::Config::parse_mem(r#"mode "switch-mode" { x { command "nop"; }; }"#)
            .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode switch-mode")[0].success);
    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "bindswitch tablet:toggle workspace toggled",
        )[0]
        .success
    );

    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("toggled")
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace before-off")[0].success);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::Off,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("toggled")
    );

    // The mode-local binding is inactive in default mode.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    assert!(
        crate::command::execute(fixture.niri_state(), "workspace default-switch-mode")[0].success
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::TabletMode,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-switch-mode")
    );
}

#[test]
fn runtime_bindswitch_refuses_to_shadow_a_narrower_kdl_switch_event() {
    let config = swayward_config::Config::parse_mem(
        r#"
switch-events {
    lid-close { spawn "true"; }
}
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "bindswitch lid:on workspace would-shadow",
    );
    assert!(!outcome[0].success, "{outcome:?}");
    assert_eq!(
        outcome[0].error.as_deref(),
        Some("runtime switch binding conflicts with a narrower KDL switch-event binding")
    );
    assert!(fixture.swayward().runtime_switch_bindings.is_empty());
}

/// Gesture binds remain honest failures: gesture events have no sway command
/// binding table or matching path.
#[test]
fn unsupported_runtime_gesture_binds_do_not_mutate_key_table() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));
    let before = fixture.swayward().config.borrow().binds.0.clone();

    for command in ["bindgesture swipe:3:left nop", "unbindgesture swipe:3:left"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(!outcome[0].success, "{command}: {outcome:?}");
    }
    assert_eq!(fixture.swayward().config.borrow().binds.0, before);
}

#[test]
fn numlock_qualified_binding_dispatches_only_while_numlock_is_active() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Num+a { command "rename workspace to numlocked"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_none());

    key_event(&mut fixture, 77, true);
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .num_lock
    );
    key_event(&mut fixture, 77, false);
    assert!(
        fixture
            .swayward()
            .seat
            .get_keyboard()
            .unwrap()
            .modifier_state()
            .num_lock
    );
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_some());
}

#[test]
fn unqualified_binding_dispatches_while_numlock_is_active() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Mod4+a { command "rename workspace to numlocked"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 77, true);
    key_event(&mut fixture, 77, false);
    key_event(&mut fixture, 133, true);
    key_event(&mut fixture, 38, true);
    key_event(&mut fixture, 38, false);
    key_event(&mut fixture, 133, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("numlocked")
        .is_some());
}

#[test]
fn modifier_bindcode_matches_without_its_own_modifier() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { "code:133" release=true { command "rename workspace to super-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 133, true);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("super-release")
        .is_none());
    key_event(&mut fixture, 133, false);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("super-release")
        .is_some());
}

/// Waybar tracks the focused workspace from the `workspace` event stream, not
/// by polling GET_WORKSPACES. Jumping to a workspace from the overview changes
/// the active workspace through `toggle_overview_to_workspace`, which is not a
/// command dispatch, so nothing on that path told the event stream anything
/// had happened and every bar kept highlighting the workspace the user left.
#[test]
fn overview_workspace_jump_emits_a_workspace_focus_event() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));

    for command in ["workspace 2", "workspace 1"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((msg_type, payload), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());
    assert_eq!(msg_type, MessageType::Subscribe as u32);
    assert_eq!(payload, r#"{"success": true}"#);

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let before = active_idx(&mut fixture);

    // Exactly what a click on another workspace in the overview does.
    assert!(fixture.swayward().layout.open_overview());
    fixture
        .swayward()
        .layout
        .toggle_overview_to_workspace(before + 1);
    fixture.niri_state().refresh_and_flush_clients();

    let after = active_idx(&mut fixture);
    assert_eq!(
        after,
        before + 1,
        "the overview jump must change the active workspace"
    );

    let ((event_type, payload), _) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    assert_eq!(event_type, 1 << 31, "expected a workspace event");
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(
        event["change"], "focus",
        "a bar learns the workspace changed only from this event: {event}"
    );
}

/// Waybar answers a workspace event by immediately re-reading GET_TREE and
/// rendering whatever that reply says
/// (Waybar/src/modules/sway/workspaces.cpp:107-113,146-172). So the tree the
/// server is holding at the moment it emits the event is the tree the bar
/// draws.
///
/// ipc_refresh_layout emits from ipc_refresh_workspaces first and only then
/// calls refresh_query_state, so for that window query_state.tree still
/// describes the workspace the user left.
#[test]
fn query_state_tree_is_current_when_a_workspace_event_is_emitted() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));
    for command in ["workspace 2", "workspace 1"] {
        crate::command::execute(fixture.niri_state(), command);
    }
    fixture.niri_state().refresh_and_flush_clients();

    // Subscribe first: this client is the bar, and it must not see a stale
    // tree after being told the workspace changed.
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let before = fixture
        .swayward()
        .layout
        .monitor_for_output(&output)
        .unwrap()
        .active_workspace_idx();

    assert!(fixture.swayward().layout.open_overview());
    fixture
        .swayward()
        .layout
        .toggle_overview_to_workspace(before + 1);

    fixture.niri_state().ipc_refresh_layout();

    // Wait for the event, then query exactly as waybar does on receiving it.
    let ((_, payload), _) = read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder);
    let event = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(event["change"], "focus", "expected a focus event: {event}");

    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut workspaces);
    let focused = workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "the cached GET_TREE served to a bar that just saw the event still \
         names the old workspace: {tree:#}"
    );
}

/// Arrows inside the overview change the active workspace while the overview
/// is still open. A bar must track that immediately, not only once the
/// overview closes.
#[test]
fn overview_arrow_emits_focus_event_before_the_overview_closes() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));
    for command in ["workspace 2", "workspace 1"] {
        crate::command::execute(fixture.niri_state(), command);
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    key_event(&mut fixture, 116, true);
    key_event(&mut fixture, 116, false);
    fixture.niri_state().refresh_and_flush_clients();

    let mut changes = Vec::new();
    while let Some(((_, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        changes.push((
            event["change"].as_str().unwrap_or_default().to_owned(),
            event["current"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    assert!(
        changes
            .iter()
            .any(|(change, name)| change == "focus" && name == "2"),
        "the bar must learn about the new workspace while the overview is \
         still open, got {changes:?}"
    );

    // And GET_TREE, which is what waybar actually renders, must agree.
    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut workspaces);
    let focused = workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "GET_TREE must mark the arrowed-to workspace focused while the \
         overview is open: {tree:#}"
    );
}

/// The keyboard route into the same jump: open the overview, arrow to another
/// workspace, then Escape to leave. A bar must end up highlighting the
/// workspace the user landed on.
#[test]
fn overview_arrow_then_escape_emits_workspace_focus_events() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(test_socket_path())).unwrap();
    let socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.add_output(1, (1280, 720));

    for command in ["workspace 2", "workspace 1"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }

    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["workspace"]"#,
        ))
        .unwrap();
    let ((_, _), mut remainder) =
        read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, Vec::new());

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let before = active_idx(&mut fixture);

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();

    // Down arrow, then Escape to close the overview.
    for key in [116, 1] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
    }
    fixture.niri_state().refresh_and_flush_clients();

    assert_eq!(
        active_idx(&mut fixture),
        before + 1,
        "arrow then escape must leave the new workspace active"
    );

    let mut changes = Vec::new();
    while let Some(((event_type, payload), rest)) =
        try_read_ipc_reply_with_remainder(&mut fixture, &mut subscriber, remainder.clone())
    {
        remainder = rest;
        assert_eq!(event_type, 1 << 31);
        let event = serde_json::from_str::<Value>(&payload).unwrap();
        changes.push((
            event["change"].as_str().unwrap_or_default().to_owned(),
            event["current"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        ));
    }
    assert!(
        changes
            .iter()
            .any(|(change, name)| change == "focus" && name == "2"),
        "a bar must be told workspace 2 is focused, got {changes:?}"
    );

    // Waybar ignores the event payload entirely and re-reads GET_TREE, then
    // reads `focused` and `visible` off the workspace nodes
    // (Waybar/src/modules/sway/workspaces.cpp:88-113,146-172,307-321,363-371).
    // The event is only the trigger; GET_TREE is what the bar renders.
    let mut query = UnixStream::connect(&socket).unwrap();
    let tree = query_ipc(&mut fixture, &mut query, MessageType::GetTree);
    let mut tree_workspaces = Vec::new();
    collect_workspace_nodes(&tree, &mut tree_workspaces);
    let tree_focused = tree_workspaces
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        tree_focused,
        vec!["2".to_string()],
        "GET_TREE is what waybar renders, and it must mark the new workspace \
         focused: {tree:#}"
    );

    let mut query = UnixStream::connect(&socket).unwrap();
    let workspaces = query_ipc(&mut fixture, &mut query, MessageType::GetWorkspaces);
    let focused = workspaces
        .as_array()
        .unwrap()
        .iter()
        .filter(|ws| ws["focused"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        focused,
        vec!["2".to_string()],
        "GET_WORKSPACES must agree with the focus event: {workspaces}"
    );
    let visible = workspaces
        .as_array()
        .unwrap()
        .iter()
        .filter(|ws| ws["visible"] == true)
        .map(|ws| ws["name"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        visible,
        vec!["2".to_string()],
        "the workspace the user landed on must be the visible one: {workspaces}"
    );
}

#[test]
fn overview_arrow_keys_move_between_workspaces() {
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 2", "workspace 1", "split vertical"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    for _ in 0..2 {
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    let first_workspace = active_workspace_idx(&mut fixture);
    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();
    assert!(
        fixture.swayward().keyboard_focus.is_overview(),
        "overview opened with keyboard focus {:?}",
        fixture.swayward().keyboard_focus
    );

    for (key, expected_workspace) in [(116, first_workspace + 1), (111, first_workspace)] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
        assert_eq!(
            active_workspace_idx(&mut fixture),
            expected_workspace,
            "keycode {key} did not focus workspace index {expected_workspace}"
        );
    }
}

#[test]
fn overview_arrow_keys_wrap_at_the_ends() {
    // The overview shows the whole stack at once, so an arrow that stops dead
    // at the last workspace reads as a broken key rather than as an edge. With
    // only two workspaces one of the two arrows always looked dead, which is
    // how this was reported. Sway's own `workspace next` wraps.
    let config = swayward_config::Config::parse_mem(
        r#"workspace "1" {}
workspace "2" {}"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    let client = fixture.add_client();

    for command in ["workspace 1", "workspace 2"] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
        let window = fixture.client(client).create_window();
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }

    let output = fixture.swayward().layout.active_output().unwrap().clone();
    let active_workspace_idx = |fixture: &mut Fixture| {
        fixture
            .swayward()
            .layout
            .monitor_for_output(&output)
            .unwrap()
            .active_workspace_idx()
    };
    // The monitor also holds a trailing unnamed workspace, so the wrap target
    // is the last index rather than the last *named* one. Drive to index 0
    // first so the wrap is unambiguous.
    crate::command::execute(fixture.niri_state(), "workspace 1");
    // Let the workspace-switch animation finish: the wrapping helpers defer to
    // the plain clamped ones while a switch is in flight.
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.swayward().layout.advance_animations();
    fixture.swayward().clock.set_complete_instantly(false);
    assert_eq!(active_workspace_idx(&mut fixture), 0);
    let last = fixture.swayward().layout.workspaces().count() - 1;

    assert!(fixture.swayward().layout.open_overview());
    fixture.niri_state().update_keyboard_focus();

    // Up from the first workspace wraps to the last, and Down from the last
    // wraps back to the first.
    for (key, expected) in [(111, last), (116, 0)] {
        key_event(&mut fixture, key, true);
        key_event(&mut fixture, key, false);
        // Settle the switch animation: a wrap issued mid-switch falls back to
        // the clamped helper and would test the wrong thing.
        fixture.swayward().clock.set_complete_instantly(true);
        fixture.swayward().layout.advance_animations();
        fixture.swayward().clock.set_complete_instantly(false);
        assert_eq!(
            active_workspace_idx(&mut fixture),
            expected,
            "keycode {key} did not wrap to workspace index {expected}"
        );
    }
}

#[test]
fn ordinary_modified_keysym_bind_still_matches() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+Return { command "rename workspace to modified"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[133, 36]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("modified")
        .is_some());
}

#[test]
fn translated_keysym_uses_post_transition_consumed_modifiers() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Alt+at { command "rename workspace to translated"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    type_key_chords(&mut fixture, &[&[64, 50, 11]]);
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("translated")
        .is_some());
}

#[test]
fn bindcode_uses_the_xkb_keycode_from_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { "code:39" { command "rename workspace to bindcode"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("bindcode")
        .is_some());
}

#[test]
fn release_key_binding_dispatches_only_on_release_through_real_input() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    key_event(&mut fixture, 53, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn another_key_cancels_a_held_release_binding_without_an_ipc_event() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { x release=true { command "nop release"; }; }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1280, 720));
    let mut subscriber = UnixStream::connect(socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    key_event(&mut fixture, 53, true);
    key_event(&mut fixture, 52, true);
    key_event(&mut fixture, 53, false);
    key_event(&mut fixture, 52, false);
    fixture.swayward().ipc_server.as_ref().unwrap().send_event(
        swayward_ipc::legacy::Event::SwayBinding {
            command: "sentinel".into(),
            event_state_mask: vec![],
            input_codes: vec![],
            input_code: 0,
            symbols: vec!["t".into()],
            symbol: Some("t".into()),
            input_type: "keyboard".into(),
        },
    );

    let mut commands = Vec::new();
    loop {
        let (message_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
        assert_eq!(message_type, (1 << 31) | 5);
        let command = serde_json::from_str::<Value>(&payload).unwrap()["binding"]["command"]
            .as_str()
            .unwrap()
            .to_owned();
        commands.push(command.clone());
        if command == "sentinel" {
            break;
        }
    }
    assert_eq!(commands, ["sentinel"]);
}

#[test]
fn release_key_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            x { command "mode other"; }
            x release=true { command "workspace key-released"; }
        }
        mode "other" { y { command "nop"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    key_event(&mut fixture, 53, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_survives_config_reload_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace key-released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    super::i3_conformance::reload_test_config(&mut fixture, "font monospace\n").unwrap();
    assert_eq!(fixture.swayward().binding_mode, "default");
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("key-released")
        .is_some());
}

#[test]
fn release_key_binding_is_not_replaced_by_the_new_modes_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"mode "held" { x release=true { command "workspace original-release"; }; }
        mode "other" { x release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));
    assert!(crate::command::execute(fixture.niri_state(), "mode held")[0].success);

    key_event(&mut fixture, 53, true);
    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    key_event(&mut fixture, 53, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("original-release")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_survives_mode_change_after_press() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            MouseLeft { command "mode other"; }
            MouseLeft release=true { command "workspace mouse-released"; }
        }
        mode "other" { MouseLeft release=true { command "workspace wrong-release"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_eq!(fixture.swayward().binding_mode, "other");
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("mouse-released")
        .is_some());
    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("wrong-release")
        .is_none());
}

#[test]
fn release_mouse_binding_dispatches_only_on_release() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { MouseLeft release=true { command "rename workspace to released"; }; }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    assert_ne!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );

    pointer_button(&mut fixture, 0x110, false);
    assert_eq!(
        active_workspace_name(&mut fixture),
        Some("released".to_owned())
    );
}

#[test]
fn pointer_button_event_dispatches_a_real_mouse_binding() {
    let config = swayward_config::Config::parse_mem(
        r#"binds {
            X { command "workspace startup"; }
            MouseLeft { command "workspace clicked"; }
        }"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    pointer_button(&mut fixture, 0x110, true);
    pointer_button(&mut fixture, 0x110, false);

    assert!(fixture
        .swayward()
        .layout
        .find_workspace_by_name("clicked")
        .is_some());
}

#[test]
fn binding_modes_switch_binds_emit_events_and_list_over_ipc() {
    let config = swayward_config::Config::parse_mem(
        r#"binds { Super+R { command "mode resize"; }; }
        mode "resize" {
            Super+1 { command "workspace 7"; };
            Escape { command "mode default"; };
        }"#,
    )
    .unwrap();
    let (mut fixture, socket) = ipc_fixture();
    *fixture.swayward().config.borrow_mut() = config;
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode","binding"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    let expected: Value = serde_json::from_str(sway_fixture!("events/mode.resize.json")).unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    type_key_chords(&mut fixture, &[&[133, 10]]);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 5);
    let expected: Value = serde_json::from_str(sway_fixture!("events/binding.run.json")).unwrap();
    assert_event_shape(
        &expected,
        &serde_json::from_str(&payload).unwrap(),
        "$binding",
    );

    let swayward = fixture.swayward();
    // The bind switched to workspace 7, which is what this asserts. It is not
    // necessarily first: sway sorts numbered workspaces numerically
    // (sway/sway/tree/output.c:387-405), so the startup workspace 1 precedes it.
    assert!(
        describe_workspaces(&swayward.layout, &swayward.global_space)
            .iter()
            .any(|workspace| workspace.num == 7 && workspace.focused)
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    let expected: Value = serde_json::from_str(sway_fixture!("events/mode.default.json")).unwrap();
    assert_event_shape(&expected, &serde_json::from_str(&payload).unwrap(), "$mode");

    let mut query = UnixStream::connect(socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "resize"])
    );

    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"name": "default"})
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode resize")[0].success);
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingState,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    let state = serde_json::from_str::<Value>(&payload).unwrap();
    assert_eq!(state, serde_json::json!({"name": "resize"}));
    assert_eq!(state.as_object().unwrap().len(), 1);
}

#[test]
fn runtime_mode_definition_with_set_creates_a_switchable_pango_mode() {
    let (mut fixture, socket) = ipc_fixture();
    fixture.add_output(1, (1920, 1080));
    let mut subscriber = UnixStream::connect(&socket).unwrap();
    subscriber
        .write_all(&swayward_ipc::wire::encode(
            MessageType::Subscribe,
            r#"["mode"]"#,
        ))
        .unwrap();
    let (_, reply) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(reply, r#"{"success": true}"#);

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode --pango_markup created set $destination workspace-7",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "default");
    assert_eq!(
        fixture.swayward().sway_variables,
        [("$destination".into(), "workspace-7".into())]
    );
    let config = fixture.swayward().config.borrow();
    let created = config
        .binding_modes
        .iter()
        .find(|mode| mode.name == "created")
        .unwrap();
    assert!(created.pango_markup);
    assert!(created.binds.0.is_empty());
    drop(config);

    let mut query = UnixStream::connect(&socket).unwrap();
    query
        .write_all(&swayward_ipc::wire::encode(
            MessageType::GetBindingModes,
            "",
        ))
        .unwrap();
    let (_, payload) = read_ipc_reply(&mut fixture, &mut query);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!(["default", "created"])
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode created");
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    let (event_type, payload) = read_ipc_reply(&mut fixture, &mut subscriber);
    assert_eq!(event_type, (1 << 31) | 2);
    assert_eq!(
        serde_json::from_str::<Value>(&payload).unwrap(),
        serde_json::json!({"change":"created","pango_markup":true})
    );

    assert!(crate::command::execute(fixture.niri_state(), "workspace $destination")[0].success);
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-7".into())
    );

    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode inline set $next workspace-8; workspace $next",
    );
    assert!(outcome.iter().all(|result| result.success), "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "created");
    assert_eq!(
        fixture
            .swayward()
            .layout
            .active_workspace()
            .unwrap()
            .sway_name(),
        Some("workspace-8".into())
    );

    let outcome = crate::command::execute(fixture.niri_state(), "mode missing");
    assert!(!outcome[0].success);
    assert_eq!(outcome[0].error.as_deref(), Some("Unknown mode `missing'"));
}

/// Gesture binds are the only nested subcommands still refused, top level or
/// nested: swayward has no gesture command-binding table. The refusal happens
/// at parse time, so the named mode is not created either.
#[test]
fn runtime_mode_definition_rejects_only_gesture_binding_subcommands() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1920, 1080));

    for subcommand in ["bindgesture swipe:3:left nop", "unbindgesture swipe:3:left"] {
        let command = format!("mode blocked {subcommand}");
        let outcome = crate::command::execute(fixture.niri_state(), &command);
        assert!(!outcome[0].success, "{command}");
        assert_eq!(outcome[0].parse_error, Some(true), "{command}");
        assert_eq!(
            outcome[0].error.as_deref(),
            Some("gesture events have no sway command-binding model")
        );
    }
    assert!(fixture
        .swayward()
        .config
        .borrow()
        .binding_modes
        .iter()
        .all(|mode| mode.name != "blocked"));
}

/// `mode <name> bindsym` inserts into the named mode without switching to it
/// (`sway/sway/commands/mode.c:69-84`). The binding must fire once that mode
/// is active, stay silent in the default mode, and be discarded by reload.
#[test]
fn runtime_nested_mode_bindsym_fires_only_in_that_mode_and_is_discarded_by_reload() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    let added = crate::command::execute(
        fixture.niri_state(),
        "mode nested bindsym Mod4+a workspace nested-bound",
    );
    assert!(added[0].success, "{added:?}");
    // The nested form defines the mode but does not enter it.
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Inactive in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace still-default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("still-default")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-bound")
    );

    // Unbinding through the nested form removes it again.
    let removed = crate::command::execute(fixture.niri_state(), "mode nested unbindsym Mod4+a");
    assert!(removed[0].success, "{removed:?}");
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );

    assert!(
        crate::command::execute(
            fixture.niri_state(),
            "mode nested bindsym Mod4+a workspace should-not-survive",
        )[0]
        .success
    );
    let config =
        swayward_config::Config::parse_mem(r#"mode "nested" { x { command "nop"; }; }"#).unwrap();
    fixture.niri_state().reload_config(Ok(config));
    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    assert!(crate::command::execute(fixture.niri_state(), "workspace reload-check")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("reload-check")
    );
}

/// The keycode and switch nested forms reach the same tables as their
/// top-level counterparts, and target the named mode rather than the active
/// one.
#[test]
fn runtime_nested_mode_bindcode_and_bindswitch_target_the_named_mode() {
    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 720));

    for command in [
        "mode nested bindcode 39 workspace nested-code",
        "mode nested bindswitch lid:on workspace nested-switch",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert_eq!(fixture.swayward().binding_mode, "default");

    // Neither fires in the default mode.
    assert!(crate::command::execute(fixture.niri_state(), "workspace default-still")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-still")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode nested")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-code")
    );
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-switch")
    );

    for command in [
        "mode nested unbindcode 39",
        "mode nested unbindswitch lid:on",
    ] {
        let outcome = crate::command::execute(fixture.niri_state(), command);
        assert!(outcome[0].success, "{command}: {outcome:?}");
    }
    assert!(crate::command::execute(fixture.niri_state(), "workspace nested-unbound")[0].success);
    key_event(&mut fixture, 39, true);
    key_event(&mut fixture, 39, false);
    switch_event(
        &mut fixture,
        smithay::backend::input::Switch::Lid,
        smithay::backend::input::SwitchState::On,
    );
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("nested-unbound")
    );
}

/// A nested bind aimed at the mode the compositor is already in must not
/// leak into the default table, and a nested bind issued while a mode is
/// active must land in the named mode rather than the active one.
#[test]
fn runtime_nested_mode_bindsym_ignores_the_active_mode() {
    let config = swayward_config::Config::parse_mem(
        r#"
binds { Mod4+a { command "workspace default-mode"; }; }
mode "other" { Mod4+b { command "nop"; }; }
"#,
    )
    .unwrap();
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 720));

    assert!(crate::command::execute(fixture.niri_state(), "mode other")[0].success);
    let outcome = crate::command::execute(
        fixture.niri_state(),
        "mode elsewhere bindsym Mod4+a workspace elsewhere-bound",
    );
    assert!(outcome[0].success, "{outcome:?}");
    assert_eq!(fixture.swayward().binding_mode, "other");

    // The active mode did not receive the binding.
    assert!(crate::command::execute(fixture.niri_state(), "workspace untouched")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("untouched")
    );

    // Neither did the default table.
    assert!(crate::command::execute(fixture.niri_state(), "mode default")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("default-mode")
    );

    assert!(crate::command::execute(fixture.niri_state(), "mode elsewhere")[0].success);
    press_mod_a(&mut fixture);
    assert_eq!(
        active_workspace_name(&mut fixture).as_deref(),
        Some("elsewhere-bound")
    );
}

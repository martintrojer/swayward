use std::collections::HashSet;
use std::sync::Mutex;

use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisRelativeDirection, AxisSource, ButtonState, Device,
    DeviceCapability, Event, InputBackend, InputTime, PointerAxisEvent, PointerButtonEvent,
    PointerMotionAbsoluteEvent, PointerMotionEvent, UnusedEvent,
};
use smithay::input::pointer::AxisFrame;
use smithay::output::Output;
use smithay::reexports::wayland_protocols_wlr;
use smithay::reexports::wayland_server::protocol::wl_pointer;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use wayland_backend::protocol::WEnum;
use wayland_protocols_wlr::virtual_pointer::v1::server::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};
use zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1;
use zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1;

use crate::protocols::EmptyData;

const VERSION: u32 = 2;

pub struct VirtualPointerManagerState {
    virtual_pointers: HashSet<ZwlrVirtualPointerV1>,
}

pub struct VirtualPointerManagerGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

pub struct VirtualPointerInputBackend;

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct VirtualPointer {
    pointer: ZwlrVirtualPointerV1,
}

#[derive(Debug)]
pub struct VirtualPointerUserData {
    seat: Option<WlSeat>,
    output: Option<Output>,

    axis_frame: Mutex<PendingAxisFrame>,
}

#[derive(Debug, Default)]
struct PendingAxisFrame {
    frame: Option<AxisFrame>,
    source: Option<AxisSource>,
}

impl PendingAxisFrame {
    fn finish(&mut self) -> Option<AxisFrame> {
        self.source = None;
        self.frame.take()
    }

    fn mutate(&mut self, time: Option<u32>, f: impl FnOnce(AxisFrame) -> AxisFrame) {
        let source = self.source;
        self.frame = self
            .frame
            .or(time.map(InputTime::from_millis).map(AxisFrame::new))
            .map(|frame| source.map_or(frame, |source| frame.source(source)))
            .map(f);
    }

    fn set_source(&mut self, source: AxisSource) {
        self.source = Some(source);
        if let Some(frame) = self.frame.take() {
            self.frame = Some(frame.source(source));
        }
    }
}

impl VirtualPointer {
    fn data(&self) -> &VirtualPointerUserData {
        self.pointer.data().unwrap()
    }

    pub fn seat(&self) -> Option<&WlSeat> {
        self.data().seat.as_ref()
    }

    pub fn output(&self) -> Option<&Output> {
        self.data().output.as_ref()
    }

    fn finish_axis_frame(&self) -> Option<AxisFrame> {
        self.data().axis_frame.lock().unwrap().finish()
    }

    fn mutate_axis_frame(&self, time: Option<u32>, f: impl FnOnce(AxisFrame) -> AxisFrame) {
        self.data().axis_frame.lock().unwrap().mutate(time, f);
    }
}

impl Device for VirtualPointer {
    fn id(&self) -> String {
        format!("wlr virtual pointer {}", self.pointer.id())
    }

    fn name(&self) -> String {
        String::from("virtual pointer")
    }

    fn has_capability(&self, capability: DeviceCapability) -> bool {
        matches!(capability, DeviceCapability::Pointer)
    }

    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }

    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

pub struct VirtualPointerMotionEvent {
    pointer: VirtualPointer,
    time: u32,
    dx: f64,
    dy: f64,
}

impl Event<VirtualPointerInputBackend> for VirtualPointerMotionEvent {
    fn time(&self) -> InputTime {
        InputTime::from_millis(self.time)
    }

    fn device(&self) -> VirtualPointer {
        self.pointer.clone()
    }
}

impl PointerMotionEvent<VirtualPointerInputBackend> for VirtualPointerMotionEvent {
    fn delta_x(&self) -> f64 {
        self.dx
    }

    fn delta_y(&self) -> f64 {
        self.dy
    }

    fn delta_x_unaccel(&self) -> f64 {
        self.dx
    }

    fn delta_y_unaccel(&self) -> f64 {
        self.dy
    }
}

pub struct VirtualPointerMotionAbsoluteEvent {
    pointer: VirtualPointer,
    time: u32,
    x: u32,
    y: u32,
    x_extent: u32,
    y_extent: u32,
}

impl Event<VirtualPointerInputBackend> for VirtualPointerMotionAbsoluteEvent {
    fn time(&self) -> InputTime {
        InputTime::from_millis(self.time)
    }

    fn device(&self) -> VirtualPointer {
        self.pointer.clone()
    }
}

fn normalized_absolute_coordinate(value: u32, extent: u32) -> Option<f64> {
    (extent != 0).then(|| value as f64 / extent as f64)
}

impl AbsolutePositionEvent<VirtualPointerInputBackend> for VirtualPointerMotionAbsoluteEvent {
    fn x(&self) -> f64 {
        normalized_absolute_coordinate(self.x, self.x_extent).unwrap_or_default()
    }

    fn y(&self) -> f64 {
        normalized_absolute_coordinate(self.y, self.y_extent).unwrap_or_default()
    }

    fn x_transformed(&self, width: i32) -> f64 {
        self.x() * f64::from(width)
    }

    fn y_transformed(&self, height: i32) -> f64 {
        self.y() * f64::from(height)
    }
}

pub struct VirtualPointerButtonEvent {
    pointer: VirtualPointer,
    time: u32,
    button: u32,
    state: ButtonState,
}

impl Event<VirtualPointerInputBackend> for VirtualPointerButtonEvent {
    fn time(&self) -> InputTime {
        InputTime::from_millis(self.time)
    }

    fn device(&self) -> VirtualPointer {
        self.pointer.clone()
    }
}

impl PointerButtonEvent<VirtualPointerInputBackend> for VirtualPointerButtonEvent {
    fn button_code(&self) -> u32 {
        self.button
    }

    fn state(&self) -> ButtonState {
        self.state
    }
}

pub struct VirtualPointerAxisEvent {
    pointer: VirtualPointer,
    frame: AxisFrame,
}

impl Event<VirtualPointerInputBackend> for VirtualPointerAxisEvent {
    fn time(&self) -> InputTime {
        self.frame.time
    }

    fn device(&self) -> VirtualPointer {
        self.pointer.clone()
    }
}

fn tuple_axis<T>(tuple: (T, T), axis: Axis) -> T {
    match axis {
        Axis::Horizontal => tuple.0,
        Axis::Vertical => tuple.1,
    }
}

fn axis_source_or_default(source: Option<AxisSource>) -> AxisSource {
    source.unwrap_or(AxisSource::Wheel)
}

fn discrete_to_v120(discrete: i32) -> i32 {
    discrete.saturating_mul(120)
}

impl PointerAxisEvent<VirtualPointerInputBackend> for VirtualPointerAxisEvent {
    fn amount(&self, axis: Axis) -> Option<f64> {
        Some(tuple_axis(self.frame.axis, axis))
    }

    fn amount_v120(&self, axis: Axis) -> Option<f64> {
        self.frame.v120.map(|v120| tuple_axis(v120, axis) as f64)
    }

    fn source(&self) -> AxisSource {
        axis_source_or_default(self.frame.source)
    }

    fn relative_direction(&self, axis: Axis) -> AxisRelativeDirection {
        tuple_axis(self.frame.relative_direction, axis)
    }
}

impl PointerMotionAbsoluteEvent<VirtualPointerInputBackend> for VirtualPointerMotionAbsoluteEvent {}

impl InputBackend for VirtualPointerInputBackend {
    type Device = VirtualPointer;

    type KeyboardKeyEvent = UnusedEvent;
    type PointerAxisEvent = VirtualPointerAxisEvent;
    type PointerButtonEvent = VirtualPointerButtonEvent;
    type PointerMotionEvent = VirtualPointerMotionEvent;
    type PointerMotionAbsoluteEvent = VirtualPointerMotionAbsoluteEvent;

    type GestureSwipeBeginEvent = UnusedEvent;
    type GestureSwipeUpdateEvent = UnusedEvent;
    type GestureSwipeEndEvent = UnusedEvent;
    type GesturePinchBeginEvent = UnusedEvent;
    type GesturePinchUpdateEvent = UnusedEvent;
    type GesturePinchEndEvent = UnusedEvent;
    type GestureHoldBeginEvent = UnusedEvent;
    type GestureHoldEndEvent = UnusedEvent;

    type TouchDownEvent = UnusedEvent;
    type TouchUpEvent = UnusedEvent;
    type TouchMotionEvent = UnusedEvent;
    type TouchCancelEvent = UnusedEvent;
    type TouchFrameEvent = UnusedEvent;
    type TabletToolAxisEvent = UnusedEvent;
    type TabletToolProximityEvent = UnusedEvent;
    type TabletToolTipEvent = UnusedEvent;
    type TabletToolButtonEvent = UnusedEvent;

    type SwitchToggleEvent = UnusedEvent;

    type SpecialEvent = UnusedEvent;
}

pub trait VirtualPointerHandler {
    fn virtual_pointer_manager_state(&mut self) -> &mut VirtualPointerManagerState;

    fn create_virtual_pointer(&mut self, pointer: VirtualPointer) {
        let _ = pointer;
    }
    fn destroy_virtual_pointer(&mut self, pointer: VirtualPointer) {
        let _ = pointer;
    }

    fn on_virtual_pointer_motion(&mut self, event: VirtualPointerMotionEvent);
    fn on_virtual_pointer_motion_absolute(&mut self, event: VirtualPointerMotionAbsoluteEvent);
    fn on_virtual_pointer_button(&mut self, event: VirtualPointerButtonEvent);
    fn on_virtual_pointer_axis(&mut self, event: VirtualPointerAxisEvent);
}

impl VirtualPointerManagerState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where
        D: GlobalDispatch<ZwlrVirtualPointerManagerV1, VirtualPointerManagerGlobalData>,
        D: VirtualPointerHandler,
        D: 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let global_data = VirtualPointerManagerGlobalData {
            filter: Box::new(filter),
        };
        display.create_global::<D, ZwlrVirtualPointerManagerV1, _>(VERSION, global_data);

        Self {
            virtual_pointers: HashSet::new(),
        }
    }
}

impl<D> GlobalDispatch2<ZwlrVirtualPointerManagerV1, D> for VirtualPointerManagerGlobalData
where
    D: Dispatch<ZwlrVirtualPointerManagerV1, EmptyData>,
    D: 'static,
{
    fn bind(
        &self,
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        manager: New<ZwlrVirtualPointerManagerV1>,
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(manager, EmptyData);
    }

    fn can_view(&self, client: &Client) -> bool {
        (self.filter)(client)
    }
}

impl<D> Dispatch2<ZwlrVirtualPointerManagerV1, D> for EmptyData
where
    D: Dispatch<ZwlrVirtualPointerV1, VirtualPointerUserData>,
    D: VirtualPointerHandler,
    D: 'static,
{
    fn request(
        &self,
        state: &mut D,
        _client: &Client,
        _resource: &ZwlrVirtualPointerManagerV1,
        request: <ZwlrVirtualPointerManagerV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        let (id, seat, output) = match request {
            zwlr_virtual_pointer_manager_v1::Request::CreateVirtualPointer { seat, id } => {
                (id, seat, None)
            }
            zwlr_virtual_pointer_manager_v1::Request::CreateVirtualPointerWithOutput {
                seat,
                output,
                id,
            } => (id, seat, output.as_ref().and_then(Output::from_resource)),
            zwlr_virtual_pointer_manager_v1::Request::Destroy => return,
            _ => unreachable!(),
        };

        let pointer = data_init.init(
            id,
            VirtualPointerUserData {
                seat,
                output,
                axis_frame: Mutex::new(PendingAxisFrame::default()),
            },
        );
        state
            .virtual_pointer_manager_state()
            .virtual_pointers
            .insert(pointer.clone());

        state.create_virtual_pointer(VirtualPointer { pointer });
    }
}

impl<D> Dispatch2<ZwlrVirtualPointerV1, D> for VirtualPointerUserData
where
    D: VirtualPointerHandler,
    D: 'static,
{
    fn request(
        &self,
        handler: &mut D,
        _client: &Client,
        resource: &ZwlrVirtualPointerV1,
        request: <ZwlrVirtualPointerV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        let pointer = VirtualPointer {
            pointer: resource.clone(),
        };
        match request {
            zwlr_virtual_pointer_v1::Request::Motion { time, dx, dy } => {
                let event = VirtualPointerMotionEvent {
                    pointer,
                    time,
                    dx,
                    dy,
                };
                handler.on_virtual_pointer_motion(event);
            }
            zwlr_virtual_pointer_v1::Request::MotionAbsolute {
                time,
                x,
                y,
                x_extent,
                y_extent,
            } => {
                if x_extent == 0 || y_extent == 0 {
                    return;
                }
                let event = VirtualPointerMotionAbsoluteEvent {
                    pointer,
                    time,
                    x,
                    y,
                    x_extent,
                    y_extent,
                };
                handler.on_virtual_pointer_motion_absolute(event);
            }
            zwlr_virtual_pointer_v1::Request::Button {
                time,
                button,
                state,
            } => {
                // state is an enum but wlroots treats it as a C boolean (zero or nonzero)
                // so we emulate that behaviour too. ButtonState::Pressed and any invalid value
                // counts as pressed.
                // https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/3187479c07c34a4de82c06a316a763a36a0499da/types/wlr_virtual_pointer_v1.c#L74
                let state = match state {
                    WEnum::Value(wl_pointer::ButtonState::Released) => ButtonState::Released,
                    _ => ButtonState::Pressed,
                };
                let event = VirtualPointerButtonEvent {
                    pointer,
                    time,
                    button,
                    state,
                };
                handler.on_virtual_pointer_button(event);
            }
            zwlr_virtual_pointer_v1::Request::Axis { time, axis, value } => {
                let axis = match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => Axis::Vertical,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => Axis::Horizontal,
                    _ => {
                        warn!("Axis: invalid axis");
                        resource.post_error(
                            zwlr_virtual_pointer_v1::Error::InvalidAxis,
                            "invalid axis",
                        );
                        return;
                    }
                };

                pointer.mutate_axis_frame(Some(time), |frame| frame.value(axis, value));
            }
            zwlr_virtual_pointer_v1::Request::Frame => {
                if let Some(frame) = pointer.finish_axis_frame() {
                    let event = VirtualPointerAxisEvent { pointer, frame };
                    handler.on_virtual_pointer_axis(event);
                }
            }
            zwlr_virtual_pointer_v1::Request::AxisSource { axis_source } => {
                let axis_source = match axis_source {
                    WEnum::Value(wl_pointer::AxisSource::Wheel) => AxisSource::Wheel,
                    WEnum::Value(wl_pointer::AxisSource::Finger) => AxisSource::Finger,
                    WEnum::Value(wl_pointer::AxisSource::Continuous) => AxisSource::Continuous,
                    WEnum::Value(wl_pointer::AxisSource::WheelTilt) => AxisSource::WheelTilt,

                    _ => {
                        warn!("AxisSource: invalid axis source");
                        resource.post_error(
                            zwlr_virtual_pointer_v1::Error::InvalidAxisSource,
                            "invalid axis source",
                        );
                        return;
                    }
                };

                pointer
                    .data()
                    .axis_frame
                    .lock()
                    .unwrap()
                    .set_source(axis_source);
            }
            zwlr_virtual_pointer_v1::Request::AxisStop { time, axis } => {
                let axis = match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => Axis::Vertical,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => Axis::Horizontal,
                    _ => {
                        warn!("AxisStop: invalid axis");
                        resource.post_error(
                            zwlr_virtual_pointer_v1::Error::InvalidAxis,
                            "invalid axis",
                        );
                        return;
                    }
                };

                pointer.mutate_axis_frame(Some(time), |frame| frame.stop(axis));
            }
            zwlr_virtual_pointer_v1::Request::AxisDiscrete {
                time,
                axis,
                value,
                discrete,
            } => {
                let axis = match axis {
                    WEnum::Value(wl_pointer::Axis::VerticalScroll) => Axis::Vertical,
                    WEnum::Value(wl_pointer::Axis::HorizontalScroll) => Axis::Horizontal,
                    _ => {
                        warn!("AxisDiscrete: invalid axis");
                        resource.post_error(
                            zwlr_virtual_pointer_v1::Error::InvalidAxis,
                            "invalid axis",
                        );
                        return;
                    }
                };
                pointer.mutate_axis_frame(Some(time), |frame| {
                    frame
                        .value(axis, value)
                        .v120(axis, discrete_to_v120(discrete))
                });
            }
            zwlr_virtual_pointer_v1::Request::Destroy => {}
            _ => unreachable!(),
        }
    }

    fn destroyed(
        &self,
        handler: &mut D,
        _client: wayland_backend::server::ClientId,
        resource: &ZwlrVirtualPointerV1,
    ) {
        let pointer = VirtualPointer {
            pointer: resource.clone(),
        };

        handler.destroy_virtual_pointer(pointer);
        handler
            .virtual_pointer_manager_state()
            .virtual_pointers
            .remove(resource);
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::fmt::MakeWriter;

    use super::*;

    #[derive(Clone, Default)]
    struct Log(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Log {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Log {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn absolute_coordinates_are_normalized_without_output_scale() {
        assert_eq!(normalized_absolute_coordinate(1, 0), None);
        assert_eq!(normalized_absolute_coordinate(900, 1200), Some(0.75));
        assert_eq!(normalized_absolute_coordinate(701, 701), Some(1.));
    }

    #[test]
    fn axis_frames_are_isolated_and_unfinished_frames_are_dropped() {
        let mut first = PendingAxisFrame::default();
        let mut second = PendingAxisFrame::default();

        first.mutate(Some(1), |frame| {
            frame
                .value(Axis::Vertical, 2.)
                .source(AxisSource::Finger)
                .relative_direction(Axis::Vertical, AxisRelativeDirection::Inverted)
        });
        second.mutate(Some(2), |frame| frame.value(Axis::Horizontal, 3.));

        let first_frame = first.finish().unwrap();
        assert_eq!(first_frame.time, InputTime::from_millis(1));
        assert_eq!(first_frame.axis, (0., 2.));
        assert_eq!(first_frame.source, Some(AxisSource::Finger));
        assert_eq!(
            first_frame.relative_direction,
            (
                AxisRelativeDirection::Identical,
                AxisRelativeDirection::Inverted
            )
        );
        assert!(first.finish().is_none());

        let second_frame = second.finish().unwrap();
        assert_eq!(second_frame.time, InputTime::from_millis(2));
        assert_eq!(second_frame.axis, (3., 0.));

        {
            let mut unfinished = PendingAxisFrame::default();
            unfinished.mutate(Some(3), |frame| frame.value(Axis::Vertical, 4.));
        }
    }

    #[test]
    fn axis_source_before_axis_applies_to_the_frame() {
        let mut pending = PendingAxisFrame::default();
        pending.set_source(AxisSource::Continuous);
        pending.mutate(Some(1), |frame| frame.value(Axis::Vertical, 2.));

        assert_eq!(
            pending.finish().unwrap().source,
            Some(AxisSource::Continuous)
        );
    }

    #[test]
    fn axis_source_does_not_leak_into_the_next_frame() {
        let mut pending = PendingAxisFrame::default();
        pending.set_source(AxisSource::Continuous);
        pending.mutate(Some(1), |frame| frame.value(Axis::Vertical, 2.));
        let _ = pending.finish().unwrap();
        pending.mutate(Some(2), |frame| frame.value(Axis::Vertical, 3.));

        assert_eq!(pending.finish().unwrap().source, None);
    }

    #[test]
    fn discrete_axis_conversion_does_not_overflow() {
        assert_eq!(discrete_to_v120(1), 120);
        assert_eq!(discrete_to_v120(i32::MAX), i32::MAX);
        assert_eq!(discrete_to_v120(i32::MIN), i32::MIN);
    }

    #[test]
    fn omitted_axis_source_defaults_to_wheel_without_a_warning() {
        let log = Log::default();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(log.clone())
            .finish();

        let source = tracing::subscriber::with_default(subscriber, || axis_source_or_default(None));

        assert_eq!(source, AxisSource::Wheel);
        assert_eq!(&*log.0.lock().unwrap(), b"");
    }
}

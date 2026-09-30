use std::any::Any;
use std::collections::hash_map::Entry;
use std::collections::HashSet;
use std::time::Duration;

use calloop::timer::{TimeoutAction, Timer};
use input::event::gesture::GestureEventCoordinates as _;
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Device, DeviceCapability, Event,
    GestureBeginEvent, GestureEndEvent, GesturePinchUpdateEvent as _, GestureSwipeUpdateEvent as _,
    InputEvent, KeyState, KeyboardKeyEvent, Keycode, MouseButton, PointerAxisEvent,
    PointerButtonEvent, PointerMotionEvent, ProximityState, Switch, SwitchState, SwitchToggleEvent,
    TabletToolButtonEvent, TabletToolEvent, TabletToolProximityEvent, TabletToolTipEvent,
    TabletToolTipState, TouchEvent,
};
use smithay::backend::libinput::LibinputInputBackend;
use smithay::desktop::Window;
use smithay::input::dnd::DnDGrab;
use smithay::input::keyboard::xkb::keysym_get_name;
use smithay::input::keyboard::{keysyms, FilterResult, Keysym, Layout, ModifiersState};
use smithay::input::pointer::{
    AxisFrame, ButtonEvent, CursorIcon, CursorImageStatus, Focus, GestureHoldBeginEvent,
    GestureHoldEndEvent, GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
    GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent,
    GrabStartData as PointerGrabStartData, MotionEvent, PointerGrab, PointerHandle,
    RelativeMotionEvent,
};
use smithay::input::tablet::tool::GrabStartData as TabletToolGrabStartData;
use smithay::input::tablet::{TabletDescriptor, TabletSeatHandler, TabletSeatTrait};
use smithay::input::touch::{
    DownEvent, GrabStartData as TouchGrabStartData, MotionEvent as TouchMotionEvent, UpEvent,
};
use smithay::input::{tablet as smithay_tablet, SeatHandler};
use smithay::output::Output;
use smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle, Serial, Transform, SERIAL_COUNTER};
use smithay::wayland::keyboard_shortcuts_inhibit::KeyboardShortcutsInhibitor;
use smithay::wayland::pointer_constraints::{with_pointer_constraint, PointerConstraint};
use swayward_config::{
    Action, Bind, Binds, Config, Key, ModKey, Modifiers, MouseRegions, MruDirection, SwitchBinds,
    Trigger,
};
use swayward_ipc::LayoutSwitchTarget;
use touch_overview_grab::TouchOverviewGrab;

use self::move_grab::MoveGrab;
use self::pick_color_grab::PickColorGrab;
use self::pick_window_grab::PickWindowGrab;
use self::resize_grab::ResizeGrab;
use self::spatial_movement_grab::SpatialMovementGrab;
#[cfg(feature = "dbus")]
use crate::dbus::freedesktop_a11y::KbMonBlock;
use crate::layout::{ActivateWindow, HitType, LayoutElement as _};
use crate::swayward::{CastTarget, PointerVisibility, State};
use crate::ui::mru::{WindowMru, WindowMruUi};
use crate::ui::screenshot_ui::ScreenshotUi;
use crate::utils::spawning::{spawn, spawn_sh};
use crate::utils::{center, CastSessionId, ResizeEdge};

pub mod backend_ext;
pub mod click_grab;
pub mod move_grab;
pub mod pick_color_grab;
pub mod pick_window_grab;
pub mod resize_grab;
pub mod scroll_swipe_gesture;
pub mod scroll_tracker;
pub mod spatial_movement_grab;
pub mod swipe_tracker;
pub mod touch_overview_grab;

use backend_ext::{NiriInputBackend as InputBackend, NiriInputDevice as _};

pub const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(400);

mod actions;
mod keyboard_bindings;
mod libinput;
mod pointer;
mod tablet;
mod touch;

#[cfg(test)]
pub(crate) use keyboard_bindings::hardcoded_overview_bind;
use keyboard_bindings::*;
pub use libinput::{
    apply_libinput_settings, mods_with_binds, mods_with_finger_scroll_binds, mods_with_mouse_binds,
    mods_with_tablet_stylus_binds, mods_with_wheel_binds,
};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct IpcInputDevice {
    pub identifier: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vendor: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<u32>,
    #[serde(rename = "type")]
    pub device_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub libinput: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabletData {
    pub aspect_ratio: f64,
}

pub enum AnyStartData<D: SeatHandler + TabletSeatHandler> {
    Pointer(PointerGrabStartData<D>),
    Touch(TouchGrabStartData<D>),
    TabletTool(TabletToolGrabStartData<D>),
}

impl<D: SeatHandler + TabletSeatHandler> AnyStartData<D> {
    pub fn location(&self) -> Point<f64, Logical> {
        match self {
            AnyStartData::Pointer(x) => x.location,
            AnyStartData::Touch(x) => x.location,
            AnyStartData::TabletTool(x) => x.location,
        }
    }

    pub fn unwrap_pointer(&self) -> &PointerGrabStartData<D> {
        match self {
            AnyStartData::Pointer(x) => x,
            AnyStartData::Touch(_) | AnyStartData::TabletTool(_) => {
                panic!("start_data is not Pointer")
            }
        }
    }

    pub fn unwrap_touch(&self) -> &TouchGrabStartData<D> {
        match self {
            AnyStartData::Pointer(_) | AnyStartData::TabletTool(_) => {
                panic!("start_data is not Touch")
            }
            AnyStartData::Touch(x) => x,
        }
    }

    pub fn unwrap_tablet_tool(&self) -> &TabletToolGrabStartData<D> {
        match self {
            AnyStartData::Pointer(_) | AnyStartData::Touch(_) => {
                panic!("start_data is not TabletTool")
            }
            AnyStartData::TabletTool(x) => x,
        }
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self, Self::Pointer(_))
    }

    pub fn is_touch(&self) -> bool {
        matches!(self, Self::Touch(_))
    }

    pub fn is_tablet_tool(&self) -> bool {
        matches!(self, Self::TabletTool(_))
    }
}

impl State {
    pub fn process_input_event<I: InputBackend + 'static>(&mut self, event: InputEvent<I>)
    where
        I::Device: 'static, // Needed for downcasting.
    {
        let _span = tracy_client::span!("process_input_event");

        // Make sure some logic like workspace clean-up has a chance to run before doing actions.
        self.swayward.advance_animations();

        if self.swayward.monitors_active {
            // Notify the idle-notifier of activity.
            if should_notify_activity(&event) {
                self.swayward.notify_activity();
            }
        } else {
            // Power on monitors if they were off.
            if should_activate_monitors(&event) {
                self.swayward.activate_monitors(&mut self.backend);

                // Notify the idle-notifier of activity only if we're also powering on the
                // monitors.
                self.swayward.notify_activity();
            }
        }

        if should_reset_pointer_inactivity_timer(&event) {
            self.swayward.reset_pointer_inactivity_timer();
        }

        let hide_hotkey_overlay =
            self.swayward.hotkey_overlay.is_open() && should_hide_hotkey_overlay(&event);

        let hide_exit_confirm_dialog =
            self.swayward.exit_confirm_dialog.is_open() && should_hide_exit_confirm_dialog(&event);

        let mut consumed_by_a11y = false;
        use InputEvent::*;
        match event {
            DeviceAdded { device } => self.on_device_added(device),
            DeviceRemoved { device } => self.on_device_removed(device),
            Keyboard { event } => self.on_keyboard::<I>(event, &mut consumed_by_a11y),
            PointerMotion { event } => self.on_pointer_motion::<I>(event),
            PointerMotionAbsolute { event } => self.on_pointer_motion_absolute::<I>(event),
            PointerButton { event } => self.on_pointer_button::<I>(event),
            PointerAxis { event } => self.on_pointer_axis::<I>(event),
            TabletToolAxis { event } => self.on_tablet_tool_axis::<I>(event),
            TabletToolTip { event } => self.on_tablet_tool_tip::<I>(event),
            TabletToolProximity { event } => self.on_tablet_tool_proximity::<I>(event),
            TabletToolButton { event } => self.on_tablet_tool_button::<I>(event),
            GestureSwipeBegin { event } => self.on_gesture_swipe_begin::<I>(event),
            GestureSwipeUpdate { event } => self.on_gesture_swipe_update::<I>(event),
            GestureSwipeEnd { event } => self.on_gesture_swipe_end::<I>(event),
            GesturePinchBegin { event } => self.on_gesture_pinch_begin::<I>(event),
            GesturePinchUpdate { event } => self.on_gesture_pinch_update::<I>(event),
            GesturePinchEnd { event } => self.on_gesture_pinch_end::<I>(event),
            GestureHoldBegin { event } => self.on_gesture_hold_begin::<I>(event),
            GestureHoldEnd { event } => self.on_gesture_hold_end::<I>(event),
            TouchDown { event } => self.on_touch_down::<I>(event),
            TouchMotion { event } => self.on_touch_motion::<I>(event),
            TouchUp { event } => self.on_touch_up::<I>(event),
            TouchCancel { event } => self.on_touch_cancel::<I>(event),
            TouchFrame { event } => self.on_touch_frame::<I>(event),
            SwitchToggle { event } => self.on_switch_toggle::<I>(event),
            Special(_) => (),
        }

        // Don't hide overlays if consumed by a11y, so that you can use the screen reader
        // navigation keys.
        if consumed_by_a11y {
            return;
        }

        // Do this last so that screenshot still gets it.
        if hide_hotkey_overlay && self.swayward.hotkey_overlay.hide() {
            self.swayward.queue_redraw_all();
        }

        if hide_exit_confirm_dialog && self.swayward.exit_confirm_dialog.hide() {
            self.swayward.queue_redraw_all();
        }
    }

    pub fn process_libinput_event(&mut self, event: &mut InputEvent<LibinputInputBackend>) {
        let _span = tracy_client::span!("process_libinput_event");

        match event {
            InputEvent::DeviceAdded { device } => {
                self.swayward.devices.insert(device.clone());

                if device.has_capability(input::DeviceCapability::TabletTool) {
                    match device.size() {
                        Some((w, h)) => {
                            let aspect_ratio = w / h;
                            let data = TabletData { aspect_ratio };
                            self.swayward.tablets.insert(device.clone(), data);
                        }
                        None => {
                            warn!("tablet tool device has no size");
                        }
                    }
                }

                if device.has_capability(input::DeviceCapability::Keyboard) {
                    if let Some(led_state) = self
                        .swayward
                        .seat
                        .get_keyboard()
                        .map(|keyboard| keyboard.led_state())
                    {
                        device.led_update(led_state.into());
                    }
                }

                if device.has_capability(input::DeviceCapability::Touch) {
                    self.swayward.touch.insert(device.clone());
                }

                apply_libinput_settings(&self.swayward.config.borrow().input, device);
            }
            InputEvent::DeviceRemoved { device } => {
                self.swayward.touch.remove(device);
                self.swayward.tablets.remove(device);
                self.swayward.devices.remove(device);
            }
            _ => (),
        }
    }

    fn on_device_added(&mut self, device: impl backend_ext::NiriInputDevice) {
        let device_type = if device.has_capability(DeviceCapability::Keyboard) {
            "keyboard"
        } else if device.has_capability(DeviceCapability::Pointer) {
            "pointer"
        } else if device.has_capability(DeviceCapability::Touch) {
            "touch"
        } else if device.has_capability(DeviceCapability::TabletTool) {
            "tablet_tool"
        } else if device.has_capability(DeviceCapability::TabletPad) {
            "tablet_pad"
        } else if device.has_capability(DeviceCapability::Switch) {
            "switch"
        } else {
            "unknown"
        };
        let libinput = device.sway_libinput();
        let (vendor, product) = libinput
            .is_some()
            .then(|| device.usb_id().unwrap_or((0, 0)))
            .map_or((None, None), |(product, vendor)| {
                (Some(vendor), Some(product))
            });
        let input = IpcInputDevice {
            identifier: device.sway_identifier(),
            name: device.name(),
            vendor,
            product,
            device_type,
            scroll_factor: (device_type == "pointer").then_some(1.),
            libinput,
        };
        self.swayward
            .ipc_input_devices
            .insert(device.id(), input.clone());
        self.ipc_input_changed("added", input);

        if device.has_capability(DeviceCapability::TabletTool) {
            let tablet_seat = self.swayward.seat.tablet_seat();

            let desc = TabletDescriptor::from(&device);
            tablet_seat.add_wp_tablet(&self.swayward.display_handle, &desc);
        }
        if device.has_capability(DeviceCapability::Touch)
            && self.swayward.seat.get_touch().is_none()
        {
            self.swayward.seat.add_touch();
        }
    }

    fn on_device_removed(&mut self, device: impl Device) {
        if let Some(input) = self.swayward.ipc_input_devices.remove(&device.id()) {
            self.ipc_input_changed("removed", input);
        }

        if device.has_capability(DeviceCapability::TabletTool) {
            let tablet_seat = self.swayward.seat.tablet_seat();

            let desc = TabletDescriptor::from(&device);
            tablet_seat.remove_tablet(&desc);

            // If there are no tablets in seat we can remove all tools
            if tablet_seat.count_tablets() == 0 {
                tablet_seat.clear_tools();
            }
        }
        if device.has_capability(DeviceCapability::Touch) && self.swayward.touch.is_empty() {
            self.swayward.seat.remove_touch();
        }
    }

    /// Computes the rectangle that covers all outputs in global space.
    fn global_bounding_rectangle(&self) -> Option<Rectangle<i32, Logical>> {
        self.swayward.global_space.outputs().fold(
            None,
            |acc: Option<Rectangle<i32, Logical>>, output| {
                self.swayward
                    .global_space
                    .output_geometry(output)
                    .map(|geo| acc.map(|acc| acc.merge(geo)).unwrap_or(geo))
            },
        )
    }

    /// Computes the cursor position for the tablet event.
    ///
    /// This function handles the tablet output mapping, as well as coordinate clamping and aspect
    /// ratio correction.
    fn compute_tablet_position<I: InputBackend>(
        &self,
        event: &(impl Event<I> + TabletToolEvent<I>),
    ) -> Option<Point<f64, Logical>>
    where
        I::Device: 'static,
    {
        let device_output = event.device().output(self);
        let device_output = device_output.filter(|output| self.swayward.output_exists(output));
        let device_output = device_output.as_ref();
        let mapped_output = device_output.or_else(|| self.swayward.output_for_tablet());

        // If the tablet is configured to map to the focused window, use that window's geometry on
        // the mapped output (or on the focused output if no specific output is mapped).
        let map_to_focused_window = self
            .swayward
            .config
            .borrow()
            .input
            .tablet
            .map_to_focused_window;
        // But only if the keyboard focus is on the layout, so that it doesn't trigger on the lock
        // screen and such.
        let window_target = if map_to_focused_window && self.swayward.keyboard_focus.is_layout() {
            let output = mapped_output.or_else(|| self.swayward.layout.active_output());
            output.and_then(|output| {
                let monitor = self.swayward.layout.monitor_for_output(output)?;
                let mut rect = monitor.active_window_visual_rectangle()?;
                let output_geo = self.swayward.global_space.output_geometry(output)?;
                rect.loc += output_geo.loc.to_f64();
                Some((rect, output))
            })
        } else {
            None
        };

        let (target_geo, keep_ratio, px, transform) = if let Some((rect, output)) = window_target {
            (
                rect,
                true,
                1. / output.current_scale().fractional_scale(),
                output.current_transform(),
            )
        } else if let Some(output) = mapped_output {
            let geo = self.swayward.global_space.output_geometry(output).unwrap();
            (
                geo.to_f64(),
                true,
                1. / output.current_scale().fractional_scale(),
                output.current_transform(),
            )
        } else {
            let geo = self.global_bounding_rectangle()?.to_f64();

            // FIXME: this 1 px size should ideally somehow be computed for the rightmost output
            // corresponding to the position on the right when clamping.
            let output = self.swayward.global_space.outputs().next().unwrap();
            let scale = output.current_scale().fractional_scale();

            // Do not keep ratio for the unified mode as this is what OpenTabletDriver expects.
            (geo, false, 1. / scale, Transform::Normal)
        };

        let mut pos = {
            let size = transform.invert().transform_size(target_geo.size);
            transform.transform_point_in(event.position_transformed(size.to_i32_round()), &size)
        };

        if keep_ratio {
            pos.x /= target_geo.size.w;
            pos.y /= target_geo.size.h;

            let device = event.device();
            if let Some(device) = (&device as &dyn Any).downcast_ref::<input::Device>() {
                if let Some(data) = self.swayward.tablets.get(device) {
                    // This code does the same thing as mutter with "keep aspect ratio" enabled.
                    let size = transform.invert().transform_size(target_geo.size);
                    let output_aspect_ratio = size.w / size.h;
                    let ratio = data.aspect_ratio / output_aspect_ratio;

                    if ratio > 1. {
                        pos.x *= ratio;
                    } else {
                        pos.y /= ratio;
                    }
                }
            };

            pos.x *= target_geo.size.w;
            pos.y *= target_geo.size.h;
        }

        pos.x = pos.x.clamp(0.0, target_geo.size.w - px);
        pos.y = pos.y.clamp(0.0, target_geo.size.h - px);
        Some(pos + target_geo.loc)
    }

    fn is_inhibiting_shortcuts(&self) -> bool {
        self.swayward
            .keyboard_focus
            .surface()
            .and_then(|surface| {
                self.swayward
                    .keyboard_shortcuts_inhibiting_surfaces
                    .get(surface)
            })
            .is_some_and(KeyboardShortcutsInhibitor::is_active)
    }

    fn on_keyboard<I: InputBackend>(
        &mut self,
        event: I::KeyboardKeyEvent,
        consumed_by_a11y: &mut bool,
    ) {
        let mod_key = self.backend.mod_key(&self.swayward.config.borrow());
        let input_device = event.device().sway_identifier();

        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time(&event);
        let pressed = event.state() == KeyState::Pressed;
        let code_modifiers = self.modifier_state();

        // Stop bind key repeat on any release. This won't work 100% correctly in cases like:
        // 1. Press Mod
        // 2. Press Left (repeat starts)
        // 3. Press PgDown (new repeat starts)
        // 4. Release Left (PgDown repeat stops)
        // But it's good enough for now.
        // FIXME: handle this properly.
        if !pressed {
            if let Some(token) = self.swayward.bind_repeat_timer.take() {
                self.swayward.event_loop.remove(token);
            }
        }

        if pressed {
            self.hide_cursor_if_needed();
        }

        let is_inhibiting_shortcuts = self.is_inhibiting_shortcuts();

        // Accessibility modifier grabs should override XKB state changes (e.g. Caps Lock), so we
        // need to process them before keyboard.input() below.
        //
        // Other accessibility-grabbed keys should still update our XKB state, but not cause any
        // other changes.
        #[cfg(feature = "dbus")]
        let block = {
            let block = self.a11y_process_key(
                Duration::from_micros(time.micros()),
                event.key_code(),
                event.state(),
            );
            if block != KbMonBlock::Pass {
                *consumed_by_a11y = true;
            }
            // The accessibility modifier first press must not change XKB state, so we return
            // early here.
            if block == KbMonBlock::ModifierFirstPress {
                return;
            }
            block
        };
        #[cfg(not(feature = "dbus"))]
        let _ = consumed_by_a11y;

        let Some(keyboard) = self.swayward.seat.get_keyboard() else {
            return;
        };
        let Some(Some(bind)) = keyboard.input(
            self,
            event.key_code(),
            event.state(),
            serial,
            time,
            |this, mods, keysym| {
                let key_code = event.key_code();
                let modified = keysym.modified_sym();
                let raw = keysym.raw_latin_sym_or_raw_current_sym();
                let group = keysym.xkb().lock().unwrap().active_layout().0;
                let raw_modifiers = code_modifiers;
                let translated_modifiers = translated_modifiers(&keysym, key_code, code_modifiers);
                let modifiers = modifiers_from_state(raw_modifiers);

                // After updating XKB state from accessibility-grabbed keys, return right away and
                // don't handle them.
                #[cfg(feature = "dbus")]
                if block != KbMonBlock::Pass {
                    // HACK: there's a slight problem with this code. Here we filter out keys
                    // consumed by accessibility from getting sent to the Wayland client. However,
                    // the Wayland client can still receive these keys from the wl_keyboard
                    // enter/modifiers events. In particular, this can easily happen when opening
                    // the Orca actions menu with Orca + Shift + A: in most cases, when this menu
                    // opens, Shift is still held down, so the menu receives it in
                    // wl_keyboard.enter/modifiers. Then the menu won't react to Enter presses
                    // until the user taps Shift again to "release" it (since the initial Shift
                    // release will be intercepted here).
                    //
                    // I don't think there's any good way of dealing with this apart from keeping a
                    // separate xkb state for accessibility, so that we can track the pressed
                    // modifiers without accidentally leaking them to wl_keyboard.enter. So for now
                    // let's forward modifier releases to the clients here to deal with the most
                    // common case.
                    if !pressed
                        && matches!(
                            modified,
                            Keysym::Shift_L
                                | Keysym::Shift_R
                                | Keysym::Control_L
                                | Keysym::Control_R
                                | Keysym::Super_L
                                | Keysym::Super_R
                                | Keysym::Alt_L
                                | Keysym::Alt_R
                        )
                    {
                        return FilterResult::Forward;
                    } else {
                        return FilterResult::Intercept(None);
                    }
                }

                if this.swayward.exit_confirm_dialog.is_open() && pressed {
                    if raw == Some(Keysym::Return) {
                        info!("quitting after confirming exit dialog");
                        this.request_stop("exit");
                    }

                    // Don't send this press to any clients.
                    this.swayward.suppressed_keys.insert(key_code);
                    return FilterResult::Intercept(None);
                }

                // Check if all modifiers were released while the MRU UI was open. If so, close the
                // UI (which will also transfer the focus to the current MRU UI selection).
                if this.swayward.window_mru_ui.is_open() && !pressed && modifiers.is_empty() {
                    this.do_action(Action::MruConfirm, false);

                    if this.swayward.suppressed_keys.remove(&key_code) {
                        return FilterResult::Intercept(None);
                    } else {
                        return FilterResult::Forward;
                    }
                }

                if pressed && raw == Some(Keysym::Escape) {
                    // Cancel certain grabs on Escape.
                    let pointer = this.swayward.seat.get_pointer().unwrap();
                    if pointer
                        .with_grab(|_, grab| Self::grab_can_be_cancelled_with_esc(grab))
                        .unwrap_or(false)
                    {
                        pointer.unset_grab(this, serial, time);
                        this.swayward.suppressed_keys.insert(key_code);
                        return FilterResult::Intercept(None);
                    }
                }

                if let Some(Keysym::space) = raw {
                    this.swayward.screenshot_ui.set_space_down(pressed);
                }

                let locked = this.swayward.is_locked();
                let res = {
                    let config = this.swayward.config.borrow();
                    let bindings = make_binds_iter(
                        &config,
                        &this.swayward.binding_mode,
                        &mut this.swayward.window_mru_ui,
                        modifiers,
                    );

                    should_intercept_key(
                        &mut this.swayward.suppressed_keys,
                        &mut this.swayward.held_release_bind,
                        bindings,
                        KeyEventContext {
                            input_device: &input_device,
                            key_code,
                            modified,
                            raw,
                            group,
                            code_modifiers,
                            raw_modifiers,
                            translated_modifiers,
                        },
                        pressed,
                        &this.swayward.screenshot_ui,
                        BindingPolicy {
                            mod_key,
                            locked,
                            inhibited: is_inhibiting_shortcuts,
                            disable_power_key_handling: config.input.disable_power_key_handling,
                        },
                    )
                };

                if matches!(res, FilterResult::Forward) {
                    // If we didn't find any bind, try other hardcoded keys.
                    if this.swayward.keyboard_focus.is_overview() && pressed {
                        if let Some(bind) = raw.and_then(|raw| hardcoded_overview_bind(raw, *mods))
                        {
                            this.swayward.suppressed_keys.insert(key_code);
                            return FilterResult::Intercept(Some(bind));
                        }
                    }

                    // Interaction with the active window, immediately update the active window's
                    // focus timestamp without waiting for a possible pending MRU lock-in delay.
                    this.swayward.mru_apply_keyboard_commit();
                }

                res
            },
        ) else {
            return;
        };

        self.handle_bind(bind.clone());

        if pressed {
            self.start_key_repeat(bind);
        }
    }

    fn start_key_repeat(&mut self, bind: Bind) {
        if !bind.repeat {
            return;
        }

        // Stop the previous key repeat if any.
        if let Some(token) = self.swayward.bind_repeat_timer.take() {
            self.swayward.event_loop.remove(token);
        }

        let config = self.swayward.config.borrow();
        let config = &config.input.keyboard;

        let repeat_rate = config.repeat_rate;
        if repeat_rate == 0 {
            return;
        }
        let repeat_duration = Duration::from_secs_f64(1. / f64::from(repeat_rate));

        let repeat_timer =
            Timer::from_duration(Duration::from_millis(u64::from(config.repeat_delay)));

        let token = self
            .swayward
            .event_loop
            .insert_source(repeat_timer, move |_, _, state| {
                state.handle_bind(bind.clone());
                TimeoutAction::ToDuration(repeat_duration)
            })
            .unwrap();

        self.swayward.bind_repeat_timer = Some(token);
    }

    fn hide_cursor_if_needed(&mut self) {
        // If the pointer is already invisible, don't reset it back to Hidden causing one frame
        // of hover.
        if !self.swayward.pointer_visibility.is_visible() {
            return;
        }

        if !self.swayward.config.borrow().cursor.hide_when_typing {
            return;
        }

        // niri keeps this set only while actively using a tablet, which means the cursor position
        // is likely to change almost immediately, causing pointer_visibility to just flicker back
        // and forth.
        if self.swayward.tablet_cursor_location.is_some() {
            return;
        }

        self.swayward.pointer_visibility = PointerVisibility::Hidden;
        self.swayward.queue_redraw_all();
    }

    fn on_switch_toggle<I: InputBackend>(&mut self, evt: I::SwitchToggleEvent) {
        let Some(switch) = evt.switch() else {
            return;
        };

        if switch == Switch::Lid {
            let is_closed = evt.state() == SwitchState::On;
            trace!("lid switch {}", if is_closed { "closed" } else { "opened" });
            self.set_lid_closed(is_closed);
        }

        let locked = self.swayward.is_locked();
        let command = self
            .swayward
            .runtime_switch_bindings
            .iter()
            .filter(|binding| {
                binding.mode == self.swayward.binding_mode
                    && binding.switch == switch
                    && binding.state.is_none_or(|state| state == evt.state())
                    && (!locked || binding.locked)
            })
            .max_by_key(|binding| binding.locked == locked)
            .map(|binding| binding.command.clone());
        if let Some(command) = command {
            let _ = crate::command::execute(self, &command);
            return;
        }

        let action = {
            let bindings = &self.swayward.config.borrow().switch_events;
            find_configured_switch_action(bindings, switch, evt.state())
        };
        if let Some(action) = action {
            self.do_action(action, true);
        }
    }

    pub fn is_dnd_grab(grab: &dyn Any) -> bool {
        // Normal DnD
        grab.is::<DnDGrab<Self, WlDataSource, WlSurface>>()
            // Null-source DnD: weston-dnd --self-only
            || grab.is::<DnDGrab<Self, WlSurface, WlSurface>>()
    }

    fn grab_can_be_cancelled_with_esc(grab: &(dyn PointerGrab<State> + 'static)) -> bool {
        let grab = grab.as_any();

        grab.is::<PickWindowGrab>() || grab.is::<PickColorGrab>() || Self::is_dnd_grab(grab)
    }
}

#[cfg(test)]
mod tests;

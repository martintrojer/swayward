use std::cmp::min;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;
use std::os::fd::AsFd as _;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;
use single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;
use smithay::reexports::rustix::fs::{ftruncate, memfd_create, MemfdFlags};
use smithay::reexports::wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::{
        self, ZwpKeyboardShortcutsInhibitManagerV1,
    },
    zwp_keyboard_shortcuts_inhibitor_v1::{self, ZwpKeyboardShortcutsInhibitorV1},
};
use smithay::reexports::wayland_protocols::wp::single_pixel_buffer;
use smithay::reexports::wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay::reexports::wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use smithay::reexports::wayland_protocols::xdg::activation::v1::client::xdg_activation_token_v1::{
    self, XdgActivationTokenV1,
};
use smithay::reexports::wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::{
    self, XdgActivationV1,
};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_popup::{self, XdgPopup};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_positioner::{
    ConstraintAdjustment, XdgPositioner,
};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};
use smithay::reexports::wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
    ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
    ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
};
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{
    self, ZwlrLayerShellV1,
};
use smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};
use smithay::reexports::wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    self, ZwlrLayerSurfaceV1,
};
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_frame_v1::{
    self, ZwlrScreencopyFrameV1,
};
use smithay::reexports::wayland_protocols_wlr::screencopy::v1::client::zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1;
use smithay::reexports::wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::{self, ZwlrVirtualPointerV1},
};
use smithay::reexports::wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};
use wayland_backend::client::Backend;
use wayland_client::globals::Global;
use wayland_client::protocol::wl_buffer::{self, WlBuffer};
use wayland_client::protocol::wl_callback::{self, WlCallback};
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_display::WlDisplay;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_shm::{self, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::{self, WlSurface};
use wayland_client::{Connection, Dispatch, Proxy as _, QueueHandle};

use crate::protocols::raw::mutter_x11_interop::v1::client::mutter_x11_interop::MutterX11Interop;
use crate::utils::id::IdCounter;

pub struct Client {
    pub id: ClientId,
    pub event_loop: EventLoop<'static, State>,
    pub connection: Connection,
    pub qh: QueueHandle<State>,
    pub display: WlDisplay,
    pub state: State,
}

pub struct State {
    pub qh: QueueHandle<State>,

    pub globals: Vec<Global>,
    pub outputs: HashMap<WlOutput, String>,

    pub compositor: Option<WlCompositor>,
    pub seat: Option<WlSeat>,
    pub xdg_wm_base: Option<XdgWmBase>,
    pub xdg_wm_base_version: Option<u32>,
    pub xdg_activation: Option<XdgActivationV1>,
    pub keyboard_shortcuts_inhibit_manager: Option<ZwpKeyboardShortcutsInhibitManagerV1>,
    pub shortcut_inhibitor_events: Vec<bool>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub foreign_toplevel_manager: Option<ZwlrForeignToplevelManagerV1>,
    pub foreign_toplevels: Vec<ForeignToplevel>,
    pub ext_workspace_manager: Option<ExtWorkspaceManagerV1>,
    pub workspace_groups: Vec<WorkspaceGroup>,
    pub ext_workspaces: Vec<ExtWorkspace>,
    pub workspace_membership_events: Vec<(ExtWorkspaceHandleV1, ExtWorkspaceGroupHandleV1, bool)>,
    pub output_manager: Option<ZwlrOutputManagerV1>,
    pub output_heads: Vec<OutputHead>,
    pub output_manager_serials: Vec<u32>,
    pub output_configuration_results: Vec<OutputConfigurationResult>,
    pub spbm: Option<WpSinglePixelBufferManagerV1>,
    pub viewporter: Option<WpViewporter>,
    pub shm: Option<WlShm>,
    pub screencopy: Option<ZwlrScreencopyManagerV1>,
    pub gamma_control_manager: Option<ZwlrGammaControlManagerV1>,
    pub gamma_controls: Vec<GammaControl>,
    pub virtual_pointer_manager: Option<ZwlrVirtualPointerManagerV1>,
    pub mutter_x11_interop: Option<MutterX11Interop>,

    pub windows: Vec<Window>,
    pub popups: Vec<Popup>,
    pub layers: Vec<LayerSurface>,
}

pub struct Window {
    pub qh: QueueHandle<State>,
    pub spbm: WpSinglePixelBufferManagerV1,

    pub surface: WlSurface,
    pub xdg_surface: XdgSurface,
    pub xdg_toplevel: XdgToplevel,
    pub viewport: WpViewport,
    pub pending_configure: Configure,
    pub configures_received: Vec<(u32, Configure)>,
    pub close_requested: bool,

    pub configures_looked_at: usize,
}

pub struct Popup {
    pub surface: WlSurface,
    pub xdg_surface: XdgSurface,
    pub xdg_popup: XdgPopup,
    pub configures_received: Vec<(i32, i32, i32, i32)>,
    pub repositioned: Vec<u32>,
}

pub struct ForeignToplevel {
    pub handle: ZwlrForeignToplevelHandleV1,
    pub title: Option<String>,
}

pub struct WorkspaceGroup {
    pub handle: ExtWorkspaceGroupHandleV1,
    pub outputs: Vec<WlOutput>,
    pub workspaces: Vec<ExtWorkspaceHandleV1>,
    pub removed: bool,
}

pub struct ExtWorkspace {
    pub handle: ExtWorkspaceHandleV1,
    pub id: Option<String>,
    pub name: Option<String>,
    pub removed: bool,
}

pub struct OutputHead {
    pub proxy: ZwlrOutputHeadV1,
    pub name: Option<String>,
    pub modes: Vec<ZwlrOutputModeV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputConfigurationResult {
    Succeeded,
    Failed,
    Cancelled,
}

pub struct LayerSurface {
    pub qh: QueueHandle<State>,
    pub spbm: WpSinglePixelBufferManagerV1,

    pub surface: WlSurface,
    pub layer_surface: ZwlrLayerSurfaceV1,
    pub viewport: WpViewport,
    pub configures_received: Vec<(u32, LayerConfigure)>,
    pub close_requested: bool,

    pub configures_looked_at: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Configure {
    pub size: (i32, i32),
    pub bounds: Option<(i32, i32)>,
    pub states: Vec<xdg_toplevel::State>,
}

#[derive(Debug, Clone, Copy)]
pub struct LayerConfigure {
    pub size: (u32, u32),
}

#[derive(Clone, Copy, Default)]
pub struct LayerMargin {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

#[derive(Clone, Copy, Default)]
pub struct LayerConfigureProps {
    pub size: Option<(u32, u32)>,
    pub anchor: Option<zwlr_layer_surface_v1::Anchor>,
    pub exclusive_zone: Option<i32>,
    pub margin: Option<LayerMargin>,
    pub kb_interactivity: Option<zwlr_layer_surface_v1::KeyboardInteractivity>,
    pub layer: Option<zwlr_layer_shell_v1::Layer>,
    pub exclusive_edge: Option<zwlr_layer_surface_v1::Anchor>,
}

#[derive(Default)]
pub struct SyncData {
    pub done: AtomicBool,
}

#[derive(Debug, Default)]
pub struct ScreencopyFrameEvents {
    pub buffer: Option<(wl_shm::Format, u32, u32, u32)>,
    pub linux_dmabuf: Option<(u32, u32, u32)>,
    pub buffer_done: bool,
    pub failed: bool,
    pub ready: bool,
    pub damage: Vec<(u32, u32, u32, u32)>,
}

pub struct ScreencopyFrame {
    pub proxy: ZwlrScreencopyFrameV1,
    pub events: Arc<std::sync::Mutex<ScreencopyFrameEvents>>,
}

pub struct GammaControl {
    pub proxy: ZwlrGammaControlV1,
    pub gamma_size: Option<u32>,
    pub failed: bool,
}

static CLIENT_ID_COUNTER: IdCounter = IdCounter::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(u64);

impl ClientId {
    fn next() -> ClientId {
        ClientId(CLIENT_ID_COUNTER.next())
    }
}

impl fmt::Display for Configure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "size: {} × {}, ", self.size.0, self.size.1)?;
        if let Some(bounds) = self.bounds {
            write!(f, "bounds: {} × {}, ", bounds.0, bounds.1)?;
        } else {
            write!(f, "bounds: none, ")?;
        }
        write!(f, "states: {:?}", self.states)?;
        Ok(())
    }
}

impl fmt::Display for LayerConfigure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "size: {} × {}", self.size.0, self.size.1)?;
        Ok(())
    }
}

impl Client {
    pub fn new(stream: UnixStream) -> Self {
        let id = ClientId::next();

        let event_loop = EventLoop::try_new().unwrap();
        let backend = Backend::connect(stream).unwrap();
        let connection = Connection::from_backend(backend);
        let queue = connection.new_event_queue();
        let qh = queue.handle();
        WaylandSource::new(connection.clone(), queue)
            .insert(event_loop.handle())
            .unwrap();

        let display = connection.display();
        let _registry = display.get_registry(&qh, ());
        connection.flush().unwrap();

        let state = State {
            qh: qh.clone(),
            globals: Vec::new(),
            outputs: HashMap::new(),
            compositor: None,
            seat: None,
            xdg_wm_base: None,
            xdg_wm_base_version: None,
            xdg_activation: None,
            keyboard_shortcuts_inhibit_manager: None,
            shortcut_inhibitor_events: Vec::new(),
            layer_shell: None,
            foreign_toplevel_manager: None,
            foreign_toplevels: Vec::new(),
            ext_workspace_manager: None,
            workspace_groups: Vec::new(),
            ext_workspaces: Vec::new(),
            workspace_membership_events: Vec::new(),
            output_manager: None,
            output_heads: Vec::new(),
            output_manager_serials: Vec::new(),
            output_configuration_results: Vec::new(),
            spbm: None,
            viewporter: None,
            shm: None,
            screencopy: None,
            gamma_control_manager: None,
            gamma_controls: Vec::new(),
            virtual_pointer_manager: None,
            mutter_x11_interop: None,
            windows: Vec::new(),
            popups: Vec::new(),
            layers: Vec::new(),
        };

        Self {
            id,
            event_loop,
            connection,
            qh,
            display,
            state,
        }
    }

    pub fn dispatch(&mut self) {
        self.dispatch_unchecked();

        if let Some(error) = self.connection.protocol_error() {
            panic!("{error}");
        }
    }

    pub fn dispatch_unchecked(&mut self) {
        self.event_loop
            .dispatch(Duration::ZERO, &mut self.state)
            .unwrap();
    }

    pub fn send_sync(&self) -> Arc<SyncData> {
        let data = Arc::new(SyncData::default());
        self.display.sync(&self.qh, data.clone());
        self.connection.flush().unwrap();
        data
    }

    pub fn create_window(&mut self) -> &mut Window {
        self.state.create_window()
    }

    pub fn request_activation_token(
        &mut self,
        surface: &WlSurface,
    ) -> Arc<std::sync::Mutex<Option<String>>> {
        let token = Arc::new(std::sync::Mutex::new(None));
        let activation = self.state.xdg_activation.as_ref().unwrap();
        let request = activation.get_activation_token(&self.qh, token.clone());
        request.set_surface(surface);
        request.commit();
        self.connection.flush().unwrap();
        token
    }

    pub fn activate(&mut self, token: String, surface: &WlSurface) {
        self.state
            .xdg_activation
            .as_ref()
            .unwrap()
            .activate(token, surface);
        self.connection.flush().unwrap();
    }

    pub fn inhibit_shortcuts(&self, surface: &WlSurface) -> ZwpKeyboardShortcutsInhibitorV1 {
        self.state
            .keyboard_shortcuts_inhibit_manager
            .as_ref()
            .unwrap()
            .inhibit_shortcuts(surface, self.state.seat.as_ref().unwrap(), &self.qh, ())
    }

    pub fn set_input_region(&self, surface: &WlSurface, rectangle: Option<(i32, i32, i32, i32)>) {
        let compositor = self.state.compositor.as_ref().unwrap();
        let region = compositor.create_region(&self.qh, ());
        if let Some((x, y, width, height)) = rectangle {
            region.add(x, y, width, height);
        }
        surface.set_input_region(Some(&region));
        surface.commit();
        region.destroy();
        self.connection.flush().unwrap();
    }

    pub fn reset_input_region(&self, surface: &WlSurface) {
        surface.set_input_region(None);
        surface.commit();
        self.connection.flush().unwrap();
    }

    pub fn window(&mut self, surface: &WlSurface) -> &mut Window {
        self.state.window(surface)
    }

    pub fn create_popup(&mut self, parent: &XdgSurface) -> &mut Popup {
        self.state.create_popup(Some(parent), None)
    }

    pub fn create_layer_popup(
        &mut self,
        parent: &ZwlrLayerSurfaceV1,
        offset: (i32, i32),
    ) -> &mut Popup {
        let popup = self.state.create_popup(None, Some(offset));
        parent.get_popup(&popup.xdg_popup);
        popup
    }

    pub fn popup(&mut self, surface: &WlSurface) -> &mut Popup {
        self.state
            .popups
            .iter_mut()
            .find(|popup| popup.surface == *surface)
            .unwrap()
    }

    pub fn create_layer(
        &mut self,
        output: Option<&WlOutput>,
        layer: zwlr_layer_shell_v1::Layer,
        namespace: &str,
    ) -> &mut LayerSurface {
        self.state.create_layer(output, layer, namespace.to_owned())
    }

    pub fn layer(&mut self, surface: &WlSurface) -> &mut LayerSurface {
        self.state.layer(surface)
    }

    pub fn foreign_toplevel(&mut self, title: &str) -> &mut ForeignToplevel {
        self.state
            .foreign_toplevels
            .iter_mut()
            .find(|toplevel| toplevel.title.as_deref() == Some(title))
            .unwrap()
    }

    pub fn output(&mut self, name: &str) -> WlOutput {
        self.state
            .outputs
            .iter()
            .find(|(_, v)| *v == name)
            .unwrap()
            .0
            .clone()
    }

    pub fn capture_output(
        &self,
        output: &WlOutput,
        overlay_cursor: bool,
        region: Option<(i32, i32, i32, i32)>,
    ) -> ScreencopyFrame {
        let events = Arc::new(std::sync::Mutex::new(ScreencopyFrameEvents::default()));
        let manager = self.state.screencopy.as_ref().unwrap();
        let proxy = if let Some((x, y, width, height)) = region {
            manager.capture_output_region(
                overlay_cursor as i32,
                output,
                x,
                y,
                width,
                height,
                &self.qh,
                events.clone(),
            )
        } else {
            manager.capture_output(overlay_cursor as i32, output, &self.qh, events.clone())
        };
        self.connection.flush().unwrap();
        ScreencopyFrame { proxy, events }
    }

    pub fn gamma_control(&mut self, output: &WlOutput) -> &mut GammaControl {
        let manager = self.state.gamma_control_manager.as_ref().unwrap();
        let proxy = manager.get_gamma_control(output, &self.qh, ());
        self.state.gamma_controls.push(GammaControl {
            proxy,
            gamma_size: None,
            failed: false,
        });
        self.state.gamma_controls.last_mut().unwrap()
    }

    pub fn create_shm_buffer(
        &self,
        width: i32,
        height: i32,
        stride: i32,
        format: wl_shm::Format,
        pool_len: usize,
    ) -> WlBuffer {
        let fd = memfd_create("swayward-test-shm", MemfdFlags::CLOEXEC).unwrap();
        ftruncate(&fd, pool_len as u64).unwrap();
        let pool =
            self.state
                .shm
                .as_ref()
                .unwrap()
                .create_pool(fd.as_fd(), pool_len as i32, &self.qh, ());
        let buffer = pool.create_buffer(0, width, height, stride, format, &self.qh, ());
        pool.destroy();
        buffer
    }
}

impl State {
    fn popup_positioner(&self) -> XdgPositioner {
        let positioner = self
            .xdg_wm_base
            .as_ref()
            .unwrap()
            .create_positioner(&self.qh, ());
        positioner.set_size(100, 100);
        positioner.set_anchor_rect(0, 0, 1, 1);
        positioner
    }

    pub fn create_popup(
        &mut self,
        parent: Option<&XdgSurface>,
        offset: Option<(i32, i32)>,
    ) -> &mut Popup {
        let surface = self
            .compositor
            .as_ref()
            .unwrap()
            .create_surface(&self.qh, ());
        let xdg_surface =
            self.xdg_wm_base
                .as_ref()
                .unwrap()
                .get_xdg_surface(&surface, &self.qh, ());
        let positioner = self.popup_positioner();
        if let Some((x, y)) = offset {
            positioner.set_offset(x, y);
            positioner.set_constraint_adjustment(
                ConstraintAdjustment::SlideX | ConstraintAdjustment::SlideY,
            );
        }
        let xdg_popup = xdg_surface.get_popup(parent, &positioner, &self.qh, ());
        positioner.destroy();
        self.popups.push(Popup {
            surface,
            xdg_surface,
            xdg_popup,
            configures_received: Vec::new(),
            repositioned: Vec::new(),
        });
        self.popups.last_mut().unwrap()
    }

    pub fn reposition_popup(&self, popup: &XdgPopup, token: u32) {
        self.reposition_popup_at(popup, token, None);
    }

    pub fn reposition_popup_at(&self, popup: &XdgPopup, token: u32, offset: Option<(i32, i32)>) {
        let positioner = self.popup_positioner();
        if let Some((x, y)) = offset {
            positioner.set_offset(x, y);
            positioner.set_constraint_adjustment(
                ConstraintAdjustment::SlideX | ConstraintAdjustment::SlideY,
            );
        }
        popup.reposition(&positioner, token);
        positioner.destroy();
    }

    pub fn create_window(&mut self) -> &mut Window {
        let compositor = self.compositor.as_ref().unwrap();
        let xdg_wm_base = self.xdg_wm_base.as_ref().unwrap();
        let viewporter = self.viewporter.as_ref().unwrap();

        let surface = compositor.create_surface(&self.qh, ());
        let xdg_surface = xdg_wm_base.get_xdg_surface(&surface, &self.qh, ());
        let xdg_toplevel = xdg_surface.get_toplevel(&self.qh, ());
        let viewport = viewporter.get_viewport(&surface, &self.qh, ());

        let window = Window {
            qh: self.qh.clone(),
            spbm: self.spbm.clone().unwrap(),

            surface,
            xdg_surface,
            xdg_toplevel,
            viewport,
            pending_configure: Configure::default(),
            configures_received: Vec::new(),
            close_requested: false,

            configures_looked_at: 0,
        };

        self.windows.push(window);
        self.windows.last_mut().unwrap()
    }

    pub fn window(&mut self, surface: &WlSurface) -> &mut Window {
        self.windows
            .iter_mut()
            .find(|w| w.surface == *surface)
            .unwrap()
    }

    pub fn create_layer(
        &mut self,
        output: Option<&WlOutput>,
        layer: zwlr_layer_shell_v1::Layer,
        namespace: String,
    ) -> &mut LayerSurface {
        let compositor = self.compositor.as_ref().unwrap();
        let layer_shell = self.layer_shell.as_ref().unwrap();
        let viewporter = self.viewporter.as_ref().unwrap();

        let surface = compositor.create_surface(&self.qh, ());
        let layer_surface =
            layer_shell.get_layer_surface(&surface, output, layer, namespace, &self.qh, ());
        let viewport = viewporter.get_viewport(&surface, &self.qh, ());

        let layer_surface = LayerSurface {
            qh: self.qh.clone(),
            spbm: self.spbm.clone().unwrap(),

            surface,
            layer_surface,
            viewport,
            configures_received: Vec::new(),
            close_requested: false,

            configures_looked_at: 0,
        };

        self.layers.push(layer_surface);
        self.layers.last_mut().unwrap()
    }

    pub fn layer(&mut self, surface: &WlSurface) -> &mut LayerSurface {
        self.layers
            .iter_mut()
            .find(|w| w.surface == *surface)
            .unwrap()
    }
}

impl Window {
    pub fn commit(&self) {
        self.surface.commit();
    }

    pub fn ack_last(&self) {
        let serial = self.configures_received.last().unwrap().0;
        self.ack_configure(serial);
    }

    pub fn ack_configure(&self, serial: u32) {
        self.xdg_surface.ack_configure(serial);
    }

    pub fn ack_last_and_commit(&self) {
        self.ack_last();
        self.commit();
    }

    pub fn attach_new_buffer(&self) {
        let buffer = self.spbm.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, ());
        self.surface.attach(Some(&buffer), 0, 0);
    }

    pub fn attach_null(&self) {
        self.surface.attach(None, 0, 0);
    }

    pub fn set_size(&self, w: u16, h: u16) {
        self.viewport.set_destination(i32::from(w), i32::from(h));
    }

    pub fn set_min_size(&self, width: i32, height: i32) {
        self.xdg_toplevel.set_min_size(width, height);
    }

    pub fn set_max_size(&self, width: i32, height: i32) {
        self.xdg_toplevel.set_max_size(width, height);
    }

    pub fn set_fullscreen(&self, output: Option<&WlOutput>) {
        self.xdg_toplevel.set_fullscreen(output);
    }

    pub fn unset_fullscreen(&self) {
        self.xdg_toplevel.unset_fullscreen();
    }

    pub fn set_maximized(&self) {
        self.xdg_toplevel.set_maximized();
    }

    pub fn unset_maximized(&self) {
        self.xdg_toplevel.unset_maximized();
    }

    pub fn set_parent(&self, parent: Option<&XdgToplevel>) {
        self.xdg_toplevel.set_parent(parent);
    }

    pub fn set_title(&self, title: &str) {
        self.xdg_toplevel.set_title(title.to_owned());
    }

    pub fn destroy_role(&self) {
        self.xdg_toplevel.destroy();
        self.xdg_surface.destroy();
    }

    pub fn recent_configures(&mut self) -> impl Iterator<Item = &Configure> {
        let start = self.configures_looked_at;
        self.configures_looked_at = self.configures_received.len();
        self.configures_received[start..].iter().map(|(_, c)| c)
    }

    pub fn format_recent_configures(&mut self) -> String {
        let mut buf = String::new();
        for configure in self.recent_configures() {
            if !buf.is_empty() {
                buf.push('\n');
            }
            write!(buf, "{configure}").unwrap();
        }
        buf
    }
}

impl LayerSurface {
    pub fn commit(&self) {
        self.surface.commit();
    }

    pub fn ack_last(&self) {
        let serial = self.configures_received.last().unwrap().0;
        self.layer_surface.ack_configure(serial);
    }

    pub fn ack_last_and_commit(&self) {
        self.ack_last();
        self.commit();
    }

    pub fn set_configure_props(&self, props: LayerConfigureProps) {
        let LayerConfigureProps {
            size,
            anchor,
            exclusive_zone,
            margin,
            kb_interactivity,
            layer,
            exclusive_edge,
        } = props;

        if let Some(x) = size {
            self.layer_surface.set_size(x.0, x.1);
        }
        if let Some(x) = anchor {
            self.layer_surface.set_anchor(x);
        }
        if let Some(x) = exclusive_zone {
            self.layer_surface.set_exclusive_zone(x);
        }
        if let Some(x) = margin {
            self.layer_surface
                .set_margin(x.top, x.right, x.bottom, x.left);
        }
        if let Some(x) = kb_interactivity {
            self.layer_surface.set_keyboard_interactivity(x);
        }
        if let Some(x) = layer {
            self.layer_surface.set_layer(x);
        }
        if let Some(x) = exclusive_edge {
            self.layer_surface.set_exclusive_edge(x);
        }
    }

    pub fn attach_new_buffer(&self) {
        let buffer = self.spbm.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, ());
        self.surface.attach(Some(&buffer), 0, 0);
    }

    pub fn attach_null(&self) {
        self.surface.attach(None, 0, 0);
    }

    pub fn set_size(&self, w: u16, h: u16) {
        self.viewport.set_destination(i32::from(w), i32::from(h));
    }

    pub fn recent_configures(&mut self) -> impl Iterator<Item = &LayerConfigure> {
        let start = self.configures_looked_at;
        self.configures_looked_at = self.configures_received.len();
        self.configures_received[start..].iter().map(|(_, c)| c)
    }

    pub fn format_recent_configures(&mut self) -> String {
        let mut buf = String::new();
        for configure in self.recent_configures() {
            if !buf.is_empty() {
                buf.push('\n');
            }
            write!(buf, "{configure}").unwrap();
        }
        buf
    }
}

impl Dispatch<WlCallback, Arc<SyncData>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlCallback,
        event: <WlCallback as wayland_client::Proxy>::Event,
        data: &Arc<SyncData>,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_callback::Event::Done { .. } => data.done.store(true, Ordering::Relaxed),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                if interface == WlCompositor::interface().name {
                    let version = min(version, WlCompositor::interface().version);
                    state.compositor = Some(registry.bind(name, version, qh, ()));
                } else if interface == WlSeat::interface().name {
                    let version = min(version, WlSeat::interface().version);
                    state.seat = Some(registry.bind(name, version, qh, ()));
                } else if interface == XdgWmBase::interface().name {
                    let version = min(version, XdgWmBase::interface().version);
                    state.xdg_wm_base = Some(registry.bind(name, version, qh, ()));
                    state.xdg_wm_base_version = Some(version);
                } else if interface == XdgActivationV1::interface().name {
                    let version = min(version, XdgActivationV1::interface().version);
                    state.xdg_activation = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwpKeyboardShortcutsInhibitManagerV1::interface().name {
                    let version = min(
                        version,
                        ZwpKeyboardShortcutsInhibitManagerV1::interface().version,
                    );
                    state.keyboard_shortcuts_inhibit_manager =
                        Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrLayerShellV1::interface().name {
                    let version = min(version, ZwlrLayerShellV1::interface().version);
                    state.layer_shell = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrForeignToplevelManagerV1::interface().name {
                    let version = min(version, ZwlrForeignToplevelManagerV1::interface().version);
                    state.foreign_toplevel_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ExtWorkspaceManagerV1::interface().name {
                    let version = min(version, ExtWorkspaceManagerV1::interface().version);
                    state.ext_workspace_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrOutputManagerV1::interface().name {
                    let version = min(version, ZwlrOutputManagerV1::interface().version);
                    state.output_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrVirtualPointerManagerV1::interface().name {
                    let version = min(version, ZwlrVirtualPointerManagerV1::interface().version);
                    state.virtual_pointer_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == WpSinglePixelBufferManagerV1::interface().name {
                    let version = min(version, WpSinglePixelBufferManagerV1::interface().version);
                    state.spbm = Some(registry.bind(name, version, qh, ()));
                } else if interface == WpViewporter::interface().name {
                    let version = min(version, WpViewporter::interface().version);
                    state.viewporter = Some(registry.bind(name, version, qh, ()));
                } else if interface == WlShm::interface().name {
                    let version = min(version, WlShm::interface().version);
                    state.shm = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrScreencopyManagerV1::interface().name {
                    let version = min(version, ZwlrScreencopyManagerV1::interface().version);
                    state.screencopy = Some(registry.bind(name, version, qh, ()));
                } else if interface == ZwlrGammaControlManagerV1::interface().name {
                    let version = min(version, ZwlrGammaControlManagerV1::interface().version);
                    state.gamma_control_manager = Some(registry.bind(name, version, qh, ()));
                } else if interface == MutterX11Interop::interface().name {
                    let version = min(version, MutterX11Interop::interface().version);
                    state.mutter_x11_interop = Some(registry.bind(name, version, qh, ()));
                } else if interface == WlOutput::interface().name {
                    let version = min(version, WlOutput::interface().version);
                    let output = registry.bind(name, version, qh, ());
                    state.outputs.insert(output, String::new());
                }

                let global = Global {
                    name,
                    interface,
                    version,
                };
                state.globals.push(global);
            }
            wl_registry::Event::GlobalRemove { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn event(
        state: &mut Self,
        output: &WlOutput,
        event: <WlOutput as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Geometry { .. } => (),
            wl_output::Event::Mode { .. } => (),
            wl_output::Event::Done => (),
            wl_output::Event::Scale { .. } => (),
            wl_output::Event::Name { name } => {
                *state.outputs.get_mut(output).unwrap() = name;
            }
            wl_output::Event::Description { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_seat::Event::Capabilities { .. } => (),
            wl_seat::Event::Name { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlCompositor,
        _event: <WlCompositor as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgActivationV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgActivationV1,
        _event: xdg_activation_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgActivationTokenV1, Arc<std::sync::Mutex<Option<String>>>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgActivationTokenV1,
        event: xdg_activation_token_v1::Event,
        token: &Arc<std::sync::Mutex<Option<String>>>,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_activation_token_v1::Event::Done { token: value } => {
                *token.lock().unwrap() = Some(value);
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpKeyboardShortcutsInhibitManagerV1,
        _event: zwp_keyboard_shortcuts_inhibit_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwpKeyboardShortcutsInhibitorV1,
        event: zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Active => {
                state.shortcut_inhibitor_events.push(true)
            }
            zwp_keyboard_shortcuts_inhibitor_v1::Event::Inactive => {
                state.shortcut_inhibitor_events.push(false)
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn event(
        _state: &mut Self,
        xdg_wm_base: &XdgWmBase,
        event: <XdgWmBase as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_wm_base::Event::Ping { serial } => {
                xdg_wm_base.pong(serial);
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                state.workspace_groups.push(WorkspaceGroup {
                    handle: workspace_group,
                    outputs: Vec::new(),
                    workspaces: Vec::new(),
                    removed: false,
                });
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                state.ext_workspaces.push(ExtWorkspace {
                    handle: workspace,
                    id: None,
                    name: None,
                    removed: false,
                });
            }
            ext_workspace_manager_v1::Event::Done | ext_workspace_manager_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(State, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_group_handle_v1::Event::OutputEnter { output } => state
                .workspace_groups
                .iter_mut()
                .find(|candidate| candidate.handle == *group)
                .unwrap()
                .outputs
                .push(output),
            ext_workspace_group_handle_v1::Event::OutputLeave { output } => state
                .workspace_groups
                .iter_mut()
                .find(|candidate| candidate.handle == *group)
                .unwrap()
                .outputs
                .retain(|candidate| candidate != &output),
            ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } => {
                state
                    .workspace_membership_events
                    .push((workspace.clone(), group.clone(), true));
                state
                    .workspace_groups
                    .iter_mut()
                    .find(|candidate| candidate.handle == *group)
                    .unwrap()
                    .workspaces
                    .push(workspace);
            }
            ext_workspace_group_handle_v1::Event::WorkspaceLeave { workspace } => {
                state
                    .workspace_membership_events
                    .push((workspace.clone(), group.clone(), false));
                state
                    .workspace_groups
                    .iter_mut()
                    .find(|candidate| candidate.handle == *group)
                    .unwrap()
                    .workspaces
                    .retain(|candidate| candidate != &workspace);
            }
            ext_workspace_group_handle_v1::Event::Removed => {
                state
                    .workspace_groups
                    .iter_mut()
                    .find(|candidate| candidate.handle == *group)
                    .unwrap()
                    .removed = true;
            }
            ext_workspace_group_handle_v1::Event::Capabilities { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        workspace: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let workspace = state
            .ext_workspaces
            .iter_mut()
            .find(|candidate| candidate.handle == *workspace)
            .unwrap();
        match event {
            ext_workspace_handle_v1::Event::Id { id } => workspace.id = Some(id),
            ext_workspace_handle_v1::Event::Name { name } => workspace.name = Some(name),
            ext_workspace_handle_v1::Event::Removed => workspace.removed = true,
            ext_workspace_handle_v1::Event::Coordinates { .. }
            | ext_workspace_handle_v1::Event::State { .. }
            | ext_workspace_handle_v1::Event::Capabilities { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.foreign_toplevels.push(ForeignToplevel {
                    handle: toplevel,
                    title: None,
                });
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(
        State,
        ZwlrForeignToplevelManagerV1,
        [zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ())]
    );
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let toplevel = state
            .foreign_toplevels
            .iter_mut()
            .find(|toplevel| toplevel.handle == *handle)
            .unwrap();
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                toplevel.title = Some(title);
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::Closed => (),
            zwlr_foreign_toplevel_handle_v1::Event::OutputEnter { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::OutputLeave { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::State { .. }
            | zwlr_foreign_toplevel_handle_v1::Event::Done
            | zwlr_foreign_toplevel_handle_v1::Event::Parent { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrLayerShellV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrLayerShellV1,
        _event: <ZwlrLayerShellV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlSurface,
        event: <WlSurface as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_surface::Event::Enter { .. } => (),
            wl_surface::Event::Leave { .. } => (),
            wl_surface::Event::PreferredBufferScale { .. } => (),
            wl_surface::Event::PreferredBufferTransform { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgPositioner, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &XdgPositioner,
        _event: <XdgPositioner as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<XdgPopup, ()> for State {
    fn event(
        state: &mut Self,
        popup: &XdgPopup,
        event: xdg_popup::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let popup = state
            .popups
            .iter_mut()
            .find(|candidate| candidate.xdg_popup == *popup)
            .unwrap();
        match event {
            xdg_popup::Event::Configure {
                x,
                y,
                width,
                height,
            } => popup.configures_received.push((x, y, width, height)),
            xdg_popup::Event::PopupDone => (),
            xdg_popup::Event::Repositioned { token } => popup.repositioned.push(token),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgSurface, ()> for State {
    fn event(
        state: &mut Self,
        xdg_surface: &XdgSurface,
        event: <XdgSurface as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            xdg_surface::Event::Configure { serial } => {
                if let Some(window) = state
                    .windows
                    .iter_mut()
                    .find(|window| window.xdg_surface == *xdg_surface)
                {
                    let configure = window.pending_configure.clone();
                    window.configures_received.push((serial, configure));
                } else if let Some(popup) = state
                    .popups
                    .iter()
                    .find(|popup| popup.xdg_surface == *xdg_surface)
                {
                    popup.xdg_surface.ack_configure(serial);
                } else {
                    panic!("configure for unknown xdg_surface")
                }
            }
            _ => unreachable!(),
        }
    }
}

impl Dispatch<XdgToplevel, ()> for State {
    fn event(
        state: &mut Self,
        xdg_toplevel: &XdgToplevel,
        event: <XdgToplevel as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let window = state
            .windows
            .iter_mut()
            .find(|w| w.xdg_toplevel == *xdg_toplevel)
            .unwrap();

        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                let configure = &mut window.pending_configure;
                configure.size = (width, height);
                configure.states = states
                    .chunks_exact(4)
                    .flat_map(TryInto::<[u8; 4]>::try_into)
                    .map(u32::from_ne_bytes)
                    .flat_map(xdg_toplevel::State::try_from)
                    .collect();
            }
            xdg_toplevel::Event::Close => {
                window.close_requested = true;
            }
            xdg_toplevel::Event::ConfigureBounds { width, height } => {
                window.pending_configure.bounds = Some((width, height));
            }
            xdg_toplevel::Event::WmCapabilities { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrVirtualPointerManagerV1,
        _event: <ZwlrVirtualPointerManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrVirtualPointerV1,
        _event: zwlr_virtual_pointer_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrOutputManagerV1,
        event: zwlr_output_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Head { head } => {
                state.output_heads.push(OutputHead {
                    proxy: head,
                    name: None,
                    modes: Vec::new(),
                });
            }
            zwlr_output_manager_v1::Event::Done { serial } => {
                state.output_manager_serials.push(serial);
            }
            zwlr_output_manager_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(State, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for State {
    fn event(
        state: &mut Self,
        head: &ZwlrOutputHeadV1,
        event: zwlr_output_head_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let output = state
            .output_heads
            .iter_mut()
            .find(|output| output.proxy == *head)
            .unwrap();
        match event {
            zwlr_output_head_v1::Event::Name { name } => output.name = Some(name),
            zwlr_output_head_v1::Event::Mode { mode } => output.modes.push(mode),
            zwlr_output_head_v1::Event::Description { .. }
            | zwlr_output_head_v1::Event::PhysicalSize { .. }
            | zwlr_output_head_v1::Event::Enabled { .. }
            | zwlr_output_head_v1::Event::CurrentMode { .. }
            | zwlr_output_head_v1::Event::Position { .. }
            | zwlr_output_head_v1::Event::Transform { .. }
            | zwlr_output_head_v1::Event::Scale { .. }
            | zwlr_output_head_v1::Event::Make { .. }
            | zwlr_output_head_v1::Event::Model { .. }
            | zwlr_output_head_v1::Event::SerialNumber { .. }
            | zwlr_output_head_v1::Event::AdaptiveSync { .. }
            | zwlr_output_head_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }

    wayland_client::event_created_child!(State, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrOutputModeV1,
        event: zwlr_output_mode_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_mode_v1::Event::Size { .. }
            | zwlr_output_mode_v1::Event::Refresh { .. }
            | zwlr_output_mode_v1::Event::Preferred
            | zwlr_output_mode_v1::Event::Finished => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrOutputConfigurationV1,
        event: zwlr_output_configuration_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let result = match event {
            zwlr_output_configuration_v1::Event::Succeeded => OutputConfigurationResult::Succeeded,
            zwlr_output_configuration_v1::Event::Failed => OutputConfigurationResult::Failed,
            zwlr_output_configuration_v1::Event::Cancelled => OutputConfigurationResult::Cancelled,
            _ => unreachable!(),
        };
        state.output_configuration_results.push(result);
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrOutputConfigurationHeadV1,
        _event: zwlr_output_configuration_head_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for State {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: <ZwlrLayerSurfaceV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let layer_surface = state
            .layers
            .iter_mut()
            .find(|w| w.layer_surface == *layer_surface)
            .unwrap();

        match event {
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
                let configure = LayerConfigure {
                    size: (width, height),
                };
                layer_surface.configures_received.push((serial, configure));
            }
            zwlr_layer_surface_v1::Event::Closed => layer_surface.close_requested = true,
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlRegion, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegion,
        _event: <WlRegion as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WlShm, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlShm,
        event: wl_shm::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_shm::Event::Format { .. } => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlShmPool, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlShmPool,
        _event: <WlShmPool as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<MutterX11Interop, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &MutterX11Interop,
        _event: <MutterX11Interop as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrGammaControlManagerV1,
        _event: <ZwlrGammaControlManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrGammaControlV1, ()> for State {
    fn event(
        state: &mut Self,
        proxy: &ZwlrGammaControlV1,
        event: <ZwlrGammaControlV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let control = state
            .gamma_controls
            .iter_mut()
            .find(|control| control.proxy == *proxy)
            .unwrap();
        match event {
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                control.gamma_size = Some(size);
            }
            zwlr_gamma_control_v1::Event::Failed => control.failed = true,
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ZwlrScreencopyManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrScreencopyManagerV1,
        _event: <ZwlrScreencopyManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, Arc<std::sync::Mutex<ScreencopyFrameEvents>>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &ZwlrScreencopyFrameV1,
        event: zwlr_screencopy_frame_v1::Event,
        data: &Arc<std::sync::Mutex<ScreencopyFrameEvents>>,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let mut data = data.lock().unwrap();
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => data.buffer = Some((format.into_result().unwrap(), width, height, stride)),
            zwlr_screencopy_frame_v1::Event::LinuxDmabuf {
                format,
                width,
                height,
            } => data.linux_dmabuf = Some((format, width, height)),
            zwlr_screencopy_frame_v1::Event::BufferDone => data.buffer_done = true,
            zwlr_screencopy_frame_v1::Event::Flags { .. } => (),
            zwlr_screencopy_frame_v1::Event::Ready { .. } => data.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => data.failed = true,
            zwlr_screencopy_frame_v1::Event::Damage {
                x,
                y,
                width,
                height,
            } => data.damage.push((x, y, width, height)),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WlBuffer,
        event: <WlBuffer as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            wl_buffer::Event::Release => (),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<WpSinglePixelBufferManagerV1, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpSinglePixelBufferManagerV1,
        _event: <WpSinglePixelBufferManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WpViewporter, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpViewporter,
        _event: <WpViewporter as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<WpViewport, ()> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpViewport,
        _event: <WpViewport as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

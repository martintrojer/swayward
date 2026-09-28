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
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::client::{
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
    zxdg_toplevel_decoration_v1::{self, ZxdgToplevelDecorationV1},
};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_popup::{self, XdgPopup};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_positioner::{
    ConstraintAdjustment, XdgPositioner,
};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel::{self, XdgToplevel};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};
use smithay::reexports::wayland_protocols::xdg::toplevel_tag::v1::client::xdg_toplevel_tag_manager_v1::XdgToplevelTagManagerV1;
use smithay::reexports::wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1::{self, ExtSessionLockManagerV1},
    ext_session_lock_surface_v1::{self, ExtSessionLockSurfaceV1},
    ext_session_lock_v1::{self, ExtSessionLockV1},
};
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
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
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
    pub keyboard: Option<WlKeyboard>,
    pub keyboard_enter_serial: Option<u32>,
    pub xdg_wm_base: Option<XdgWmBase>,
    pub xdg_wm_base_version: Option<u32>,
    pub xdg_activation: Option<XdgActivationV1>,
    pub xdg_decoration_manager: Option<ZxdgDecorationManagerV1>,
    pub xdg_toplevel_tag_manager: Option<XdgToplevelTagManagerV1>,
    pub keyboard_shortcuts_inhibit_manager: Option<ZwpKeyboardShortcutsInhibitManagerV1>,
    pub shortcut_inhibitor_events: Vec<bool>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub foreign_toplevel_manager: Option<ZwlrForeignToplevelManagerV1>,
    pub foreign_toplevels: Vec<ForeignToplevel>,
    pub ext_workspace_manager: Option<ExtWorkspaceManagerV1>,
    pub session_lock_manager: Option<ExtSessionLockManagerV1>,
    pub session_locked: bool,
    pub lock_surfaces: Vec<SessionLockSurface>,
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

pub struct SessionLockSurface {
    pub surface: WlSurface,
    pub lock_surface: ExtSessionLockSurfaceV1,
    pub viewport: WpViewport,
    pub configure: Option<(u32, u32, u32)>,
}

pub struct Window {
    pub qh: QueueHandle<State>,
    pub spbm: WpSinglePixelBufferManagerV1,

    pub surface: WlSurface,
    pub xdg_surface: XdgSurface,
    pub xdg_toplevel: XdgToplevel,
    pub xdg_decoration: Option<ZxdgToplevelDecorationV1>,
    pub decoration_modes: Vec<zxdg_toplevel_decoration_v1::Mode>,
    pub viewport: WpViewport,
    pub pending_configure: Configure,
    pub configures_received: Vec<(u32, Configure)>,
    pub close_requested: bool,

    pub configures_looked_at: usize,
    pub last_acked_configure: Option<u32>,
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
            keyboard: None,
            keyboard_enter_serial: None,
            xdg_wm_base: None,
            xdg_wm_base_version: None,
            xdg_activation: None,
            xdg_decoration_manager: None,
            xdg_toplevel_tag_manager: None,
            keyboard_shortcuts_inhibit_manager: None,
            shortcut_inhibitor_events: Vec::new(),
            layer_shell: None,
            foreign_toplevel_manager: None,
            foreign_toplevels: Vec::new(),
            ext_workspace_manager: None,
            session_lock_manager: None,
            session_locked: false,
            lock_surfaces: Vec::new(),
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

    pub fn decorate_last_window(&mut self, mode: zxdg_toplevel_decoration_v1::Mode) {
        let state = &mut self.state;
        let manager = state.xdg_decoration_manager.as_ref().unwrap();
        let window = state.windows.last_mut().unwrap();
        let decoration = manager.get_toplevel_decoration(&window.xdg_toplevel, &state.qh, ());
        decoration.set_mode(mode);
        window.xdg_decoration = Some(decoration);
        self.connection.flush().unwrap();
    }

    pub fn request_decoration_mode(&self, mode: zxdg_toplevel_decoration_v1::Mode) {
        self.state
            .windows
            .last()
            .unwrap()
            .xdg_decoration
            .as_ref()
            .unwrap()
            .set_mode(mode);
        self.connection.flush().unwrap();
    }

    pub fn request_activation_token(
        &mut self,
        surface: &WlSurface,
    ) -> Arc<std::sync::Mutex<Option<String>>> {
        self.request_activation_token_with_focus(Some(surface))
    }

    pub fn request_activation_token_with_focus(
        &mut self,
        surface: Option<&WlSurface>,
    ) -> Arc<std::sync::Mutex<Option<String>>> {
        let token = Arc::new(std::sync::Mutex::new(None));
        let activation = self.state.xdg_activation.as_ref().unwrap();
        let request = activation.get_activation_token(&self.qh, token.clone());
        if let Some(surface) = surface {
            request.set_surface(surface);
            request.set_serial(
                self.state.keyboard_enter_serial.unwrap(),
                self.state.seat.as_ref().unwrap(),
            );
        }
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

    pub fn set_toplevel_tag(&self, toplevel: &XdgToplevel, tag: &str) {
        self.state
            .xdg_toplevel_tag_manager
            .as_ref()
            .unwrap()
            .set_toplevel_tag(toplevel, tag.to_owned());
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

    /// Like [`Self::create_shm_buffer`], but keeps the pool's memfd so a test can read back
    /// what the compositor wrote into it.
    pub fn create_readable_shm_buffer(
        &self,
        width: i32,
        height: i32,
        stride: i32,
        format: wl_shm::Format,
    ) -> (WlBuffer, std::os::fd::OwnedFd) {
        let pool_len = (stride * height) as usize;
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
        (buffer, fd)
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


use std::cell::Cell;

use calloop::Interest;
use smithay::backend::input::InputTime;
use smithay::desktop::{
    find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output, utils, LayerSurface,
    PopupKeyboardGrab, PopupKind, PopupManager, PopupPointerGrab, PopupUngrabStrategy, Window,
    WindowSurfaceType,
};
use smithay::input::pointer::Focus;
use smithay::input::tablet::TabletSeatTrait;
use smithay::output::Output;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_positioner::ConstraintAdjustment;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::{self};
use smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration;
use smithay::reexports::wayland_server::protocol::wl_output;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{self, Resource, WEnum};
use smithay::utils::{Logical, Rectangle, Serial};
use smithay::wayland::compositor::{
    add_blocker, add_pre_commit_hook, with_states, BufferAssignment, CompositorHandler as _,
    HookId, SurfaceAttributes,
};
use smithay::wayland::dmabuf::get_dmabuf;
use smithay::wayland::input_method::InputMethodSeat;
use smithay::wayland::shell::kde::decoration::{KdeDecorationHandler, KdeDecorationState};
use smithay::wayland::shell::wlr_layer::{self, Layer};
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    XdgToplevelSurfaceData,
};
use smithay::wayland::xdg_foreign::{XdgForeignHandler, XdgForeignState};
use smithay::wayland::xdg_toplevel_tag::XdgToplevelTagHandler;
use swayward_config::PresetSize;
use swayward_ipc::command::AssignmentTarget;
use tracing::field::Empty;

use crate::input::move_grab::MoveGrab;
use crate::input::resize_grab::ResizeGrab;
use crate::input::{AnyStartData, DOUBLE_CLICK_TIME};
use crate::layout::{ActivateWindow, LayoutElement as _};
use crate::swayward::{CastTarget, PopupGrabState, State};
use crate::utils::transaction::Transaction;
use crate::utils::{
    get_monotonic_time, output_matches_name, send_scale_transform, update_tiled_state, ResizeEdge,
};
use crate::window::{InitialConfigureState, ResolvedWindowRules, Unmapped, WindowRef};

mod configure;
mod decorations;
mod requests;

pub use configure::add_mapped_toplevel_pre_commit_hook;
pub use decorations::KdeDecorationsModeState;

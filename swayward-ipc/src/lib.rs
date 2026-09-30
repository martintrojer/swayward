//! Rust types and blocking clients for sway-compatible IPC.
//!
//! [`sway_socket::SwaySocket`] connects to `$SWAYSOCK`, frames requests with sway's binary
//! header, and returns raw JSON for schemas that this crate does not model. [`MessageType`] selects
//! the request. After a subscribe request, use [`sway_socket::SwaySocket::read_event`] to read raw
//! event IDs and payloads.
//!
//! [`socket::Socket`] and [`legacy`] implement swayward's separate line-delimited JSON protocol on
//! `$SWAYWARD_SOCKET`. New sway-compatible clients should not use that legacy endpoint.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::string_slice,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod command;
pub mod criteria;
mod event;
pub mod legacy;
mod message;
pub mod socket;
pub mod state;
pub mod sway_socket;
mod tree;
pub mod wire;

pub use event::*;
pub use legacy::{
    Action, Cast, CastKind, CastTarget, ColumnDisplay, ConfiguredMode, ConfiguredPosition,
    HSyncPolarity, KeyboardLayouts, Layer, LayerSurface, LayerSurfaceKeyboardInteractivity,
    LayoutSwitchTarget, LogicalOutput, MaxBpc, Mode, ModeToSet, OutputAction, OutputConfigChanged,
    Overview, PickedColor, PositionChange, PositionToSet, Reply, Request, Response, ScaleToSet,
    SizeChange, Timestamp, Transform, VSyncPolarity, VrrToSet, Window, WindowLayout,
    WorkspaceReferenceArg,
};
pub use message::*;
pub use tree::*;

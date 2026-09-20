//! Rust types for sway's IPC JSON schemas.

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

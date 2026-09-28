//! niri config parsing.
//!
//! The config can be constructed from multiple files (includes). To support this, many types are
//! split into two. For example, `Layout` and `LayoutPart` where `Layout` is the final config and
//! `LayoutPart` is one part parsed from one config file.
//!
//! The convention for `Default` impls is to set the initial values before the parsing occurs.
//! Then, parsing will update the values with those parsed from the config.
//!
//! The `Default` values match those from `default-config.kdl` in almost all cases, with a notable
//! exception of `binds {}` and some window rules.

#[macro_use]
extern crate tracing;

#[macro_use]
pub mod macros;

pub mod animations;
pub mod appearance;
pub mod binds;
pub mod debug;
pub mod error;
pub mod gestures;
pub mod input;
pub mod layer_rule;
pub mod layout;
pub mod misc;
pub mod output;
pub mod recent_windows;
pub mod utils;
pub mod window_rule;
pub mod workspace;

pub use crate::animations::{Animation, Animations};
pub use crate::appearance::*;
pub use crate::binds::*;
pub use crate::debug::Debug;
pub use crate::error::{ConfigIncludeError, ConfigParseResult};
pub use crate::gestures::Gestures;
pub use crate::input::{
    FloatingModifier, FocusFollowsMouse, FocusFollowsMouseMode, Input, ModKey, MouseWarping,
    ScrollMethod, TrackLayout, WarpMouseToFocusMode, Xkb,
};
pub use crate::layer_rule::LayerRule;
pub use crate::layout::*;
pub use crate::misc::*;
pub use crate::output::{Output, OutputName, Outputs, Position, Vrr};
pub use crate::recent_windows::{MruDirection, MruFilter, MruPreviews, MruScope, RecentWindows};
pub use crate::utils::FloatOrInt;
pub use crate::window_rule::{
    FloatingPosition, OnXdgActivate, PopupsRule, RelativeTo, ResolvedPopupsRules,
    SwayWindowBorderStyle, WindowRule,
};
pub use crate::workspace::{Workspace, WorkspaceLayoutPart};

mod config;
mod decode;
mod loader;

pub use config::Config;
pub use loader::ConfigPath;

#[cfg(test)]
mod tests;

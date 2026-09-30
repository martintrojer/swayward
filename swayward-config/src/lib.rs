//! Parsing and resolved values for swayward's KDL configuration.
//!
//! [`Config`] is the resolved configuration used by the compositor. Types whose names end in
//! `Part`, such as [`LayoutPart`], represent values from one file before includes and overrides are
//! merged. Their `Option` fields distinguish an omitted setting from an explicit value.
//!
//! Unless a field says otherwise, dimensions use logical pixels, durations with an `_ms` suffix
//! use milliseconds, and proportions use values from `0.0` to `1.0`. `Default` supplies parser
//! defaults before values from the config and its includes are merged. These defaults usually
//! match `resources/default-config.kdl`; bindings and some window rules are exceptions.

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
pub use crate::utils::{FloatOrInt, PositiveFloatOrInt};
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

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use proptest::prelude::*;
use smithay::output::{self, Output};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Point, Serial, Transform};
use swayward_ipc::command::{BorderStyle, LayoutToggle};

use super::*;
use crate::animation::Clock;
use crate::layout::tile::Tile;
use crate::layout::{
    titlebar, ConfigureIntent, InteractiveResizeData, LayoutElementRenderSnapshot, Options,
    SizingMode,
};
use crate::render_helpers::offscreen::OffscreenData;

mod decorations;
mod fixtures;
mod focus_movement;
mod fullscreen;
mod geometry;
mod invariants;
mod movement;
mod properties;
mod rendering;
mod resize;
mod tile;
mod transfer;
mod tree_mutation;

use fixtures::*;

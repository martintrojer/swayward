//! Types for communicating with niri via IPC.
//!
//! After connecting to the niri socket, you can send [`Request`]s. Niri will process them one by
//! one, in order, and to each request it will respond with a single [`Reply`], which is a `Result`
//! wrapping a [`Response`].
//!
//! If you send a [`Request::EventStream`], niri will *stop* reading subsequent [`Request`]s, and
//! will start continuously writing compositor [`Event`]s to the socket. If you'd like to read an
//! event stream and write more requests at the same time, you need to use two IPC sockets.
//!
//! <div class="warning">
//!
//! Requests are *always* processed separately. Time passes between requests, even when sending
//! multiple requests to the socket at once. For example, sending [`Request::Workspaces`] and
//! [`Request::Windows`] together may not return consistent results (e.g. a window may open on a
//! new workspace in-between the two responses). This goes for actions too: sending
//! [`Action::FocusWindow`] and <code>[Action::CloseWindow] { id: None }</code> together may close
//! the wrong window because a different window got focused in-between these requests.
//!
//! </div>
//!
//! You can use the [`socket::Socket`] helper if you're fine with blocking communication. However,
//! it is a fairly simple helper, so if you need async, or if you're using a different language,
//! you are encouraged to communicate with the socket manually.
//!
//! 1. Read the socket filesystem path from [`socket::SOCKET_PATH_ENV`] (`$SWAYWARD_SOCKET`).
//! 2. Connect to the socket and write a JSON-formatted [`Request`] on a single line. You can follow
//!    up with a line break and a flush, or just flush and shutdown the write end of the socket.
//! 3. Niri will respond with a single line JSON-formatted [`Reply`].
//! 4. You can keep writing [`Request`]s, each on a single line, and read [`Reply`]s, also each on a
//!    separate line.
//! 5. After you request an event stream, niri will keep responding with JSON-formatted [`Event`]s,
//!    on a single line each.
//!
//! ## Backwards compatibility
//!
//! This crate follows the niri version. It is **not** API-stable in terms of the Rust semver. In
//! particular, expect new struct fields and enum variants to be added in patch version bumps.
//!
//! Use an exact version requirement to avoid breaking changes:
//!
//! ```toml
//! [dependencies]
//! swayward-ipc = "=26.4.0"
//! ```
//!
//! ## Features
//!
//! This crate defines the following features:
//! - `json-schema`: derives the [schemars](https://lib.rs/crates/schemars) `JsonSchema` trait for
//!   the types.
//! - `clap`: derives the clap CLI parsing traits for some types. Used internally by niri itself.
#![warn(missing_docs)]

use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

mod action;
mod event;
mod output;
mod parse;
mod request;
mod types;

pub use action::*;
pub use event::*;
pub use output::*;
pub use request::*;
pub use types::*;

#[cfg(test)]
mod tests;

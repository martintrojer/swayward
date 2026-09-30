//! Types for swayward's legacy line-delimited JSON protocol.
//!
//! This protocol uses `$SWAYWARD_SOCKET`. New clients that need sway-compatible IPC should use
//! [`crate::sway_socket::SwaySocket`] with `$SWAYSOCK` instead.
//!
//! After connecting to the legacy socket, a client can send [`Request`]s. Swayward processes each
//! request in order and responds with one [`Reply`], which wraps a [`Response`].
//!
//! After [`Request::EventStream`], swayward stops reading [`Request`]s and continuously writes
//! compositor [`Event`]s to the socket. Use two connections to read events and send requests at
//! the same time.
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
//! 3. Swayward responds with one JSON-formatted [`Reply`] on a single line.
//! 4. You can keep writing [`Request`]s and reading [`Reply`]s, each on a separate line.
//! 5. After you request an event stream, swayward keeps responding with one JSON-formatted
//!    [`Event`] per line.
//!
//! ## Backwards compatibility
//!
//! This legacy API is not stable under Rust semantic versioning. New struct fields and enum
//! variants can appear in patch releases. Pin an exact version if those additions would break
//! your client.
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
//! - `clap`: derives clap command-line parsing traits for selected types.
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

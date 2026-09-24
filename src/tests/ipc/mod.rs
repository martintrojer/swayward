//! IPC conformance tests. The empirical coverage boundary and known gaps are
//! recorded in `docs/IPC_ORACLE_COVERAGE.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use swayward_config::OutputName;
use swayward_ipc::MessageType;
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::*;
use crate::ipc::tree::{describe_outputs, describe_tree, describe_workspaces};
use crate::layout::tiling_tree::{IpcNode, Layout as TreeLayout, NodeId};
use crate::layout::LayoutElement as _;

include!("fixtures.rs");
include!("wire.rs");
include!("events.rs");
include!("outputs.rs");
include!("bindings.rs");
include!("config_commands.rs");
include!("tree_commands.rs");
include!("workspace_commands.rs");
include!("focus_and_move.rs");
include!("workspace_output.rs");
include!("floating_commands.rs");
include!("misc_commands.rs");

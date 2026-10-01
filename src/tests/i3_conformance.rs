//! Runner for unmodified layout tests from i3's Perl testsuite.

use std::any::Any;
use std::collections::HashSet;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::Fixture;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

fn pause_i3_poll() {
    thread::sleep(Duration::from_millis(1));
}

fn oracle_i3_dir() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/sway-ipc-oracle/i3");
    assert!(
        path.join("t").is_dir(),
        "i3 oracle is missing; run ./contrib/fetch-oracle"
    );
    path
}

mod adapter;
mod allowlist;
mod docs_checks;
mod harness_tests;
mod manifest;
mod runner;

pub(in crate::tests) use adapter::reload_test_config;
use adapter::*;
use allowlist::*;
use manifest::*;
use runner::*;

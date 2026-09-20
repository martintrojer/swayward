//! Runner for unmodified layout tests from i3's Perl testsuite.

use std::any::Any;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::Fixture;
use crate::utils::transaction::Transaction;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

struct AllowedRejection {
    test: &'static str,
    command: &'static str,
    reason: &'static str,
    /// Allow this command to be rejected any number of times.
    ///
    /// Only for a file whose own input is randomised, where the count is not
    /// reproducible. Every other entry stays exact, so an unexpected rejection
    /// or a changed count still fails.
    repeatable: bool,
}

impl AllowedRejection {
    fn matches(&self, test: &str, command: &str) -> bool {
        self.test == test
            && (self.command == command
                || (self.command == "[con_mark=\"*\"] focus"
                    && command.starts_with("[con_mark=\"")
                    && command.ends_with("\"] focus"))
                || (self.command == "[con_mark=a] move to workspace *"
                    && command.starts_with("[con_mark=a] move to workspace "))
                || (self.command == "[id= . *] focus output right"
                    && (command.starts_with("[id= . ") || command.starts_with("[con_id= . "))
                    && command.ends_with("] focus output right"))
                || (self.command == "[id=*] swap container with id *"
                    && command.starts_with("[id=")
                    && command.contains("] swap container with id "))
                || (self.command == "[app_id=b] swap with id *"
                    && command.starts_with("[app_id=b] swap with id ")))
    }
}

// Every rejection from a passing conformance file must be reviewed here. Keying by both file and
// exact command prevents a new rejected setup command from hiding behind an unrelated exception.
const ALLOWED_REJECTIONS: &[AllowedRejection] = &[
    AllowedRejection {
        test: "308-focus_wrapping.t",
        command: "focus top",
        repeatable: true,
        reason: "`top` is not a focus direction in sway: cmd_focus accepts no such \
                 argument (sway/sway/commands/focus.c), so rejecting it is correct. \
                 The file's own random subtest emits it, and every assertion that \
                 does not depend on it passes",
    },
    AllowedRejection {
        test: "176-workspace-baf.t",
        command: "restart",
        repeatable: false,
        reason: "sway has no runtime restart command; coverage classifies the dependent assertion as unproven",
    },
    AllowedRejection {
        test: "111-goto.t",
        command: "[con_mark=\"*\"] focus",
        repeatable: false,
        reason: "test asserts that an unknown mark leaves focus unchanged",
    },
    AllowedRejection {
        test: "218-regress-floating-split.t",
        command: "layout stacked",
        repeatable: false,
        reason: "sway rejects layout changes on floating windows",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"nomatch\"] scratchpad show",
        repeatable: false,
        reason: "the test expects unmatched criteria to leave focus unchanged",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"non-scratch\"] scratchpad show",
        repeatable: false,
        reason: "the test expects a matching non-scratchpad window to remain unchanged",
    },
    AllowedRejection {
        test: "202-scratchpad-criteria.t",
        command: "[title=\"nothingmatchthistitle\"] scratchpad show",
        repeatable: false,
        reason: "the test expects unmatched criteria to leave focus unchanged",
    },
    AllowedRejection {
        test: "184-regress-float-split-resize.t",
        command: "resize grow up 10 px or 10 ppt",
        repeatable: false,
        reason: "the test only checks that the compositor remains live; floating \
                 split containers are a documented compatibility gap, and sway \
                 answers `Cannot resize any further` when the resulting tiled resize \
                 changes neither size fraction (sway/sway/commands/resize.c:273-279)",
    },
    AllowedRejection {
        test: "189-floating-constraints.t",
        command: "resize grow up 10px or 10ppt",
        repeatable: false,
        reason: "the window is already at the configured floating maximum, so sway \
                 answers `Cannot resize any further` too; the next two assertions \
                 check the window did not move",
    },
    AllowedRejection {
        test: "132-move-workspace.t",
        command: "[con_mark=a] move to workspace *",
        repeatable: false,
        reason: "the test expects moving an empty workspace by criteria to be a no-op",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "move gibberish",
        repeatable: false,
        reason: "the regression intentionally sends this invalid command eleven times",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "bullshit-command-which-we-never-implement meh",
        repeatable: false,
        reason: "the test asserts that this invalid command returns an error",
    },
    AllowedRejection {
        test: "169-border-toggle.t",
        command: "border 1pixel",
        repeatable: false,
        reason: "i3-only alias; sway accepts the equivalent border pixel 1",
    },
    AllowedRejection {
        test: "141-resize.t",
        command: "resize grow right 10 px or 25 ppt",
        repeatable: false,
        reason: "the adapter's float is already at sway's automatic maximum",
    },
    AllowedRejection {
        test: "134-invalid-command.t",
        command: "blargh!",
        repeatable: false,
        reason: "the regression intentionally sends an invalid command",
    },
    AllowedRejection {
        test: "101-focus.t",
        command: "[con_mark=__does_not_exist] focus",
        repeatable: false,
        reason: "the assertion expects this unmatched criterion to fail",
    },
    AllowedRejection {
        test: "119-match.t",
        command: "[con_id=\"99999\"] kill",
        repeatable: false,
        reason: "the test verifies that an unmatched criterion leaves the window alive",
    },
    AllowedRejection {
        test: "260-invalid-criteria.t",
        command: "[con_id=foobar] kill",
        repeatable: false,
        reason: "the test intentionally sends a malformed con_id criterion",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_id=__focused__ app_id=doesnotmatch] kill",
        repeatable: false,
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_mark=marked app_id=doesnotmatch] kill",
        repeatable: false,
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[con_mark=doesnotexist] focus output right",
        repeatable: false,
        reason: "the assertion expects the unmatched criterion to leave output focus unchanged",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[id= . *] focus output right",
        repeatable: false,
        reason:
            "unchanged upstream file contains this malformed criterion and expects no focus change",
    },
    AllowedRejection {
        test: "294-focus-order.t",
        command: "[id=*] swap container with id *",
        repeatable: false,
        reason: "sway's id swap target is an X11 window id unavailable to native Wayland clients",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[app_id=b] swap with id *",
        repeatable: false,
        reason: "i3's optional swap words and X11 id target are unavailable in sway",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark V1",
        repeatable: false,
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark T1",
        repeatable: false,
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "126-regress-close.t",
        command: "mode toggle",
        repeatable: false,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "127-regress-floating-parent.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup cannot create or restore the floating container under test",
    },
    AllowedRejection {
        test: "142-regress-move-floating.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup leaves the window tiled instead of testing a floating move",
    },
    AllowedRejection {
        test: "144-regress-floating-resize.t",
        command: "mode toggle",
        repeatable: false,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "147-regress-floatingmove.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup leaves the parent tiled instead of testing floating-tree moves",
    },
    AllowedRejection {
        test: "151-regress-float-size.t",
        command: "mode toggle",
        repeatable: false,
        reason: "obsolete setup omits both floating-to-tiling transitions under test",
    },
    AllowedRejection {
        test: "152-regress-level-up.t",
        command: "mode toggle",
        repeatable: false,
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "192-layout.t",
        command: "layout toggle stacked",
        repeatable: false,
        reason: "documented i3/sway layout-toggle divergence",
    },
    AllowedRejection {
        test: "292-regress-layout-toggle.t",
        command: "layout toggle 1337 1337",
        repeatable: false,
        reason: "the regression intentionally sends invalid layout names",
    },
    AllowedRejection {
        test: "273-regress-focus-toggle.t",
        command: "focus mode_toggle",
        repeatable: false,
        reason: "the liveness regression runs this command on an empty workspace",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace to 2",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace to baz",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace 5 to 2",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "522-rename-assigned-workspace.t",
        command: "rename workspace 1 to baz",
        repeatable: false,
        reason: "sway rejects renaming to an existing workspace name",
    },
    AllowedRejection {
        test: "271-for_window_tilingfloating.t",
        command: "[tiling_from=\"auto\" con_mark=\"tiling\"] mark --add tiling_auto",
        repeatable: false,
        reason: "sway has no tiling provenance criterion",
    },
    AllowedRejection {
        test: "271-for_window_tilingfloating.t",
        command: "[floating_from=\"auto\" con_mark=\"floating\"] mark --add floating_auto",
        repeatable: false,
        reason: "sway has no floating provenance criterion",
    },
];

fn rejected_commands(stderr: &str) -> impl Iterator<Item = &str> {
    stderr.lines().filter_map(|line| {
        line.trim_start()
            .strip_prefix("# swayward rejected `")
            .and_then(|line| line.split_once("`: "))
            .map(|(command, _)| command)
    })
}

fn allowed_rejections(test: &str) -> Vec<&'static AllowedRejection> {
    ALLOWED_REJECTIONS
        .iter()
        .filter(|allowed| allowed.test == test)
        .collect()
}

fn expected_rejections(test: &str) -> Vec<&'static AllowedRejection> {
    allowed_rejections(test)
        .into_iter()
        .flat_map(|allowed| {
            let count = if test == "120-multiple-cmds.t" && allowed.command == "move gibberish" {
                11
            } else if matches!(
                test,
                "127-regress-floating-parent.t" | "151-regress-float-size.t"
            ) && allowed.command == "mode toggle"
            {
                2
            } else if test == "294-focus-order.t"
                && allowed.command == "[id=*] swap container with id *"
            {
                3
            } else {
                1
            };
            std::iter::repeat_n(allowed, count)
        })
        .collect()
}

fn rejections_match(test: &str, rejected: &[&str]) -> bool {
    let expected = expected_rejections(test);
    // Collapse runs of a repeatable command, whose count is not reproducible
    // because the file randomises its own input. Everything else is compared
    // exactly, in order.
    let repeatable = |command: &str| {
        expected
            .iter()
            .any(|allowed| allowed.repeatable && allowed.matches(test, command))
    };
    let mut collapsed: Vec<&str> = Vec::new();
    for command in rejected {
        if repeatable(command) && collapsed.last() == Some(command) {
            continue;
        }
        collapsed.push(command);
    }
    collapsed.len() == expected.len()
        && collapsed
            .iter()
            .zip(expected)
            .all(|(command, allowed)| allowed.matches(test, command))
}

fn socket_path(kind: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "swayward-i3-{kind}-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

/// A uniquely named scratch file. Callers are responsible for removing it; the
/// translated config outlives its creator because the reload watcher reads it.
fn scratch_path(kind: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "swayward-i3-{kind}-{}-{}.{extension}",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ))
}

fn requested_size(request: &Value) -> Option<(u16, u16)> {
    Some((
        request["requested_width"].as_u64()?.try_into().ok()?,
        request["requested_height"].as_u64()?.try_into().ok()?,
    ))
}

fn create_window(fixture: &mut Fixture, client: super::client::ClientId, request: &Value) -> u32 {
    let fullscreen_output = request["fullscreen_output"]
        .as_str()
        .map(|name| fixture.client(client).output(name));
    if request["initial_floating"].as_bool() == Some(true) {
        fixture
            .swayward()
            .config
            .borrow_mut()
            .window_rules
            .push(swayward_config::WindowRule {
                open_floating: Some(true),
                ..Default::default()
            });
    }
    let window = fixture.client(client).create_window();
    if let Some(app_id) = request["app_id"].as_str() {
        window.xdg_toplevel.set_app_id(app_id.to_owned());
    }
    if let Some(name) = request["name"].as_str() {
        window.set_title(name);
    }
    if let Some(output) = fullscreen_output.as_ref() {
        window.set_fullscreen(Some(output));
    }
    if let Some((width, height)) = requested_size(request) {
        window.set_size(width, height);
    }
    window.surface.id().protocol_id()
}

fn map_window(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    surface_id: u32,
    requested_size: Option<(u16, u16)>,
) -> i64 {
    let surface = fixture
        .client(client)
        .state
        .windows
        .iter()
        .find(|window| window.surface.id().protocol_id() == surface_id)
        .unwrap()
        .surface
        .clone();
    fixture.client(client).window(&surface).commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    if let Some((width, height)) = requested_size {
        window.set_size(width, height);
    }
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    if requested_size.is_some() {
        settle_configures(fixture, client);
    }
    fixture
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface_id)
                .then(|| crate::ipc::tree::window_id(mapped.id()))
        })
        .unwrap()
}

fn remove_window_for_surface(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    surface_id: u32,
) -> bool {
    let window = {
        fixture.swayward().layout.windows().find_map(|(_, mapped)| {
            (mapped.toplevel().wl_surface().id().protocol_id() == surface_id)
                .then(|| (mapped.id(), mapped.window.clone()))
        })
    };
    let Some((id, window)) = window else {
        return false;
    };
    fixture.swayward().unmark(Some(id), None);
    fixture
        .swayward()
        .layout
        .remove_window(&window, Transaction::new());
    let windows = &mut fixture.client(client).state.windows;
    if let Some(index) = windows
        .iter()
        .position(|window| window.surface.id().protocol_id() == surface_id)
    {
        windows.swap_remove(index);
    }
    true
}

fn settle_configures(fixture: &mut Fixture, client: super::client::ClientId) {
    fixture.double_roundtrip(client);
    let windows = &mut fixture.client(client).state.windows;
    for window in windows {
        let count = window.configures_received.len();
        if window.configures_looked_at == count {
            continue;
        }
        window.configures_looked_at = count;
        let Some((_, configure)) = window.configures_received.last() else {
            continue;
        };
        let size = configure.size;
        if size.0 > 0 && size.1 > 0 {
            window.set_size(size.0 as u16, size.1 as u16);
        }
        window.ack_last_and_commit();
    }
    fixture.double_roundtrip(client);
}

fn reap_closed_windows(fixture: &mut Fixture, client: super::client::ClientId) {
    fixture.double_roundtrip(client);
    let closed = fixture
        .client(client)
        .state
        .windows
        .iter()
        .filter(|window| window.close_requested)
        .map(|window| window.surface.id().protocol_id())
        .collect::<Vec<_>>();
    for surface_id in closed {
        remove_window_for_surface(fixture, client, surface_id);
    }
}

fn remove_all_windows(fixture: &mut Fixture, client: super::client::ClientId) {
    let surfaces = fixture
        .client(client)
        .state
        .windows
        .iter()
        .map(|window| window.surface.id().protocol_id())
        .collect::<Vec<_>>();
    for surface_id in surfaces {
        remove_window_for_surface(fixture, client, surface_id);
    }
}

fn activate_window(fixture: &mut Fixture, client: super::client::ClientId, id: i64) -> bool {
    let surface_id = fixture.swayward().layout.windows().find_map(|(_, mapped)| {
        (crate::ipc::tree::window_id(mapped.id()) == id)
            .then(|| mapped.toplevel().wl_surface().id().protocol_id())
    });
    let Some(surface_id) = surface_id else {
        return false;
    };
    let surface = fixture
        .client(client)
        .state
        .windows
        .iter()
        .find(|window| window.surface.id().protocol_id() == surface_id)
        .unwrap()
        .surface
        .clone();
    let token = fixture.client(client).request_activation_token(&surface);
    fixture.double_roundtrip(client);
    let token = token.lock().unwrap().take().unwrap();
    fixture.client(client).activate(token, &surface);
    fixture.double_roundtrip(client);
    true
}

fn window_states(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    id: i64,
) -> Option<Vec<&'static str>> {
    settle_configures(fixture, client);
    let surface_id = fixture
        .swayward()
        .layout
        .windows()
        .find_map(|(_, mapped)| {
            (crate::ipc::tree::window_id(mapped.id()) == id)
                .then(|| mapped.toplevel().wl_surface().id().protocol_id())
        })?;
    let window = fixture
        .client(client)
        .state
        .windows
        .iter()
        .find(|window| window.surface.id().protocol_id() == surface_id)?;
    let states = &window.configures_received.last()?.1.states;
    Some(
        states
            .iter()
            .filter_map(|state| match state {
                xdg_toplevel::State::Activated => Some("activated"),
                xdg_toplevel::State::Maximized => Some("maximized"),
                xdg_toplevel::State::TiledLeft => Some("tiled-left"),
                xdg_toplevel::State::TiledRight => Some("tiled-right"),
                xdg_toplevel::State::TiledTop => Some("tiled-top"),
                xdg_toplevel::State::TiledBottom => Some("tiled-bottom"),
                _ => None,
            })
            .collect(),
    )
}

fn close_window(fixture: &mut Fixture, client: super::client::ClientId, id: i64) -> bool {
    let surface_id = fixture.swayward().layout.windows().find_map(|(_, mapped)| {
        (crate::ipc::tree::window_id(mapped.id()) == id)
            .then(|| mapped.toplevel().wl_surface().id().protocol_id())
    });
    surface_id.is_some_and(|surface_id| remove_window_for_surface(fixture, client, surface_id))
}

type FakeOutput = ((i32, i32), (u16, u16));

fn fake_outputs(config: &str) -> Result<Option<Vec<FakeOutput>>, String> {
    let Some(spec) = config.lines().find_map(|line| {
        line.trim()
            .strip_prefix("fake-outputs ")
            .or_else(|| line.trim().strip_prefix("fake_outputs "))
    }) else {
        return Ok(None);
    };
    let outputs = spec
        .split(',')
        .map(|output| {
            let output = output.strip_suffix('P').unwrap_or(output);
            let (width, rest) = output
                .split_once('x')
                .ok_or_else(|| format!("invalid fake-outputs entry '{output}'"))?;
            let (height, rest) = rest
                .split_once('+')
                .ok_or_else(|| format!("invalid fake-outputs entry '{output}'"))?;
            let (x, y) = rest
                .split_once('+')
                .ok_or_else(|| format!("invalid fake-outputs entry '{output}'"))?;
            Ok((
                (
                    x.parse().map_err(|_| format!("invalid output x '{x}'"))?,
                    y.parse().map_err(|_| format!("invalid output y '{y}'"))?,
                ),
                (
                    width
                        .parse()
                        .map_err(|_| format!("invalid output width '{width}'"))?,
                    height
                        .parse()
                        .map_err(|_| format!("invalid output height '{height}'"))?,
                ),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    (!outputs.is_empty())
        .then_some(outputs)
        .ok_or_else(|| "fake-outputs lists no outputs".into())
        .map(Some)
}

fn only_ignorable_translation_warnings(test: &str, stderr: &str) -> bool {
    let mut lines = stderr.lines();
    let Some(count) = lines
        .next()
        .and_then(|line| line.strip_prefix("manual attention: "))
        .and_then(|line| line.strip_suffix(" directive(s)"))
        .and_then(|count| count.parse::<usize>().ok())
    else {
        return false;
    };
    let warnings = lines.collect::<Vec<_>>();
    warnings.len() == count
        && warnings.iter().all(|line| {
            line.contains(": bar blocks are unsupported; use waybar: bar ")
                || (test == "271-for_window_tilingfloating.t"
                    && (line.contains(": i3-only provenance criterion tiling_from ")
                        || line.contains(": i3-only provenance criterion floating_from ")))
        })
}

fn translate_config_file(
    test: &str,
    config: &str,
) -> Result<(PathBuf, swayward_config::Config), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = scratch_path("config", "kdl");
    let config = config
        .lines()
        .filter(|line| {
            !matches!(
                line.split_whitespace().next(),
                Some("fake-outputs" | "fake_outputs")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, config).map_err(|error| error.to_string())?;
    let output = Command::new(root.join("contrib/sway-to-kdl"))
        .arg(&path)
        .output()
        .map_err(|error| error.to_string());
    let _ = std::fs::remove_file(&path);
    let output = output?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().eq("manual attention: none")
        && !only_ignorable_translation_warnings(test, &stderr)
    {
        return Err(format!("i3 config translation was incomplete:\n{stderr}"));
    }
    let translated = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    let path = scratch_path("translated-config", "kdl");
    std::fs::write(&path, translated).map_err(|error| error.to_string())?;
    let config = match swayward_config::Config::load(&path).config {
        Ok(config) => config,
        Err(error) => {
            // A config swayward refuses to load still left a file behind.
            let _ = std::fs::remove_file(&path);
            return Err(format!("{error:?}"));
        }
    };
    Ok((path, config))
}

fn translate_config(config: &str) -> Result<swayward_config::Config, String> {
    let test = std::env::var("SWAYWARD_I3_TEST").unwrap_or_default();
    let (path, config) = translate_config_file(&test, config)?;
    let _ = std::fs::remove_file(path);
    Ok(config)
}

fn configure_client_state_oracle(config: &mut swayward_config::Config, test: &str) {
    match test {
        "257-keypress-group1-fallback.t" => {
            config.input.keyboard.xkb.layout = "us,ru".to_owned();
            config.input.keyboard.xkb.options = Some("grp:alt_shift_toggle".to_owned());
        }
        "295-net-wm-state-focused.t" => config.debug.deactivate_unfocused_windows = true,
        "551-net-wm-state-maximized.t" => config.prefer_no_csd = true,
        _ => (),
    }
}

fn prepare_test_config(source: &str) -> Result<swayward_config::Config, String> {
    let mut config = translate_config(source)?;
    if !source.lines().any(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("gaps inner ")
    }) {
        config.layout.gaps = 0.;
    }
    config.layout.border.off = false;
    if let Ok(test) = std::env::var("SWAYWARD_I3_TEST") {
        configure_client_state_oracle(&mut config, &test);
    }
    if !source.lines().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("focus_follows_mouse"))
    }) {
        config
            .input
            .focus_follows_mouse
            .get_or_insert(swayward_config::input::FocusFollowsMouse {
                max_scroll_amount: None,
            });
    }
    Ok(config)
}

pub(super) fn reload_test_config(fixture: &mut Fixture, source: &str) -> Result<(), String> {
    let config = prepare_test_config(source)?;
    fixture.swayward().for_window.clear();
    fixture.niri_state().reload_config(Ok(config));
    fixture.niri_state().ipc_config_loaded(false);
    Ok(())
}

fn reload_loaded_test_config(fixture: &mut Fixture, source: Option<&str>) -> Result<(), String> {
    let source = source.ok_or_else(|| "no test config has been loaded".to_owned())?;
    reload_test_config(fixture, source)
}

fn handle_control(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    loaded_config_source: &mut Option<String>,
    scratch: &mut Vec<PathBuf>,
    stream: UnixStream,
) {
    let mut request = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut request)
        .unwrap();
    let request: Value = serde_json::from_str(&request).unwrap();
    let reply = match request["action"].as_str().unwrap() {
        "config_default" => {
            let mut config = swayward_config::Config::default();
            config.gestures.hot_corners.off = true;
            fixture.niri_state().reload_config(Ok(config));
            json!({ "success": true })
        }
        "config" => {
            let source = request["config"].as_str().unwrap();
            let test = std::env::var("SWAYWARD_I3_TEST").unwrap_or_default();
            match (fake_outputs(source), translate_config_file(&test, source)) {
                (Ok(outputs), Ok((path, mut config))) => {
                    scratch.push(path.clone());
                    if let Some(server) = &fixture.swayward().ipc_server {
                        server.set_loaded_config_file_name(path.to_string_lossy().into_owned());
                    }
                    if !source.lines().any(|line| {
                        line.trim_start()
                            .to_ascii_lowercase()
                            .starts_with("gaps inner ")
                    }) {
                        config.layout.gaps = 0.;
                    }
                    config.layout.border.off = false;
                    if let Ok(test) = std::env::var("SWAYWARD_I3_TEST") {
                        configure_client_state_oracle(&mut config, &test);
                    }
                    if !source.lines().any(|line| {
                        line.split_whitespace()
                            .next()
                            .is_some_and(|word| word.eq_ignore_ascii_case("focus_follows_mouse"))
                    }) {
                        config.input.focus_follows_mouse.get_or_insert(
                            swayward_config::input::FocusFollowsMouse {
                                max_scroll_amount: None,
                            },
                        );
                    }
                    fixture
                        .swayward()
                        .layout
                        .initialize_workspaces_from_bindings(&config);
                    fixture.swayward().for_window.clear();
                    fixture.niri_state().reload_config(Ok(config));
                    crate::utils::watcher::setup(
                        fixture.niri_state(),
                        &swayward_config::ConfigPath::Explicit(path),
                        Vec::new(),
                    );
                    if let Some(outputs) = outputs {
                        fixture.replace_outputs(outputs);
                        fixture.double_roundtrip(client);
                    }
                    *loaded_config_source = Some(source.to_owned());
                    json!({ "success": true })
                }
                (Err(error), _) | (_, Err(error)) => json!({ "success": false, "error": error }),
            }
        }
        "reload" => match reload_loaded_test_config(fixture, loaded_config_source.as_deref()) {
            Ok(()) => json!({ "success": true }),
            Err(error) => json!({ "success": false, "error": error }),
        },
        "create" => json!({ "handle": create_window(fixture, client, &request) }),
        "open" => {
            let handle = create_window(fixture, client, &request);
            json!({ "id": map_window(fixture, client, handle, requested_size(&request)) })
        }
        "map" => json!({
            "id": map_window(
                fixture,
                client,
                request["handle"].as_u64().unwrap() as u32,
                requested_size(&request),
            )
        }),
        "fullscreen" => {
            let surface_id = request["handle"].as_u64().unwrap() as u32;
            let surface = fixture
                .client(client)
                .state
                .windows
                .iter()
                .find(|window| window.surface.id().protocol_id() == surface_id)
                .unwrap()
                .surface
                .clone();
            let window = fixture.client(client).window(&surface);
            if request["enabled"].as_bool() == Some(true) {
                window.set_fullscreen(None);
            } else {
                window.unset_fullscreen();
            }
            fixture.double_roundtrip(client);
            json!({ "success": true })
        }
        "set_title" => {
            let surface_id = request["handle"].as_u64().unwrap() as u32;
            let title = request["title"].as_str().unwrap();
            let surface = fixture
                .client(client)
                .state
                .windows
                .iter()
                .find(|window| window.surface.id().protocol_id() == surface_id)
                .unwrap()
                .surface
                .clone();
            fixture.client(client).window(&surface).set_title(title);
            fixture.double_roundtrip(client);
            json!({ "success": true })
        }
        "set_parent" => {
            let surface_id = request["handle"].as_u64().unwrap() as u32;
            let parent_id = request["parent_handle"].as_u64().unwrap() as u32;
            let windows = &fixture.client(client).state.windows;
            let surface = windows
                .iter()
                .find(|window| window.surface.id().protocol_id() == surface_id)
                .unwrap()
                .surface
                .clone();
            let parent_surface = windows
                .iter()
                .find(|window| window.surface.id().protocol_id() == parent_id)
                .unwrap()
                .surface
                .clone();
            let client_id = client;
            let wayland_client = fixture.client(client_id);
            let parent = wayland_client.window(&parent_surface).xdg_toplevel.clone();
            wayland_client.window(&surface).set_parent(Some(&parent));
            fixture.double_roundtrip(client_id);
            json!({ "success": true })
        }
        "close" => {
            json!({ "success": close_window(fixture, client, request["id"].as_i64().unwrap()) })
        }
        "focused" => json!({
            "id": fixture
                .swayward()
                .layout
                .focus()
                .map(|mapped| crate::ipc::tree::window_id(mapped.id()))
        }),
        "activate" => json!({
            "success": activate_window(fixture, client, request["id"].as_i64().unwrap())
        }),
        "window_states" => {
            let states = window_states(fixture, client, request["id"].as_i64().unwrap());
            json!({
                "states": states,
                "xdg_wm_base_version": fixture.client(client).state.xdg_wm_base_version,
            })
        }
        "pointer_button" => match (request["button"].as_u64(), request["pressed"].as_bool()) {
            (Some(button), Some(pressed)) => {
                super::ipc::pointer_button(fixture, u32::try_from(button).unwrap(), pressed);
                json!({ "success": true })
            }
            _ => json!({ "success": false, "error": "button and pressed are required" }),
        },
        "pointer_axis" => match (
            request["horizontal_v120"].as_f64(),
            request["vertical_v120"].as_f64(),
        ) {
            (Some(horizontal), Some(vertical)) => {
                super::ipc::pointer_axis(fixture, horizontal, vertical);
                json!({ "success": true })
            }
            _ => {
                json!({ "success": false, "error": "horizontal_v120 and vertical_v120 are required" })
            }
        },
        "key_event" => match (request["key"].as_u64(), request["pressed"].as_bool()) {
            (Some(key), Some(pressed)) => {
                super::ipc::key_event(fixture, u32::try_from(key).unwrap(), pressed);
                json!({ "success": true })
            }
            _ => json!({ "success": false, "error": "key and pressed are required" }),
        },
        "set_xkb_group" => match request["group"].as_u64() {
            Some(group) => {
                let keyboard = fixture.swayward().seat.get_keyboard().unwrap();
                keyboard.with_xkb_state(fixture.niri_state(), |mut context| {
                    context.set_layout(smithay::input::keyboard::Layout(
                        u32::try_from(group).unwrap(),
                    ));
                });
                json!({ "success": true })
            }
            None => json!({ "success": false, "error": "group is required" }),
        },
        "type_key_chords" => {
            let chords = request["chords"]
                .as_array()
                .unwrap()
                .iter()
                .map(|chord| {
                    chord
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|key| u32::try_from(key.as_u64().unwrap()).unwrap())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let chords = chords.iter().map(Vec::as_slice).collect::<Vec<_>>();
            super::ipc::type_key_chords(fixture, &chords);
            json!({ "success": true })
        }
        "warp_pointer" => match (request["x"].as_f64(), request["y"].as_f64()) {
            (Some(x), Some(y)) => {
                settle_configures(fixture, client);
                fixture.swayward().clock.set_complete_instantly(true);
                fixture.swayward().layout.advance_animations();
                fixture.swayward().clock.set_complete_instantly(false);
                // Feed a real absolute motion event so a pointer grab sees it.
                // move_cursor alone teleports the cursor and updates pointer
                // contents, but never reaches Layout::interactive_move_update,
                // so a dragged window did not follow the pointer at all.
                super::ipc::pointer_motion_absolute(fixture, x, y);
                let location = (x, y).into();
                let under = fixture.swayward().contents_under(location);
                fixture.swayward().handle_focus_follows_mouse(&under);
                fixture.niri_state().move_cursor(location);
                json!({ "success": true })
            }
            _ => json!({ "success": false, "error": "pointer coordinates must be numeric" }),
        },
        "prepare_resize" => {
            settle_configures(fixture, client);
            json!({ "success": true })
        }
        "reap_closed" => {
            let settle = request["settle_configures"].as_bool() == Some(true);
            if settle {
                settle_configures(fixture, client);
            }
            reap_closed_windows(fixture, client);
            json!({ "success": true })
        }
        "remove_all_windows" => {
            remove_all_windows(fixture, client);
            json!({ "success": true })
        }
        "request_stop" => {
            fixture.niri_state().request_stop("exit");
            json!({ "success": true })
        }
        action => panic!("unknown i3 test control action: {action}"),
    };
    writeln!(&stream, "{reply}").unwrap();
}

fn tap_failure_summary(stdout: &str, stderr: &str) -> String {
    stdout
        .lines()
        .filter(|line| line.starts_with("not ok "))
        .chain(
            stderr
                .lines()
                .filter(|line| line.starts_with("#   Failed test") || line.starts_with("#   at ")),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

fn tap_skips(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter(|line| line.starts_with("ok ") && line.contains("# skip"))
        .collect()
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic payload")
}

fn with_test_context(test: &str, run: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        panic!(
            "i3 test {test} panicked: {}",
            panic_message(payload.as_ref())
        );
    }
}

fn run_i3_test_with_context(test: &str) {
    with_test_context(test, || run_i3_test(test));
}

fn run_i3_test(test: &str) {
    let mut config = swayward_config::Config::default();
    config.layout.gaps = 0.;
    config.layout.border.off = false;
    configure_client_state_oracle(&mut config, test);
    config.input.focus_follows_mouse = Some(swayward_config::input::FocusFollowsMouse {
        max_scroll_amount: None,
    });
    config.animations.window_movement.0.off = true;
    config.animations.window_resize.anim.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();

    let handle = fixture.swayward().event_loop.clone();
    let ipc_dir = socket_path("ipc");
    std::fs::create_dir(&ipc_dir).unwrap();
    let ipc_socket = ipc_dir.join("ipc.sock");
    let ipc_server =
        crate::ipc::server::IpcServer::start_at(&handle, Some(ipc_socket.clone())).unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.niri_state().ipc_refresh_layout();

    let control_path = socket_path("control");
    let control = UnixListener::bind(&control_path).unwrap();
    control.set_nonblocking(true).unwrap();
    // Remove the socket even when a test panics or times out. Without this the
    // whole suite leaks one file per conformance test per run, and a run left
    // over 7000 of them in the temp directory. Unix socket paths are limited to
    // about 108 bytes, so an accumulating temp directory eventually makes bind
    // fail in whichever file happens to run next.
    struct Scratch {
        files: Vec<PathBuf>,
        dirs: Vec<PathBuf>,
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            for path in &self.files {
                let _ = std::fs::remove_file(path);
            }
            for path in &self.dirs {
                let _ = std::fs::remove_dir(path);
            }
        }
    }
    let mut scratch = Scratch {
        files: vec![control_path.clone()],
        dirs: vec![ipc_dir],
    };

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut child = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg(root.join("tests/i3/t").join(test))
        .env("I3SOCK", &ipc_socket)
        .env("SWAYWARD_TEST_CONTROL", &control_path)
        .env("SWAYWARD_I3_TEST", test)
        .env(
            "PATH",
            format!(
                "{}:{}",
                root.join("tests/i3/bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let started = Instant::now();
    // Generous enough that a cold build cache and a loaded machine cannot trip
    // it: 132-move-workspace.t runs 160 assertions in ~14s warm but has been
    // measured at 33s cold, and the suite runs these files in parallel. A
    // genuinely hung test still fails, just later.
    let deadline = started + Duration::from_secs(180);
    let mut loaded_config_source = None;
    loop {
        fixture.dispatch();
        match control.accept() {
            Ok((stream, _)) => handle_control(
                &mut fixture,
                client,
                &mut loaded_config_source,
                &mut scratch.files,
                stream,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("test control accept failed: {error}"),
        }
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            eprint!("{stdout}");
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                eprint!("{stderr}");
            }
            let rejected = rejected_commands(&stderr).collect::<Vec<_>>();
            let expected = expected_rejections(test)
                .iter()
                .map(|item| item.command)
                .collect::<Vec<_>>();
            let adapter_failed = stderr.contains("swayward xdotool adapter");
            let skips = tap_skips(&stdout);
            assert!(
                status.success()
                    && rejections_match(test, &rejected)
                    && !adapter_failed
                    && (!passing_tests().any(|green| green == test) || skips.is_empty()),
                "i3 test {test} failed, its xdotool adapter failed, its rejected commands changed, or a green file skipped assertions\nTAP failures:\n{}\nTAP skips: {skips:?}\nexpected rejections: {expected:?}\nactual rejections: {rejected:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                tap_failure_summary(&stdout, &stderr),
            );
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!(
                "i3 test {test} timed out after {:?}\nTAP failures:\n{}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                started.elapsed(),
                tap_failure_summary(&stdout, &stderr),
            );
        }
        thread::yield_now();
    }
}

/// The i3 conformance classification: one entry per vendored file with its
/// assertion counts, reason code, citation and note.
///
/// This is the source of truth. The set of fully green files is *derived* from
/// it rather than listed separately, because a hand-maintained duplicate of a
/// derivable fact drifts: the old `passing.txt` and the coverage prose
/// disagreed about the green count more than once, and each had its own
/// invariant asserting it against the other.
const COVERAGE: &str = include_str!("../../tests/i3/coverage.toml");
const COVERAGE_README: &str = include_str!("../../tests/i3/README.md");
const PROJECT_README: &str = include_str!("../../README.md");
const HARNESS: &str = include_str!("../../tests/i3/lib/i3test.pm");

#[test]
fn harness_preserves_unmapped_rectangles_and_forwards_fullscreen_requests() {
    assert!(HARNESS.contains("requested_rect => $args{rect}"));
    assert!(HARNESS.contains("return $_[0]->{requested_rect} unless defined($_[0]->{id});"));
    assert!(HARNESS.contains("action => 'fullscreen'"));
    assert!(HARNESS.contains("action => 'set_parent'"));
    assert!(HARNESS.contains("sub subscribe"));
    assert!(HARNESS.contains("action => 'request_stop'"));
}

#[test]
fn harness_does_not_convert_wrong_named_assertions_into_skips() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg("-e")
        .arg(
            "use i3test; is('splith', 'tabbed', \
             'workspace layout is \"tabbed\"'); done_testing;",
        )
        .env("SWAYWARD_I3_TEST", "509-workspace_layout.t")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !output.status.success(),
        "a wrong assertion must fail: {stdout}"
    );
    assert!(
        stdout.contains("not ok 1 - workspace layout is \"tabbed\""),
        "the real comparison must reach TAP: {stdout}"
    );
    assert!(
        !stdout.contains("# skip"),
        "the harness must not intercept it: {stdout}"
    );
}

#[test]
fn harness_skips_only_i3_invalid_criteria_wording() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("perl")
        .arg(format!("-I{}", root.join("tests/i3/lib").display()))
        .arg("-e")
        .arg(
            "use i3test; ok(1, 'command was unsuccessful'); \
             is('sway text', 'i3 text', 'correct error is returned'); done_testing;",
        )
        .env("SWAYWARD_I3_TEST", "260-invalid-criteria.t")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("ok 1 - command was unsuccessful"),
        "{stdout}"
    );
    assert!(
        stdout.contains("ok 2 # skip i3 error wording differs"),
        "{stdout}"
    );
}

#[test]
fn failure_diagnostics_name_assertions_and_non_tap_panics() {
    let payload = std::panic::catch_unwind(|| {
        with_test_context("setup-failure.t", || panic!("setup failed"));
    })
    .unwrap_err();
    assert_eq!(
        panic_message(payload.as_ref()),
        "i3 test setup-failure.t panicked: setup failed"
    );

    let stdout = "ok 159 - setup\nnot ok 160 - No empty workspace created\n1..160\n";
    let stderr = "#   Failed test 'No empty workspace created'\n#   at test.t line 398.\n";
    assert_eq!(
        tap_failure_summary(stdout, stderr),
        "not ok 160 - No empty workspace created\n#   Failed test 'No empty workspace created'\n#   at test.t line 398."
    );
    assert_eq!(
        tap_skips("ok 1 - portable\nok 2 # skip i3-only\n"),
        ["ok 2 # skip i3-only"]
    );
}

#[test]
fn rejection_allowlist_is_keyed_by_file_and_exact_command() {
    let stderr = "# swayward rejected `[con_mark=__does_not_exist] focus`: error\n";
    assert_eq!(
        rejected_commands(stderr).collect::<Vec<_>>(),
        allowed_rejections("101-focus.t")
            .iter()
            .map(|item| item.command)
            .collect::<Vec<_>>()
    );
    assert!(!rejections_match(
        "119-match.t",
        &rejected_commands(stderr).collect::<Vec<_>>()
    ));
    assert!(rejections_match(
        "111-goto.t",
        &["[con_mark=\"mark.A1b2\"] focus"]
    ));
    assert!(rejections_match(
        "294-focus-order.t",
        &[
            "[id=1] swap container with id 2",
            "[id=3] swap container with id 4",
            "[id=5] swap container with id 6",
        ]
    ));
    assert!(!rejections_match(
        "294-focus-order.t",
        &["[id=1] swap container with con_id 2"]
    ));
    assert!(ALLOWED_REJECTIONS
        .iter()
        .all(|rejection| !rejection.reason.is_empty()));
}

#[test]
fn fake_outputs_create_real_outputs_with_requested_geometry() {
    let outputs = fake_outputs("font monospace\nfake-outputs 1024x768+0+0P,800x600+1024+20\n")
        .unwrap()
        .unwrap();
    assert_eq!(outputs, [((0, 0), (1024, 768)), ((1024, 20), (800, 600))]);

    let mut fixture = Fixture::new();
    fixture.add_output(1, (1280, 800));
    fixture.replace_outputs(outputs);
    let swayward = fixture.swayward();
    let actual = crate::ipc::tree::describe_outputs(&swayward.layout, &swayward.global_space);
    assert_eq!(
        actual
            .iter()
            .map(|output| (output.name.as_str(), output.rect))
            .collect::<Vec<_>>(),
        [
            (
                "fake-0",
                swayward_ipc::Rect {
                    x: 0,
                    y: 0,
                    width: 1024,
                    height: 768
                }
            ),
            (
                "fake-1",
                swayward_ipc::Rect {
                    x: 1024,
                    y: 20,
                    width: 800,
                    height: 600
                }
            ),
        ]
    );
}

#[test]
fn test_config_reload_requires_loaded_source() {
    let mut fixture = Fixture::new();
    assert_eq!(
        reload_loaded_test_config(&mut fixture, None).unwrap_err(),
        "no test config has been loaded"
    );
    reload_loaded_test_config(&mut fixture, Some("font monospace")).unwrap();
}

#[test]
fn i3_config_translation_ignores_only_unsupported_bar_blocks() {
    translate_config("font monospace\nbar {\n    output primary\n}\n").unwrap();
    assert!(!only_ignorable_translation_warnings(
        "316-drag-container.t",
        "manual attention: 1 directive(s)\n  config:2: another warning\n"
    ));
    assert!(!only_ignorable_translation_warnings(
        "316-drag-container.t",
        "manual attention: 2 directive(s)\n  config:2: bar blocks are unsupported; use waybar: bar { | }\n"
    ));

    let error = translate_config("bar { output primary }\nmystery value\n").unwrap_err();
    assert!(error.contains("manual attention: 2 directive(s)"));
    assert!(error.contains("unhandled: mystery value"));
}

#[test]
fn i3_config_translation_ignores_provenance_warnings_only_for_271() {
    let warnings = "manual attention: 2 directive(s)\n  config:2: i3-only provenance criterion tiling_from has no sway equivalent: for_window [tiling_from=\"auto\"]\n  config:3: i3-only provenance criterion floating_from has no sway equivalent: for_window [floating_from=\"user\"]\n";
    assert!(only_ignorable_translation_warnings(
        "271-for_window_tilingfloating.t",
        warnings
    ));
    assert!(!only_ignorable_translation_warnings(
        "272-regress-focus-assign.t",
        warnings
    ));
}

#[test]
fn i3_config_translation_rejects_unhandled_directives() {
    let error = translate_config("font monospace\nmystery value\n").unwrap_err();
    assert!(error.contains("manual attention: 1 directive(s)"));
    assert!(error.contains("unhandled: mystery value"));
}

#[test]
fn i3_config_translation_never_applies_a_partial_config() {
    let incomplete = translate_config("bindsym X\n").unwrap_err();
    assert!(incomplete.contains("manual attention: 1 directive(s)"));
    assert!(incomplete.contains("malformed bindsym: X"));
}

#[test]
fn explicit_default_binding_mode_loads() {
    let config = translate_config("mode \"default\" {\n    bindsym X nop\n}\n").unwrap();
    let mode = config
        .binding_modes
        .iter()
        .find(|mode| mode.name == "default")
        .unwrap();
    assert_eq!(mode.binds.0.len(), 1);
}

#[test]
fn workspace_layout_config_wraps_new_windows() {
    let config = translate_config("workspace_layout tabbed\n").unwrap();
    assert_eq!(
        config.layout.workspace_layout,
        swayward_config::WorkspaceLayout::Tabbed
    );
}

/// One entry from `coverage.toml`: the file name and the fields this runner
/// needs. Parsed with a small reader rather than a TOML crate, because the
/// document is flat and adding a runtime dependency for four integers is worse
/// than twenty lines of parsing.
struct Coverage {
    file: &'static str,
    assertions: usize,
    passing: usize,
    plan_unknown: bool,
}

fn coverage_entries() -> Vec<Coverage> {
    let mut entries = Vec::new();
    let mut current: Option<Coverage> = None;
    for line in COVERAGE.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[files.\"") {
            if let Some(done) = current.take() {
                entries.push(done);
            }
            let file = rest
                .split_once("\"]")
                .expect("a files entry names its file")
                .0;
            current = Some(Coverage {
                file,
                assertions: 0,
                passing: 0,
                plan_unknown: false,
            });
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        // Values are bare integers or `true`; notes may contain anything, so
        // only read the keys this runner uses.
        let read = |value: &str| -> usize {
            value
                .split(|c: char| !c.is_ascii_digit())
                .find(|part| !part.is_empty())
                .unwrap_or("0")
                .parse()
                .unwrap_or(0)
        };
        if let Some(value) = line.strip_prefix("assertions = ") {
            entry.assertions = read(value);
        } else if let Some(value) = line.strip_prefix("pass = ") {
            entry.passing = read(value);
        } else if line.starts_with("plan_unknown = true") {
            entry.plan_unknown = true;
        }
    }
    if let Some(done) = current {
        entries.push(done);
    }
    entries
}

/// The files that pass every declared assertion.
///
/// A file with no TAP plan cannot be green: `plan_unknown` means it aborted
/// before declaring how many assertions it has, so "all of them pass" is not a
/// statement anyone can make. Treating the reached count as the plan silently
/// credited fourteen files with a plan they do not have. A zero-assertion file
/// is likewise not green; it offers no evidence either way.
fn passing_tests() -> impl Iterator<Item = &'static str> {
    coverage_entries()
        .into_iter()
        .filter(|entry| {
            entry.assertions > 0 && !entry.plan_unknown && entry.passing == entry.assertions
        })
        .map(|entry| entry.file)
        .collect::<Vec<_>>()
        .into_iter()
}

/// The manifest is read by the runner and reviewed by hand, and concurrent
/// merges have appended entries out of order three times. Sorted and duplicate
/// free keeps a review diff honest and stops one file being listed twice.
#[test]
fn passing_manifest_is_sorted_and_unique() {
    let files = passing_tests().collect::<Vec<_>>();
    let mut sorted = files.clone();
    sorted.sort_unstable();
    assert_eq!(files, sorted, "the derived green set is not sorted");
    let mut seen = std::collections::HashSet::new();
    for file in &files {
        assert!(
            seen.insert(file),
            "the derived green set lists {file} twice"
        );
    }
}

/// A green file's coverage row must not read as a failing one. `297-scroll-tabbed.t`
/// passed 14/14 while its row led with `diagnostic: 4 pass; 10 fail`, which
/// described a superseded reduced run rather than the file. The number is only
/// safe to quote when the leading result belongs to the manifest entry.
#[test]
fn green_coverage_rows_do_not_lead_with_a_failing_result() {
    let green = passing_tests().collect::<std::collections::HashSet<_>>();
    for line in COVERAGE_README.lines() {
        let Some(rest) = line.strip_prefix("| `") else {
            continue;
        };
        let Some((file, rest)) = rest.split_once("` |") else {
            continue;
        };
        if !green.contains(file) {
            continue;
        }
        // The result cell is a terse measurement, not prose: it is the first
        // cell whose words are only counts and verdicts. Matching any cell
        // containing "fail" instead picks up citation prose such as "returns
        // sway's `No matching node.` failure".
        let Some(result) = rest.split('|').find(|cell| {
            let head = cell.split('(').next().unwrap_or(cell).trim();
            // A bare assertion count such as "14" occupies an earlier cell and
            // would always look clean, so require a verdict word too.
            head.split_whitespace()
                .any(|word| matches!(word.trim_end_matches([';', ',']), "pass" | "fail" | "skip"))
                && head.split_whitespace().all(|word| {
                    let word = word.trim_end_matches([';', ',']);
                    word.chars().all(|c| c.is_ascii_digit())
                        || matches!(
                            word,
                            "pass" | "fail" | "skip" | "finished:" | "reached" | "diagnostic:"
                        )
                })
        }) else {
            continue;
        };
        let (verdict, aside) = result.split_once('(').unwrap_or((result, ""));
        assert!(
            !verdict.contains("fail") && !verdict.contains("skip"),
            "{file} derives as green but its coverage row reports \
             {verdict:?} outside any parenthetical aside ({aside:?}); a green \
             file's leading result must be its own"
        );
    }
}

/// The harness adjusts its behaviour per test file, and an audit found those
/// branches masking TAP skips in fifteen manifest entries. The count is quoted
/// in the coverage report, so keep it honest: a new branch is a deliberate act
/// that should be classified, not an accident.
#[test]
fn per_file_harness_branch_count_matches_the_audit() {
    let actual = HARNESS.matches("SWAYWARD_I3_TEST").count();
    let claimed: usize = COVERAGE_README
        .lines()
        .find_map(|line| line.trim().strip_prefix("An audit at commit `"))
        .and_then(|rest| rest.split_once("found "))
        .map(|(_, rest)| rest)
        .expect("the coverage report states an audited branch count")
        .split_once(" textual references")
        .expect("the count precedes the phrase")
        .0
        .parse()
        .expect("audited count is a number");
    assert_eq!(
        actual, claimed,
        "tests/i3/lib/i3test.pm has {actual} per-file branches but the audit \
         records {claimed}; classify the change in the audit table"
    );
}

/// The top-level README quotes the green ceiling too, and went stale once when
/// the manifest changed. Keep its figures tied to the manifest.
#[test]
fn project_readme_green_ceiling_matches_the_manifest() {
    let green = passing_tests().count();
    let claim = PROJECT_README
        .lines()
        .find(|line| line.contains("green ceiling of"))
        .expect("the README states a green ceiling");
    let ceiling: usize = claim
        .split_once("green ceiling of ")
        .expect("ceiling is introduced")
        .1
        .split_once(" files")
        .expect("ceiling states a file count")
        .0
        .parse()
        .expect("ceiling is a number");
    let stated_green: usize = claim
        .rsplit_once(": ")
        .expect("the claim cites the green count")
        .1
        .split_once(' ')
        .expect("green count is followed by a word")
        .0
        .parse()
        .expect("green count is a number");
    assert_eq!(
        stated_green, green,
        "README.md claims {stated_green} green files but coverage.toml derives {green}"
    );
    // The sentence wraps, so match against the whole file with newlines
    // collapsed rather than line by line.
    let flattened = PROJECT_README
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let gap_only = flattened
        .split_once(" vendored files blocked only by implementation or adapter gaps.")
        .expect("the README states a gap-only count")
        .0
        .rsplit(' ')
        .next()
        .expect("gap-only count precedes the phrase")
        .parse::<usize>()
        .expect("gap-only count is a number");
    assert_eq!(
        green + gap_only,
        ceiling,
        "{green} green plus {gap_only} gap-only must equal the README ceiling {ceiling}"
    );

    // Both files state the ceiling, and they drifted apart once. Each is
    // internally consistent on its own, so compare them to each other too.
    let coverage_ceiling: usize = COVERAGE_README
        .lines()
        .find_map(|line| line.trim().strip_prefix("The **current green ceiling is "))
        .expect("the coverage report states a ceiling")
        .split_once(" files**")
        .expect("ceiling states a file count")
        .0
        .parse()
        .expect("ceiling is a number");
    assert_eq!(
        ceiling, coverage_ceiling,
        "README.md says {ceiling} but tests/i3/README.md says {coverage_ceiling}"
    );
}

/// The ceiling paragraph is maintained by hand and a merge once left two
/// contradictory copies, each with a different green count. Assert there is
/// exactly one and that its arithmetic matches the manifest and the explicit
/// gap-only table. Ordinary coverage-row edits do not affect this check.
#[test]
fn documented_green_ceiling_matches_the_manifest() {
    let claims = COVERAGE_README
        .lines()
        .filter_map(|line| line.trim().strip_prefix("The **current green ceiling is "))
        .collect::<Vec<_>>();
    assert_eq!(
        claims.len(),
        1,
        "tests/i3/README.md must state the green ceiling exactly once"
    );

    let ceiling: usize = claims[0]
        .split_once(" files**")
        .expect("ceiling claim states a file count")
        .0
        .parse()
        .expect("ceiling is a number");
    let stated_green: usize = claims[0]
        .rsplit_once("the ")
        .expect("ceiling claim cites the manifest size")
        .1
        .split_once(' ')
        .expect("manifest size is followed by a word")
        .0
        .parse()
        .expect("manifest size is a number");

    let green_files = passing_tests().collect::<std::collections::HashSet<_>>();
    let green = green_files.len();
    assert_eq!(
        stated_green, green,
        "the ceiling paragraph cites {stated_green} green files but coverage.toml derives {green}"
    );
    let gap_only = COVERAGE_README
        .lines()
        .find_map(|line| {
            line.trim().strip_suffix(
                " vendored files whose only obstacles are implementation or adapter gaps.",
            )
        })
        .expect("ceiling paragraph states a gap-only count")
        .parse::<usize>()
        .expect("gap-only count is a number");
    let gap_only_files = COVERAGE_README
        .lines()
        .skip_while(|line| *line != "| Gap-only file | Remaining gap |")
        .skip(2)
        .take_while(|line| line.starts_with("| `"))
        .map(|line| {
            line.strip_prefix("| `")
                .and_then(|line| line.split_once("` |"))
                .expect("gap-only table row contains a backtick-quoted filename")
                .0
        })
        .collect::<Vec<_>>();
    assert_eq!(
        gap_only_files.len(),
        gap_only,
        "the gap-only table lists {} files but the ceiling paragraph claims {gap_only}",
        gap_only_files.len()
    );
    // The prose above the table restates the count. It went stale at 13 while
    // the table held 12, because every other invariant checked the ceiling
    // paragraph instead of this sentence.
    let introduced = COVERAGE_README
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("These ")
                .and_then(|line| line.strip_suffix(" files define the gap-only set:"))
        })
        .expect("the gap-only table is introduced by a sentence stating its size")
        .parse::<usize>()
        .expect("the gap-only introduction states a file count");
    assert_eq!(
        introduced, gap_only,
        "the gap-only table is introduced as {introduced} files but the ceiling \
         paragraph claims {gap_only}"
    );
    let mut unique = std::collections::HashSet::new();
    for file in &gap_only_files {
        assert!(unique.insert(file), "the gap-only table lists {file} twice");
        assert!(
            !green_files.contains(file),
            "the gap-only table also lists green file {file}"
        );
        assert!(
            std::path::Path::new("tests/i3/t").join(file).is_file(),
            "the gap-only table lists non-vendored file {file}"
        );
        // The table's own rule is that a mixed file does not belong here,
        // because closing the swayward gap would not make it green. Two files
        // stayed listed after their coverage rows were reclassified as mixed,
        // which overstated the ceiling by two. Every other invariant checked
        // the arithmetic, and the arithmetic was self-consistently wrong.
        let row = COVERAGE_README
            .lines()
            .find(|line| line.starts_with(&format!("| `{file}` |")))
            .unwrap_or_else(|| panic!("gap-only file {file} has no coverage row"));
        let status = row
            .split('|')
            .nth(3)
            .expect("a coverage row has a status column");
        assert!(
            !status.contains("mixed"),
            "{file} is in the gap-only table but its coverage row classifies it \
             as mixed: {}",
            status.trim()
        );
    }
    assert_eq!(
        green + gap_only_files.len(),
        ceiling,
        "{green} green plus {} gap-only must equal the stated ceiling {ceiling}",
        gap_only_files.len()
    );
}

#[test]
fn i3_conformance_runner() {
    // `SWAYWARD_I3_TEST` selects a single file, including one with known
    // failures, so conformance findings stay executable without turning the
    // default gate red.
    if let Ok(selected) = std::env::var("SWAYWARD_I3_TEST") {
        run_i3_test_with_context(&selected);
        return;
    }

    let mut count = 0;
    for test in passing_tests() {
        run_i3_test_with_context(test);
        count += 1;
    }
    assert!(
        count > 0,
        "no fully green files derive from tests/i3/coverage.toml"
    );
}

/// `unproven` is the label most easily abused, because an assertion reclassified
/// from `fail` to `unproven` looks identical in raw TAP output: the run says
/// `not ok` either way. The distinction is whether the assertion's premise held,
/// so a row that moves failures into `unproven` must name the premise that was
/// never established. Without this check, the label is an unfalsifiable way to
/// reduce the fail count.
#[test]
fn unproven_reclassifications_name_their_missing_premise() {
    // Four `##` sections contain row-looking lines and only two hold coverage
    // rows. The audit table's rows are three columns wide, so scanning the
    // whole file matches a different shape and reports a phantom violation.
    let mut checked = 0;
    let mut section = "";
    for line in COVERAGE_README.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            section = heading.trim();
        }
        if !matches!(section, "Coverage conventions" | "Coverage table") {
            continue;
        }
        if !line.starts_with("| `") {
            continue;
        }
        let fields = line[1..].splitn(4, '|').collect::<Vec<_>>();
        if fields.len() < 4 {
            continue;
        }
        let (file, result, notes) = (fields[0].trim(), fields[2].trim(), fields[3].trim());
        // Only rows that both report failures and call them unproven.
        if !result.contains("unproven") || !result.contains("fail") {
            continue;
        }
        checked += 1;
        // The premise is always an input the harness cannot supply, or an
        // earlier assertion that did not establish the expected state. Require
        // the notes to say which, rather than asserting the conclusion.
        let names_premise = [
            "cannot establish",
            "depend on",
            "depends on",
            "require",
            "requires",
            "never establish",
            "unavailable",
            "no equivalent",
            "has no request",
            "cannot set",
            "cannot be set",
            "cannot express",
        ]
        .iter()
        .any(|phrase| notes.contains(phrase));
        assert!(
            names_premise,
            "{file} reclassifies failures as unproven but its notes do not name \
             the premise that was never established; an unproven row must say \
             which input or earlier assertion is missing, or the label is \
             hiding a defect"
        );
    }
    // If this drops to zero the rows were renamed and the check silently
    // stopped guarding anything.
    assert!(
        checked >= 5,
        "expected at least five unproven-by-reclassification rows, found {checked}"
    );
}

/// `tests/i3/coverage.toml` is the source of truth for i3 conformance
/// classification, and `contrib/coverage-report --check` validates it: every
/// assertion classified exactly once, every non-pass carrying a reason, every
/// skip carrying a citation, and the manifest agreeing with the per-file
/// counts.
///
/// The classification used to live only in Markdown prose, which meant counting
/// it required reverse-engineering English. That produced a fail total of 880
/// when the real figure was 208, and a green ceiling of 107 when it was 105 --
/// always overstating defects, which is the expensive direction because it
/// sends people hunting for bugs that are already explained.
///
/// The check is currently allowed a known number of violations, because the
/// data was extracted from that prose and the missing reasons and citations are
/// being filled in file by file. The budget only ever ratchets down: lowering
/// it is the unit of progress, and raising it requires deleting this comment
/// and explaining why a documented gap became undocumented again.
#[test]
fn coverage_data_validates_completely() {
    // Every assertion either passes or is a documented, cited skip. This began
    // as a ratchet at 165 violations while the data was extracted from prose;
    // it is now an invariant, so any entry that claims something it has not
    // shown fails the build.
    let output = std::process::Command::new("python3")
        .arg("contrib/coverage-report")
        .arg("--check")
        .output()
        .expect("contrib/coverage-report runs");
    let report = String::from_utf8(output.stdout).expect("report is UTF-8");
    let count: usize = report
        .lines()
        .last()
        .expect("the report ends with a violation count")
        .split_once(" violation")
        .expect("the last line states a violation count")
        .0
        .parse()
        .expect("the violation count is a number");

    assert_eq!(
        count, 0,
        "coverage.toml has {count} validation violations; run \
         contrib/coverage-report --check and fix each:\n{report}"
    );
}

//! Runner for unmodified layout tests from i3's Perl testsuite.

use std::any::Any;
use std::ffi::OsStr;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use wayland_client::Proxy as _;
use wayland_server::Resource as _;

use super::Fixture;
use crate::utils::transaction::Transaction;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);

struct AllowedRejection {
    test: &'static str,
    command: &'static str,
    reason: &'static str,
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
                    && command.starts_with("[id= . ")
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
        test: "111-goto.t",
        command: "[con_mark=\"*\"] focus",
        reason: "test asserts that an unknown mark leaves focus unchanged",
    },
    AllowedRejection {
        test: "132-move-workspace.t",
        command: "[con_mark=a] move to workspace *",
        reason: "the test expects moving an empty workspace by criteria to be a no-op",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "move gibberish",
        reason: "the regression intentionally sends this invalid command eleven times",
    },
    AllowedRejection {
        test: "120-multiple-cmds.t",
        command: "bullshit-command-which-we-never-implement meh",
        reason: "the test asserts that this invalid command returns an error",
    },
    AllowedRejection {
        test: "169-border-toggle.t",
        command: "border 1pixel",
        reason: "i3-only alias; sway accepts the equivalent border pixel 1",
    },
    AllowedRejection {
        test: "134-invalid-command.t",
        command: "blargh!",
        reason: "the regression intentionally sends an invalid command",
    },
    AllowedRejection {
        test: "101-focus.t",
        command: "[con_mark=__does_not_exist] focus",
        reason: "the assertion expects this unmatched criterion to fail",
    },
    AllowedRejection {
        test: "119-match.t",
        command: "[con_id=\"99999\"] kill",
        reason: "the test verifies that an unmatched criterion leaves the window alive",
    },
    AllowedRejection {
        test: "260-invalid-criteria.t",
        command: "[con_id=foobar] kill",
        reason: "the test intentionally sends a malformed con_id criterion",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_id=__focused__ app_id=doesnotmatch] kill",
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "261-match-con_id-con_mark-combinations.t",
        command: "[con_mark=marked app_id=doesnotmatch] kill",
        reason: "the test expects the combined criterion not to match",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[con_mark=doesnotexist] focus output right",
        reason: "the assertion expects the unmatched criterion to leave output focus unchanged",
    },
    AllowedRejection {
        test: "502-focus-output.t",
        command: "[id= . *] focus output right",
        reason:
            "unchanged upstream file contains this malformed criterion and expects no focus change",
    },
    AllowedRejection {
        test: "294-focus-order.t",
        command: "[id=*] swap container with id *",
        reason: "sway's id swap target is an X11 window id unavailable to native Wayland clients",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[app_id=b] swap with id *",
        reason: "i3's optional swap words and X11 id target are unavailable in sway",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark V1",
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "302-tree.t",
        command: "[con_mark=S1] swap with mark T1",
        reason: "i3 permits omitted swap words; sway requires swap container with mark",
    },
    AllowedRejection {
        test: "126-regress-close.t",
        command: "mode toggle",
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "127-regress-floating-parent.t",
        command: "mode toggle",
        reason: "obsolete setup cannot create or restore the floating container under test",
    },
    AllowedRejection {
        test: "142-regress-move-floating.t",
        command: "mode toggle",
        reason: "obsolete setup leaves the window tiled instead of testing a floating move",
    },
    AllowedRejection {
        test: "144-regress-floating-resize.t",
        command: "mode toggle",
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "147-regress-floatingmove.t",
        command: "mode toggle",
        reason: "obsolete setup leaves the parent tiled instead of testing floating-tree moves",
    },
    AllowedRejection {
        test: "151-regress-float-size.t",
        command: "mode toggle",
        reason: "obsolete setup omits both floating-to-tiling transitions under test",
    },
    AllowedRejection {
        test: "152-regress-level-up.t",
        command: "mode toggle",
        reason: "stale i3 floating setup; mode now selects a binding mode",
    },
    AllowedRejection {
        test: "192-layout.t",
        command: "layout toggle stacked",
        reason: "documented i3/sway layout-toggle divergence",
    },
    AllowedRejection {
        test: "292-regress-layout-toggle.t",
        command: "layout toggle 1337 1337",
        reason: "the regression intentionally sends invalid layout names",
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
    rejected.len() == expected.len()
        && rejected
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

fn create_window(fixture: &mut Fixture, client: super::client::ClientId, request: &Value) -> u32 {
    let fullscreen_output = request["fullscreen_output"]
        .as_str()
        .map(|name| fixture.client(client).output(name));
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
    window.surface.id().protocol_id()
}

fn map_window(fixture: &mut Fixture, client: super::client::ClientId, surface_id: u32) -> i64 {
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
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
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

fn translate_config_file(config: &str) -> Result<(PathBuf, swayward_config::Config), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = socket_path("config");
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
    if !stderr.trim().eq("manual attention: none") {
        return Err(format!("i3 config translation was incomplete:\n{stderr}"));
    }
    let translated = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    let path = socket_path("translated-config");
    std::fs::write(&path, translated).map_err(|error| error.to_string())?;
    let config = swayward_config::Config::load(&path)
        .config
        .map_err(|error| format!("{error:?}"))?;
    Ok((path, config))
}

fn translate_config(config: &str) -> Result<swayward_config::Config, String> {
    let (path, config) = translate_config_file(config)?;
    let _ = std::fs::remove_file(path);
    Ok(config)
}

fn prepare_test_config(source: &str) -> Result<swayward_config::Config, String> {
    let mut config = translate_config(source)?;
    config.layout.gaps = 0.;
    config.layout.border.off = false;
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
    stream: UnixStream,
) {
    let mut request = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut request)
        .unwrap();
    let request: Value = serde_json::from_str(&request).unwrap();
    let reply = match request["action"].as_str().unwrap() {
        "config" => {
            let source = request["config"].as_str().unwrap();
            match (fake_outputs(source), translate_config_file(source)) {
                (Ok(outputs), Ok((path, mut config))) => {
                    config.layout.gaps = 0.;
                    config.layout.border.off = false;
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
            json!({ "id": map_window(fixture, client, handle) })
        }
        "map" => json!({
            "id": map_window(fixture, client, request["handle"].as_u64().unwrap() as u32)
        }),
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
    config.input.focus_follows_mouse = Some(swayward_config::input::FocusFollowsMouse {
        max_scroll_amount: None,
    });
    config.animations.window_movement.0.off = true;
    config.animations.window_resize.anim.off = true;
    let mut fixture = Fixture::with_config(config);
    fixture.add_output(1, (1280, 800));
    let client = fixture.add_client();

    let handle = fixture.swayward().event_loop.clone();
    let ipc_server =
        crate::ipc::server::IpcServer::start(&handle, Some(OsStr::new("i3-tests"))).unwrap();
    let ipc_socket = ipc_server.socket_path.clone().unwrap();
    fixture.swayward().ipc_server = Some(ipc_server);
    fixture.niri_state().ipc_keyboard_layouts_changed();
    fixture.niri_state().ipc_refresh_layout();

    let control_path = socket_path("control");
    let control = UnixListener::bind(&control_path).unwrap();
    control.set_nonblocking(true).unwrap();

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
    let deadline = started + Duration::from_secs(30);
    let mut loaded_config_source = None;
    loop {
        fixture.dispatch();
        match control.accept() {
            Ok((stream, _)) => {
                handle_control(&mut fixture, client, &mut loaded_config_source, stream)
            }
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
            assert!(
                status.success() && rejections_match(test, &rejected),
                "i3 test {test} failed or its rejected commands changed\nTAP failures:\n{}\nexpected rejections: {expected:?}\nactual rejections: {rejected:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
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

/// Upstream i3 files that pass in full, one per line, comments with `#`.
///
/// A conformance slice adds a file here the moment it goes green. Keeping the
/// list in its own file rather than in this runner lets slices land in
/// parallel without editing the same Rust source.
const PASSING: &str = include_str!("../../tests/i3/passing.txt");

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
fn i3_config_translation_rejects_unhandled_directives() {
    let error = translate_config("font monospace\nmystery value\n").unwrap_err();
    assert!(error.contains("manual attention: 1 directive(s)"));
    assert!(error.contains("unhandled: mystery value"));
}

fn passing_tests() -> impl Iterator<Item = &'static str> {
    PASSING
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty())
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
    assert!(count > 0, "tests/i3/passing.txt lists no conformance files");
}

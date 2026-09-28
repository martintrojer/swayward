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
    initial_floating: bool,
) -> i64 {
    if initial_floating {
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
    if initial_floating {
        fixture.swayward().config.borrow_mut().window_rules.pop();
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
    let surface = fixture
        .client(client)
        .state
        .windows
        .iter()
        .find(|window| window.surface.id().protocol_id() == surface_id)
        .map(|window| window.surface.clone());
    let Some(surface) = surface else {
        return false;
    };
    fixture.client(client).window(&surface).attach_null();
    fixture.client(client).window(&surface).commit();
    fixture.double_roundtrip(client);
    fixture
        .client(client)
        .state
        .windows
        .retain(|window| window.surface.id().protocol_id() != surface_id);
    true
}

fn settle_configures(fixture: &mut Fixture, client: super::client::ClientId) {
    fixture.double_roundtrip(client);
    let windows = &mut fixture.client(client).state.windows;
    for window in windows {
        let count = window.configures_received.len();
        window.configures_looked_at = count;
        let Some((serial, configure)) = window.configures_received.last() else {
            continue;
        };
        if window.last_acked_configure == Some(*serial) {
            continue;
        }
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
            line.contains(": bar blocks are unsupported; use waybar ")
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
                mode: swayward_config::input::FocusFollowsMouseMode::Yes,
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
    initially_floating: &mut HashSet<u32>,
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
                                mode: swayward_config::input::FocusFollowsMouseMode::Yes,
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
        "create" => {
            let handle = create_window(fixture, client, &request);
            if request["initial_floating"].as_bool() == Some(true) {
                initially_floating.insert(handle);
            }
            json!({ "handle": handle })
        }
        "open" => {
            let handle = create_window(fixture, client, &request);
            json!({
                "id": map_window(
                    fixture,
                    client,
                    handle,
                    requested_size(&request),
                    request["initial_floating"].as_bool() == Some(true),
                )
            })
        }
        "map" => {
            let handle = request["handle"].as_u64().unwrap() as u32;
            json!({
                "id": map_window(
                    fixture,
                    client,
                    handle,
                    requested_size(&request),
                    initially_floating.remove(&handle),
                )
            })
        }
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


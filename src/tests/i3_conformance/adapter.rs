struct I3Scratch {
    path: PathBuf,
}

impl I3Scratch {
    fn new() -> Self {
        let root = std::env::var_os("SWAYWARD_TEST_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/tmp"));
        let path = root.join(format!(
            "swayward-i3.{}.{}",
            std::process::id(),
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    fn unique_path(&self, kind: &str, extension: &str) -> PathBuf {
        self.path.join(format!(
            "{kind}.{}.{extension}",
            NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

impl Drop for I3Scratch {
    fn drop(&mut self) {
        // The IPC server socket can still be present when the fixture drops.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn client_surface(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    surface_id: u32,
) -> Option<wayland_client::protocol::wl_surface::WlSurface> {
    fixture
        .client(client)
        .state
        .windows
        .iter()
        .find(|window| window.surface.id().protocol_id() == surface_id)
        .map(|window| window.surface.clone())
}

fn surface_id_for_window(fixture: &mut Fixture, id: i64) -> Option<u32> {
    fixture.swayward().layout.windows().find_map(|(_, mapped)| {
        (crate::ipc::tree::window_id(mapped.id()) == id)
            .then(|| mapped.toplevel().wl_surface().id().protocol_id())
    })
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
    let surface = client_surface(fixture, client, surface_id).unwrap();
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
    let surface_id = surface_id_for_window(fixture, id);
    let Some(surface_id) = surface_id else {
        return false;
    };
    let surface = client_surface(fixture, client, surface_id).unwrap();
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
    let surface_id = surface_id_for_window(fixture, id);
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
    scratch: &I3Scratch,
) -> Result<(PathBuf, swayward_config::Config), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = scratch.unique_path("config", "kdl");
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
    let path = scratch.unique_path("translated-config", "kdl");
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

fn translate_config(test: &str, config: &str) -> Result<swayward_config::Config, String> {
    let scratch = I3Scratch::new();
    translate_config_file(test, config, &scratch).map(|(_, config)| config)
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

/// The harness defaults every translated config shares, plus the per-file
/// overrides for `test`. `test` is passed explicitly rather than read from
/// `SWAYWARD_I3_TEST`: that variable is set only when one file is selected,
/// so reading it here made the gate and a single-file measurement load
/// different configs for the same file.
fn apply_harness_policy(
    config: &mut swayward_config::Config,
    source: Option<&str>,
    test: &str,
) {
    let source = source.unwrap_or_default();
    if !source.lines().any(|line| {
        line.trim_start()
            .to_ascii_lowercase()
            .starts_with("gaps inner ")
    }) {
        config.layout.gaps = 0.;
    }
    config.layout.border.off = false;
    config.animations.window_movement.0.off = true;
    config.animations.window_resize.anim.off = true;
    configure_client_state_oracle(config, test);
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
}

fn prepare_test_config(test: &str, source: &str) -> Result<swayward_config::Config, String> {
    let mut config = translate_config(test, source)?;
    apply_harness_policy(&mut config, Some(source), test);
    Ok(config)
}

pub(super) fn reload_test_config(
    fixture: &mut Fixture,
    test: &str,
    source: &str,
) -> Result<(), String> {
    let config = prepare_test_config(test, source)?;
    fixture.swayward().for_window.clear();
    fixture.niri_state().reload_config(Ok(config));
    Ok(())
}

fn reload_loaded_test_config(
    fixture: &mut Fixture,
    test: &str,
    source: Option<&str>,
) -> Result<(), String> {
    let source = source.ok_or_else(|| "no test config has been loaded".to_owned())?;
    reload_test_config(fixture, test, source)
}

fn reset_config(fixture: &mut Fixture, test: &str) -> Value {
    // i3test's `launch_with_config('-default')` requests the harness baseline,
    // not swayward's user-facing KDL defaults.
    let mut config = swayward_config::Config::default();
    apply_harness_policy(&mut config, None, test);
    config.gestures.hot_corners.off = true;
    fixture.niri_state().reload_config(Ok(config));
    json!({ "success": true })
}

/// Per-file state of one conformance run, owned by `run_i3_test`.
struct Session<'a> {
    test: &'a str,
    client: super::client::ClientId,
    loaded_config_source: Option<String>,
    scratch: &'a I3Scratch,
    initially_floating: HashSet<u32>,
}

fn load_config_source(fixture: &mut Fixture, session: &mut Session, source: &str) -> Value {
    let (outputs, path, mut config) =
        match (fake_outputs(source), translate_config_file(session.test, source, session.scratch)) {
            (Ok(outputs), Ok((path, config))) => (outputs, path, config),
            (Err(error), _) | (_, Err(error)) => {
                return json!({ "success": false, "error": error })
            }
        };
    if let Some(server) = &fixture.swayward().ipc_server {
        server.set_loaded_config_file_name(path.to_string_lossy().into_owned());
    }
    apply_harness_policy(&mut config, Some(source), session.test);
    fixture.swayward().layout.initialize_workspaces_from_bindings(&config);
    fixture.swayward().for_window.clear();
    fixture.niri_state().reload_config(Ok(config));
    crate::utils::watcher::setup(
        fixture.niri_state(),
        &swayward_config::ConfigPath::Explicit(path),
        Vec::new(),
    );
    if let Some(outputs) = outputs {
        fixture.replace_outputs(outputs);
        fixture.double_roundtrip(session.client);
    }
    session.loaded_config_source = Some(source.to_owned());
    json!({ "success": true })
}

#[derive(serde::Deserialize)]
struct WindowRequest {
    app_id: Option<String>,
    name: Option<String>,
    fullscreen_output: Option<String>,
    requested_width: Option<u16>,
    requested_height: Option<u16>,
    initial_floating: Option<bool>,
}

impl WindowRequest {
    fn size(&self) -> Option<(u16, u16)> {
        Some((self.requested_width?, self.requested_height?))
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Control {
    ConfigDefault,
    Config { config: String },
    Reload,
    Create { #[serde(flatten)] window: WindowRequest },
    Open { #[serde(flatten)] window: WindowRequest },
    Map { handle: u32, requested_width: Option<u16>, requested_height: Option<u16> },
    Fullscreen { handle: u32, enabled: bool },
    SetTitle { handle: u32, title: String },
    SetParent { handle: u32, parent_handle: u32 },
    Close { id: i64 },
    Focused,
    Activate { id: i64 },
    WindowStates { id: i64 },
    PointerButton { button: u32, pressed: bool },
    PointerAxis { horizontal_v120: f64, vertical_v120: f64 },
    KeyEvent { key: u32, pressed: bool },
    SetXkbGroup { group: u32 },
    TypeKeyChords { chords: Vec<Vec<u32>> },
    WarpPointer { x: f64, y: f64 },
    PrepareResize,
    ReapClosed { #[serde(default)] settle_configures: bool },
    RemoveAllWindows,
    RequestStop,
}

fn handle_control(fixture: &mut Fixture, session: &mut Session, stream: UnixStream) {
    let mut request = String::new();
    let reply = match BufReader::new(stream.try_clone().unwrap()).read_line(&mut request) {
        Ok(_) => match serde_json::from_str::<Control>(&request) {
            Ok(control) => dispatch_control(fixture, session, control),
            Err(error) => json!({ "success": false, "error": format!("invalid control request: {error}") }),
        },
        Err(error) => json!({ "success": false, "error": format!("cannot read control request: {error}") }),
    };
    writeln!(&stream, "{reply}").unwrap();
}

fn dispatch_control(fixture: &mut Fixture, session: &mut Session, control: Control) -> Value {
    match control {
        Control::ConfigDefault => reset_config(fixture, session.test),
        Control::Config { config } => load_config_source(fixture, session, &config),
        Control::Reload => match reload_loaded_test_config(
            fixture,
            session.test,
            session.loaded_config_source.as_deref(),
        ) {
            Ok(()) => json!({ "success": true }),
            Err(error) => json!({ "success": false, "error": error }),
        },
        Control::Create { window } => create_control_window(fixture, session, window, false),
        Control::Open { window } => create_control_window(fixture, session, window, true),
        Control::Map { handle, requested_width, requested_height } => {
            if client_surface(fixture, session.client, handle).is_none() {
                return json!({ "success": false, "error": format!("unknown surface handle {handle}") });
            }
            let size = requested_width.zip(requested_height);
            json!({ "id": map_window(
                fixture,
                session.client,
                handle,
                size,
                session.initially_floating.remove(&handle),
            ) })
        }
        Control::Fullscreen { handle, enabled } => window_control(fixture, session.client, handle, |window| {
            if enabled { window.set_fullscreen(None); } else { window.unset_fullscreen(); }
        }),
        Control::SetTitle { handle, title } => window_control(fixture, session.client, handle, |window| window.set_title(&title)),
        Control::SetParent { handle, parent_handle } => set_parent_control(fixture, session.client, handle, parent_handle),
        Control::Close { id } => json!({ "success": close_window(fixture, session.client, id) }),
        Control::Focused => json!({ "id": fixture.swayward().layout.focus().map(|mapped| crate::ipc::tree::window_id(mapped.id())) }),
        Control::Activate { id } => json!({ "success": activate_window(fixture, session.client, id) }),
        Control::WindowStates { id } => json!({
            "states": window_states(fixture, session.client, id),
            "xdg_wm_base_version": fixture.client(session.client).state.xdg_wm_base_version,
        }),
        input => dispatch_input_control(fixture, session.client, input),
    }
}

fn create_control_window(
    fixture: &mut Fixture,
    session: &mut Session,
    request: WindowRequest,
    map: bool,
) -> Value {
    let request_value = json!({
        "app_id": request.app_id,
        "name": request.name,
        "fullscreen_output": request.fullscreen_output,
        "requested_width": request.requested_width,
        "requested_height": request.requested_height,
    });
    let handle = create_window(fixture, session.client, &request_value);
    if !map {
        if request.initial_floating == Some(true) {
            session.initially_floating.insert(handle);
        }
        return json!({ "handle": handle });
    }
    json!({ "id": map_window(
        fixture,
        session.client,
        handle,
        request.size(),
        request.initial_floating == Some(true),
    ) })
}

fn window_control(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    handle: u32,
    action: impl FnOnce(&super::client::Window),
) -> Value {
    let Some(surface) = client_surface(fixture, client, handle) else {
        return json!({ "success": false, "error": format!("unknown surface handle {handle}") });
    };
    action(fixture.client(client).window(&surface));
    fixture.double_roundtrip(client);
    json!({ "success": true })
}

fn set_parent_control(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    handle: u32,
    parent_handle: u32,
) -> Value {
    let Some(surface) = client_surface(fixture, client, handle) else {
        return json!({ "success": false, "error": format!("unknown surface handle {handle}") });
    };
    let Some(parent_surface) = client_surface(fixture, client, parent_handle) else {
        return json!({ "success": false, "error": format!("unknown parent surface handle {parent_handle}") });
    };
    let wayland_client = fixture.client(client);
    let parent = wayland_client.window(&parent_surface).xdg_toplevel.clone();
    wayland_client.window(&surface).set_parent(Some(&parent));
    fixture.double_roundtrip(client);
    json!({ "success": true })
}

fn dispatch_input_control(
    fixture: &mut Fixture,
    client: super::client::ClientId,
    control: Control,
) -> Value {
    match control {
        Control::PointerButton { button, pressed } => super::ipc::pointer_button(fixture, button, pressed),
        Control::PointerAxis { horizontal_v120, vertical_v120 } => super::ipc::pointer_axis(fixture, horizontal_v120, vertical_v120),
        Control::KeyEvent { key, pressed } => super::ipc::key_event(fixture, key, pressed),
        Control::SetXkbGroup { group } => {
            let Some(keyboard) = fixture.swayward().seat.get_keyboard() else {
                return json!({ "success": false, "error": "no keyboard" });
            };
            keyboard.with_xkb_state(fixture.niri_state(), |mut context| {
                context.set_layout(smithay::input::keyboard::Layout(group));
            });
        }
        Control::TypeKeyChords { chords } => {
            let chords = chords.iter().map(Vec::as_slice).collect::<Vec<_>>();
            super::ipc::type_key_chords(fixture, &chords);
        }
        Control::WarpPointer { x, y } => {
            settle_configures(fixture, client);
            fixture.swayward().clock.set_complete_instantly(true);
            fixture.swayward().layout.advance_animations();
            fixture.swayward().clock.set_complete_instantly(false);
            super::ipc::pointer_motion_absolute(fixture, x, y);
            let location = (x, y).into();
            let under = fixture.swayward().contents_under(location);
            fixture.swayward().handle_focus_follows_mouse(&under);
            fixture.niri_state().move_cursor(location);
        }
        Control::PrepareResize => settle_configures(fixture, client),
        Control::ReapClosed { settle_configures: settle } => {
            if settle { settle_configures(fixture, client); }
            reap_closed_windows(fixture, client);
        }
        Control::RemoveAllWindows => remove_all_windows(fixture, client),
        Control::RequestStop => fixture.niri_state().request_stop("exit"),
        _ => return json!({ "success": false, "error": "control action is not an input or lifecycle action" }),
    }
    json!({ "success": true })
}

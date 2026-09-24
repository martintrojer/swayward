use std::fmt::{self, Write as _};

use insta::assert_snapshot;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use smithay::reexports::wayland_protocols::xdg::shell::client::xdg_toplevel;
use swayward_config::Config;

use super::*;
use crate::layout::LayoutElement as _;
use crate::utils::spawning::store_and_increase_nofile_rlimit;
use crate::utils::with_toplevel_role;

#[test]
fn simple_no_workspaces() {
    let mut f = Fixture::new();

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    assert_snapshot!(
        window.format_recent_configures(),
        @"size: 0 × 0, bounds: 0 × 0, states: []"
    );

    window.attach_new_buffer();
    window.set_size(100, 100);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let window = f.client(id).window(&surface);
    assert_snapshot!(
        window.format_recent_configures(),
        @"size: 1248 × 688, bounds: 1248 × 688, states: []"
    );
}

#[test]
fn production_windows_are_constructed_only_from_xdg_toplevels() {
    fn visit(path: &std::path::Path, constructors: &mut Vec<String>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                if path.file_name().is_some_and(|name| name == "tests") {
                    continue;
                }
                visit(&path, constructors);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                for line in source.lines().filter(|line| line.contains("Window::new_")) {
                    constructors.push(format!("{}: {line}", path.display()));
                }
            }
        }
    }

    let mut constructors = Vec::new();
    visit(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut constructors,
    );
    assert_eq!(
        constructors.len(),
        1,
        "production Window constructors changed: {constructors:#?}"
    );
    assert!(
        constructors[0].contains("Window::new_wayland_window(surface)"),
        "production Window must retain the xdg-toplevel role: {constructors:#?}"
    );
}

#[test]
fn commits_before_mapping_and_after_unmapping_keep_wayland_role_invariants() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();

    // Repeated pre-map commits must remain in the Unmapped path, whose Window was constructed
    // from this xdg-toplevel and therefore has a Wayland toplevel role.
    window.commit();
    window.commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().unmapped_windows.len(), 1);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    assert!(f.swayward().unmapped_windows.is_empty());
    assert_eq!(f.swayward().layout.windows().count(), 1);

    let window = f.client(id).window(&surface);
    window.attach_null();
    window.commit();
    f.double_roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());
    assert_eq!(f.swayward().unmapped_windows.len(), 1);

    // A later commit still takes the unmapped path and can issue a fresh initial configure.
    f.client(id).window(&surface).commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().unmapped_windows.len(), 1);
}

#[test]
fn commit_after_unmapped_toplevel_role_is_destroyed_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().unmapped_windows.len(), 1);

    f.client(id).window(&surface).destroy_role();
    f.roundtrip(id);
    assert!(f.swayward().unmapped_windows.is_empty());

    f.client(id).window(&surface).commit();
    f.double_roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());
    assert!(f.swayward().unmapped_windows.is_empty());
}

#[test]
fn commit_after_mapped_toplevel_role_is_destroyed_is_safe() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);
    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    assert_eq!(f.swayward().layout.windows().count(), 1);

    f.client(id).window(&surface).destroy_role();
    f.roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());

    f.client(id).window(&surface).commit();
    f.double_roundtrip(id);
    assert!(f.swayward().layout.windows().next().is_none());
    assert!(f.swayward().unmapped_windows.is_empty());
}

#[test]
fn simple() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    assert_snapshot!(
        window.format_recent_configures(),
        @"size: 1888 × 1048, bounds: 1888 × 1048, states: []"
    );

    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let window = f.client(id).window(&surface);
    assert_snapshot!(
        window.format_recent_configures(),
        @"size: 1888 × 1048, bounds: 1888 × 1048, states: [Activated]"
    );
}

#[test]
fn focusing_a_window_deactivates_the_previous_window() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let first = f.client(id).create_window();
    let first_surface = first.surface.clone();
    first.commit();
    f.roundtrip(id);
    let first = f.client(id).window(&first_surface);
    first.attach_new_buffer();
    first.ack_last_and_commit();
    f.double_roundtrip(id);
    let _ = f.client(id).window(&first_surface).recent_configures();

    let second = f.client(id).create_window();
    let second_surface = second.surface.clone();
    second.commit();
    f.roundtrip(id);
    let second = f.client(id).window(&second_surface);
    second.attach_new_buffer();
    second.ack_last_and_commit();
    f.double_roundtrip(id);

    assert!(f
        .client(id)
        .window(&first_surface)
        .recent_configures()
        .any(|configure| !configure.states.contains(&xdg_toplevel::State::Activated)));
}

#[test]
#[should_panic(expected = "Protocol error 3 on object xdg_surface")]
fn dont_ack_initial_configure() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    // Don't ack the configure.
    window.commit();
    f.double_roundtrip(id);
}

#[derive(Clone, Copy)]
enum WantFullscreen {
    No,
    UnsetBeforeInitial,
    BeforeInitial(Option<&'static str>),
    UnsetAfterInitial,
    AfterInitial(Option<&'static str>),
}

impl fmt::Display for WantFullscreen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WantFullscreen::No => write!(f, "U")?,
            WantFullscreen::UnsetBeforeInitial => write!(f, "BU")?,
            WantFullscreen::UnsetAfterInitial => write!(f, "AU")?,
            WantFullscreen::BeforeInitial(m) => write!(f, "B{}", m.unwrap_or("N"))?,
            WantFullscreen::AfterInitial(m) => write!(f, "A{}", m.unwrap_or("N"))?,
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum WantMaximized {
    No,
    UnsetBeforeInitial,
    BeforeInitial,
    UnsetAfterInitial,
    AfterInitial,
}

impl fmt::Display for WantMaximized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WantMaximized::No => write!(f, "U")?,
            WantMaximized::UnsetBeforeInitial => write!(f, "BU")?,
            WantMaximized::UnsetAfterInitial => write!(f, "AU")?,
            WantMaximized::BeforeInitial => write!(f, "B")?,
            WantMaximized::AfterInitial => write!(f, "A")?,
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum SetParent {
    BeforeInitial(&'static str),
    AfterInitial(&'static str),
}

impl fmt::Display for SetParent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetParent::BeforeInitial(m) => write!(f, "B{m}")?,
            SetParent::AfterInitial(m) => write!(f, "A{m}")?,
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum DefaultSize {
    WindowChooses,
}

impl fmt::Display for DefaultSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefaultSize::WindowChooses => write!(f, "U"),
        }
    }
}

#[test]
fn assigned_window_on_another_output_does_not_steal_focus() {
    let config = Config::parse_mem(
        r#"
window-rule {
    match app-id="assigned"
    open-on-output "headless-2"
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    f.add_output(2, (1280, 720));
    f.niri_focus_output(1);
    let client = f.add_client();

    let focused = f.client(client).create_window();
    focused.xdg_toplevel.set_app_id("focused".into());
    focused.xdg_toplevel.set_title("focused".into());
    focused.commit();
    let focused_surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&focused_surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    let assigned = f.client(client).create_window();
    assigned.xdg_toplevel.set_app_id("assigned".into());
    assigned.xdg_toplevel.set_title("assigned".into());
    assigned.commit();
    let assigned_surface = assigned.surface.clone();
    f.roundtrip(client);
    let assigned = f.client(client).window(&assigned_surface);
    assigned.attach_new_buffer();
    assigned.ack_last_and_commit();
    f.double_roundtrip(client);

    assert_eq!(f.swayward().layout.focus().unwrap().id(), focused);
    assert_eq!(
        f.swayward().layout.active_output().unwrap().name(),
        "headless-1"
    );
    let assigned_output = f
        .swayward()
        .layout
        .windows()
        .find(|(_, mapped)| mapped.id() != focused)
        .and_then(|(monitor, _)| monitor)
        .unwrap();
    assert_eq!(assigned_output.output_name(), "headless-2");
}

#[test]
fn assigned_window_on_another_workspace_does_not_steal_focus() {
    let config = Config::parse_mem(
        r#"
window-rule {
    match app-id="assigned"
    open-on-workspace "target"
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    let focused = f.client(client).create_window();
    focused.xdg_toplevel.set_app_id("focused".into());
    focused.commit();
    let focused_surface = focused.surface.clone();
    f.roundtrip(client);
    let focused = f.client(client).window(&focused_surface);
    focused.attach_new_buffer();
    focused.ack_last_and_commit();
    f.double_roundtrip(client);
    let focused = f.swayward().layout.focus().unwrap().id();

    let assigned = f.client(client).create_window();
    assigned.xdg_toplevel.set_app_id("assigned".into());
    assigned.commit();
    let assigned_surface = assigned.surface.clone();
    f.roundtrip(client);
    let assigned = f.client(client).window(&assigned_surface);
    assigned.attach_new_buffer();
    assigned.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let (_, target) = swayward.layout.find_workspace_by_name("target").unwrap();
    assert!(target.windows().any(|window| window.id() != focused));
    assert_eq!(swayward.layout.focus().unwrap().id(), focused);
}

#[test]
fn sway_default_floating_border_applies_to_initial_floats() {
    let config = Config::parse_mem(
        r#"
window-rule {
    match app-id="floating"
    open-floating true
}
window-rule {
    sway-floating-border "pixel"
    sway-floating-border-width 3
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    let client = f.add_client();

    let window = f.client(client).create_window();
    window.xdg_toplevel.set_app_id("floating".into());
    window.commit();
    let surface = window.surface.clone();
    f.roundtrip(client);
    let window = f.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(client);

    let swayward = f.swayward();
    let mapped = swayward.layout.windows().next().unwrap().1;
    let id = mapped.window.clone();
    assert!(swayward.layout.active_workspace().unwrap().is_floating(&id));
    assert_eq!(
        swayward.layout.window_border(&id),
        Some((swayward_ipc::command::BorderStyle::Pixel, 3))
    );
}

#[test]
fn workspace_number_rule_matches_digit_prefix_but_not_a_longer_number() {
    let config = Config::parse_mem(
        r#"
window-rule {
    match app-id="numbered"
    open-on-workspace-number "2"
}
window-rule {
    match app-id="named"
    open-on-workspace "2"
}
"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    // Create "21" first so a longer number exists that the rule must not
    // match, and pin it with a window. An empty workspace that focus has left
    // is destroyed (measured on sway 1.11; see 115-ipc-workspaces.t), so "21"
    // cannot be kept alive merely by having been visited.
    f.swayward()
        .layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("21".into()))
        .unwrap();
    let keeper = f.add_client();
    let window = f.client(keeper).create_window();
    window.xdg_toplevel.set_app_id("keeper".into());
    window.commit();
    let keeper_surface = window.surface.clone();
    f.roundtrip(keeper);
    let window = f.client(keeper).window(&keeper_surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(keeper);
    f.swayward()
        .layout
        .activate_sway_workspace(crate::command::WorkspaceTarget::Name("2: targetws".into()))
        .unwrap();
    let client = f.add_client();

    for app_id in ["numbered", "named"] {
        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(app_id.into());
        window.commit();
        let surface = window.surface.clone();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);
        if app_id == "numbered" {
            assert_eq!(
                f.swayward()
                    .layout
                    .find_workspace_by_name("2: targetws")
                    .unwrap()
                    .1
                    .windows()
                    .count(),
                1
            );
            assert_eq!(
                f.swayward()
                    .layout
                    .find_workspace_by_name("21")
                    .unwrap()
                    .1
                    .windows()
                    .count(),
                // Only the keeper pinning "21" alive: the rule must not send
                // the "numbered" window to this longer number.
                1
            );
        }
    }

    assert_eq!(
        f.swayward()
            .layout
            .find_workspace_by_name("2: targetws")
            .unwrap()
            .1
            .windows()
            .count(),
        1
    );
    assert_eq!(
        f.swayward()
            .layout
            .workspaces()
            .find(|(_, _, workspace)| workspace.sway_name().as_deref() == Some("2"))
            .unwrap()
            .2
            .windows()
            .count(),
        1
    );
}

#[test]
fn target_output_and_workspaces() {
    store_and_increase_nofile_rlimit();

    // Here we test a massive powerset of settings that can affect where a window opens:
    //
    // * open-on-workspace
    // * open-on-output
    // * has parent (windows will open next to their parent)
    // * want fullscreen (windows can request the target fullscreen output)
    // * open-fullscreen (can deny the fullscreen request)

    let open_on_workspace = [None, Some("1"), Some("2")];
    let open_on_output = [None, Some("1"), Some("2")];
    let open_fullscreen = [None, Some("false"), Some("true")];
    let want_fullscreen = [
        WantFullscreen::No,
        WantFullscreen::UnsetBeforeInitial, // GTK 4
        WantFullscreen::BeforeInitial(None),
        WantFullscreen::BeforeInitial(Some("1")),
        WantFullscreen::BeforeInitial(Some("2")),
        WantFullscreen::UnsetAfterInitial,
        // mpv, osu!
        WantFullscreen::AfterInitial(None),
        WantFullscreen::AfterInitial(Some("1")),
        WantFullscreen::AfterInitial(Some("2")),
    ];
    let set_parent = [
        None,
        Some(SetParent::BeforeInitial("1")),
        Some(SetParent::BeforeInitial("2")),
        Some(SetParent::AfterInitial("1")),
        Some(SetParent::AfterInitial("2")),
    ];

    let mut powerset = Vec::new();
    for ws in open_on_workspace {
        for out in open_on_output {
            for fs in open_fullscreen {
                for wfs in want_fullscreen {
                    for sp in set_parent {
                        powerset.push((ws, out, fs, wfs, sp));
                    }
                }
            }
        }
    }

    powerset.into_par_iter().for_each(|(ws, out, fs, wfs, sp)| {
        check_target_output_and_workspace(ws, out, fs, wfs, sp);
    });
}

fn check_target_output_and_workspace(
    open_on_workspace: Option<&str>,
    open_on_output: Option<&str>,
    open_fullscreen: Option<&str>,
    want_fullscreen: WantFullscreen,
    set_parent: Option<SetParent>,
) {
    let mut snapshot_desc = Vec::new();
    let mut snapshot_suffix = Vec::new();

    let mut config = String::from(
        r##"
output "headless-2" {
    layout {
        border {
            on
        }
    }
}

workspace "ws-1" {
    open-on-output "headless-1"
}

workspace "ws-2" {
    open-on-output "headless-2"

    layout {
        border {
            width 10
        }

        default-column-width {
            fixed 500
        }
    }
}

window-rule {
    exclude title="parent"

"##,
    );

    if let Some(x) = open_on_workspace {
        writeln!(config, "    open-on-workspace \"ws-{x}\"").unwrap();
        snapshot_suffix.push(format!("ws{x}"));
    }

    if let Some(x) = open_on_output {
        writeln!(config, "    open-on-output \"headless-{x}\"").unwrap();
        snapshot_suffix.push(format!("out{x}"));
    }

    if let Some(x) = open_fullscreen {
        writeln!(config, "    open-fullscreen {x}").unwrap();

        let x = if x == "true" { "T" } else { "F" };
        snapshot_suffix.push(format!("fs{x}"));
    }
    config.push('}');

    match &want_fullscreen {
        WantFullscreen::No => (),
        x => {
            snapshot_desc.push(format!("want fullscreen: {x}"));
            snapshot_suffix.push(format!("wfs{x}"));
        }
    }

    if let Some(set_parent) = set_parent {
        let mon = match set_parent {
            SetParent::BeforeInitial(mon) => mon,
            SetParent::AfterInitial(mon) => mon,
        };
        write!(
            config,
            "

window-rule {{
    match title=\"parent\"
    open-on-output \"headless-{mon}\"
}}"
        )
        .unwrap();

        snapshot_desc.push(format!("set parent: {set_parent}"));
        snapshot_suffix.push(format!("sp{set_parent}"));
    }

    snapshot_desc.push(format!("config:{config}"));

    let config = Config::parse_mem(&config).unwrap();

    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));

    let id = f.add_client();

    // To get output names.
    f.roundtrip(id);

    let mut parent = None;
    if set_parent.is_some() {
        let window = f.client(id).create_window();
        let surface = window.surface.clone();
        parent = Some(window.xdg_toplevel.clone());
        window.set_title("parent");
        window.commit();
        f.roundtrip(id);

        let window = f.client(id).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.roundtrip(id);
    }

    let client = f.client(id);
    let window = client.create_window();
    let surface = window.surface.clone();

    if let Some(SetParent::BeforeInitial(_)) = set_parent {
        client.window(&surface).set_parent(parent.as_ref());
    }

    if let WantFullscreen::UnsetBeforeInitial = want_fullscreen {
        client.window(&surface).unset_fullscreen();
    } else if let WantFullscreen::BeforeInitial(mon) = want_fullscreen {
        let output = mon.map(|mon| client.output(&format!("headless-{mon}")));
        client.window(&surface).set_fullscreen(output.as_ref());
    }

    client.window(&surface).commit();
    f.roundtrip(id);

    let client = f.client(id);
    let initial = client.window(&surface).format_recent_configures();

    if let Some(SetParent::AfterInitial(_)) = set_parent {
        client.window(&surface).set_parent(parent.as_ref());
    }

    if let WantFullscreen::UnsetAfterInitial = want_fullscreen {
        client.window(&surface).unset_fullscreen();
    } else if let WantFullscreen::AfterInitial(mon) = want_fullscreen {
        let output = mon.map(|mon| client.output(&format!("headless-{mon}")));
        client.window(&surface).set_fullscreen(output.as_ref());
    }

    let window = client.window(&surface);
    window.attach_new_buffer();
    let serial = window.configures_received.last().unwrap().0;
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    // Commit to the post-initial configures.
    let window = f.client(id).window(&surface);
    let new_serial = window.configures_received.last().unwrap().0;
    if new_serial != serial {
        window.ack_last_and_commit();
        f.double_roundtrip(id);
    }

    let swayward = f.swayward();
    let (mon, ws_idx, ws, mapped) = swayward
        .layout
        .workspaces()
        .find_map(|(mon, ws_idx, ws)| {
            ws.windows().find_map(|win| {
                if with_toplevel_role(win.toplevel(), |role| {
                    role.title.as_deref() != Some("parent")
                }) {
                    Some((mon, ws_idx, ws, win))
                } else {
                    None
                }
            })
        })
        .unwrap();
    let is_fullscreen = mapped.sizing_mode().is_fullscreen();
    let win = mapped.window.clone();
    let mon = mon.unwrap().output_name().clone();
    let ws = ws.name().cloned().unwrap_or(String::from("unnamed"));

    let window = f.client(id).window(&surface);
    let post_map = window.format_recent_configures();

    // If the window ended up fullscreen, unfullscreen it and output the configure.
    let mut post_unfullscreen = String::new();
    if is_fullscreen {
        f.swayward().layout.set_fullscreen(&win, false);
        f.double_roundtrip(id);

        let window = f.client(id).window(&surface);
        post_unfullscreen = format!(
            "\n\nunfullscreen configure:\n{}",
            window.format_recent_configures()
        );
    }

    let snapshot = format!(
        "\
final monitor: {mon}
final workspace: {ws_idx} ({ws})

initial configure:
{initial}

post-map configures:
{post_map}{post_unfullscreen}",
    );

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_suffix(snapshot_suffix.join("-"));
    settings.set_description(snapshot_desc.join("\n"));
    let _guard = settings.bind_to_scope();
    assert_snapshot!(snapshot);
}

#[test]
fn target_size() {
    store_and_increase_nofile_rlimit();

    // Exercise the sizing rules that can change a sole i3 leaf. Fixed and proportional
    // column widths and window heights, and within-column tab strips, do not affect it.
    //
    // * want fullscreen
    // * open-fullscreen
    // * open-maximized
    // * open-floating
    // * window-chosen width and height
    // * border

    let open_fullscreen = [None, Some("false"), Some("true")];
    let want_fullscreen = [
        WantFullscreen::No,
        WantFullscreen::UnsetBeforeInitial, // GTK 4
        WantFullscreen::BeforeInitial(None),
        WantFullscreen::UnsetAfterInitial,
        // mpv, osu!
        WantFullscreen::AfterInitial(None),
    ];
    let open_maximized = [None, Some("true")];
    let open_floating = [None, Some("true")];
    let default_column_width = [None, Some(DefaultSize::WindowChooses)];
    let default_window_height = [None, Some(DefaultSize::WindowChooses)];
    let border = [false, true];

    let mut powerset = Vec::new();
    for fs in open_fullscreen {
        for wfs in want_fullscreen {
            for om in open_maximized {
                for of in open_floating {
                    for dw in default_column_width {
                        for dh in default_window_height {
                            for b in border {
                                powerset.push((fs, wfs, om, of, dw, dh, b));
                            }
                        }
                    }
                }
            }
        }
    }

    powerset
        .into_par_iter()
        .for_each(|(fs, wfs, om, of, dw, dh, b)| {
            check_target_size(fs, wfs, om, of, dw, dh, b);
        });
}

#[allow(clippy::too_many_arguments)]
fn check_target_size(
    open_fullscreen: Option<&str>,
    want_fullscreen: WantFullscreen,
    open_maximized: Option<&str>,
    open_floating: Option<&str>,
    default_width: Option<DefaultSize>,
    default_height: Option<DefaultSize>,
    border: bool,
) {
    let mut snapshot_desc = Vec::new();
    let mut snapshot_suffix = Vec::new();

    let mut config = String::from(
        r##"
window-rule {
"##,
    );

    if let Some(x) = open_fullscreen {
        writeln!(config, "    open-fullscreen {x}").unwrap();

        let x = if x == "true" { "T" } else { "F" };
        snapshot_suffix.push(format!("fs{x}"));
    }

    if let Some(x) = open_maximized {
        writeln!(config, "    open-maximized {x}").unwrap();

        let x = if x == "true" { "T" } else { "F" };
        snapshot_suffix.push(format!("om{x}"));
    }

    if let Some(x) = open_floating {
        writeln!(config, "    open-floating {x}").unwrap();

        let x = if x == "true" { "T" } else { "F" };
        snapshot_suffix.push(format!("of{x}"));
    }

    if let Some(x) = default_width {
        let value = match x {
            DefaultSize::WindowChooses => String::new(),
        };
        writeln!(config, "    default-column-width {{ {value} }}").unwrap();

        snapshot_suffix.push(format!("dw{x}"));
    }

    if let Some(x) = default_height {
        let value = match x {
            DefaultSize::WindowChooses => String::new(),
        };
        writeln!(config, "    default-window-height {{ {value} }}").unwrap();

        snapshot_suffix.push(format!("dh{x}"));
    }

    if border {
        writeln!(config, "    border {{ on; }}").unwrap();
        snapshot_suffix.push(String::from("b"));
    }

    config.push('}');

    match &want_fullscreen {
        WantFullscreen::No => (),
        x => {
            snapshot_desc.push(format!("want fullscreen: {x}"));
            snapshot_suffix.push(format!("wfs{x}"));
        }
    }

    snapshot_desc.push(format!("config:{config}"));

    let config = Config::parse_mem(&config).unwrap();

    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));

    let id = f.add_client();

    // To get output names.
    f.roundtrip(id);

    let client = f.client(id);
    let window = client.create_window();
    let surface = window.surface.clone();

    if let WantFullscreen::UnsetBeforeInitial = want_fullscreen {
        client.window(&surface).unset_fullscreen();
    } else if let WantFullscreen::BeforeInitial(mon) = want_fullscreen {
        let output = mon.map(|mon| client.output(&format!("headless-{mon}")));
        client.window(&surface).set_fullscreen(output.as_ref());
    }

    client.window(&surface).commit();
    f.roundtrip(id);

    let client = f.client(id);
    let initial = client.window(&surface).format_recent_configures();

    if let WantFullscreen::UnsetAfterInitial = want_fullscreen {
        client.window(&surface).unset_fullscreen();
    } else if let WantFullscreen::AfterInitial(mon) = want_fullscreen {
        let output = mon.map(|mon| client.output(&format!("headless-{mon}")));
        client.window(&surface).set_fullscreen(output.as_ref());
    }

    let window = client.window(&surface);
    window.attach_new_buffer();
    let serial = window.configures_received.last().unwrap().0;
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    // Commit to the post-initial configures.
    let window = f.client(id).window(&surface);
    let new_serial = window.configures_received.last().unwrap().0;
    if new_serial != serial {
        window.ack_last_and_commit();
        f.double_roundtrip(id);
    }

    let window = f.client(id).window(&surface);
    let post_map = window.format_recent_configures();

    // If the window ended up fullscreen, unfullscreen it and output the configure.
    let mut post_unfullscreen = String::new();
    let mapped = f.swayward().layout.windows().next().unwrap().1;
    let is_fullscreen = mapped.sizing_mode().is_fullscreen();
    let win = mapped.window.clone();
    if is_fullscreen {
        f.swayward().layout.set_fullscreen(&win, false);
        f.double_roundtrip(id);

        let window = f.client(id).window(&surface);
        post_unfullscreen = format!(
            "\n\nunfullscreen configure:\n{}",
            window.format_recent_configures()
        );
    }

    let snapshot = format!(
        "\
initial configure:
{initial}

post-map configures:
{post_map}{post_unfullscreen}",
    );

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_suffix(snapshot_suffix.join("-"));
    settings.set_description(snapshot_desc.join("\n"));
    let _guard = settings.bind_to_scope();
    assert_snapshot!(snapshot);
}

#[test]
fn fullscreen_maximize() {
    store_and_increase_nofile_rlimit();

    let open_fullscreen = [None, Some("false"), Some("true")];
    let want_fullscreen = [
        WantFullscreen::No,
        WantFullscreen::UnsetBeforeInitial, // GTK 4
        WantFullscreen::BeforeInitial(None),
        WantFullscreen::UnsetAfterInitial,
        // mpv, osu!
        WantFullscreen::AfterInitial(None),
    ];
    let open_maximized = [None, Some("false"), Some("true")];
    let want_maximized = [
        WantMaximized::No,
        WantMaximized::UnsetBeforeInitial,
        WantMaximized::BeforeInitial,
        WantMaximized::UnsetAfterInitial,
        WantMaximized::AfterInitial,
    ];

    let mut powerset = Vec::new();
    for fs in open_fullscreen {
        for wfs in want_fullscreen {
            for tm in open_maximized {
                for wm in want_maximized {
                    powerset.push((fs, wfs, tm, wm));
                }
            }
        }
    }

    powerset.into_par_iter().for_each(|(fs, wfs, tm, wm)| {
        check_fullscreen_maximize(fs, wfs, tm, wm);
    });
}

fn check_fullscreen_maximize(
    open_fullscreen: Option<&str>,
    want_fullscreen: WantFullscreen,
    open_maximized: Option<&str>,
    want_maximized: WantMaximized,
) {
    let mut snapshot_desc = Vec::new();
    let mut snapshot_suffix = Vec::new();

    let mut config = String::from(
        r##"
window-rule {
"##,
    );

    if let Some(x) = open_fullscreen {
        writeln!(config, "    open-fullscreen {x}").unwrap();

        let x = if x == "true" { "T" } else { "F" };
        snapshot_suffix.push(format!("fs{x}"));
    }

    if let Some(x) = open_maximized {
        writeln!(config, "    open-maximized-to-edges {x}").unwrap();

        let x = if x == "true" { "T" } else { "F" };
        snapshot_suffix.push(format!("tm{x}"));
    }

    config.push('}');

    match &want_fullscreen {
        WantFullscreen::No => (),
        x => {
            snapshot_desc.push(format!("want fullscreen: {x}"));
            snapshot_suffix.push(format!("wfs{x}"));
        }
    }

    match &want_maximized {
        WantMaximized::No => (),
        x => {
            snapshot_desc.push(format!("want maximized: {x}"));
            snapshot_suffix.push(format!("wm{x}"));
        }
    }

    snapshot_desc.push(format!("config:{config}"));

    let config = Config::parse_mem(&config).unwrap();

    let mut f = Fixture::with_config(config);
    f.add_output(1, (1280, 720));
    f.add_output(2, (1920, 1080));

    let id = f.add_client();

    // To get output names.
    f.roundtrip(id);

    let client = f.client(id);
    let window = client.create_window();
    let surface = window.surface.clone();

    if let WantMaximized::UnsetBeforeInitial = want_maximized {
        client.window(&surface).unset_maximized();
    } else if let WantMaximized::BeforeInitial = want_maximized {
        client.window(&surface).set_maximized();
    }

    if let WantFullscreen::UnsetBeforeInitial = want_fullscreen {
        client.window(&surface).unset_fullscreen();
    } else if let WantFullscreen::BeforeInitial(mon) = want_fullscreen {
        let output = mon.map(|mon| client.output(&format!("headless-{mon}")));
        client.window(&surface).set_fullscreen(output.as_ref());
    }

    client.window(&surface).commit();
    f.roundtrip(id);

    let client = f.client(id);
    let initial = client.window(&surface).format_recent_configures();

    if let WantMaximized::UnsetAfterInitial = want_maximized {
        client.window(&surface).unset_maximized();
    } else if let WantMaximized::AfterInitial = want_maximized {
        client.window(&surface).set_maximized();
    }

    if let WantFullscreen::UnsetAfterInitial = want_fullscreen {
        client.window(&surface).unset_fullscreen();
    } else if let WantFullscreen::AfterInitial(mon) = want_fullscreen {
        let output = mon.map(|mon| client.output(&format!("headless-{mon}")));
        client.window(&surface).set_fullscreen(output.as_ref());
    }

    let window = client.window(&surface);
    window.attach_new_buffer();
    let serial = window.configures_received.last().unwrap().0;
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    // Commit to the post-initial configures.
    let window = f.client(id).window(&surface);
    let new_serial = window.configures_received.last().unwrap().0;
    if new_serial != serial {
        window.ack_last_and_commit();
        f.double_roundtrip(id);
    }

    let window = f.client(id).window(&surface);
    let post_map = window.format_recent_configures();

    // If the window ended up fullscreen, unfullscreen it and output the configure.
    let mut post_unfullscreen = String::new();
    let mapped = f.swayward().layout.windows().next().unwrap().1;
    let is_fullscreen = mapped.sizing_mode().is_fullscreen();
    let win = mapped.window.clone();
    if is_fullscreen {
        f.swayward().layout.set_fullscreen(&win, false);
        f.double_roundtrip(id);

        let window = f.client(id).window(&surface);
        window.ack_last_and_commit();
        f.double_roundtrip(id);

        let window = f.client(id).window(&surface);
        post_unfullscreen = format!(
            "\n\nunfullscreen configure:\n{}",
            window.format_recent_configures()
        );
    }

    // If the window ended up maximized, unmaximize it and output the configure.
    let mut post_unmaximize = String::new();
    let mapped = f.swayward().layout.windows().next().unwrap().1;
    let is_maximized = mapped.sizing_mode().is_maximized();
    let win = mapped.window.clone();
    if is_maximized {
        f.swayward().layout.set_maximized(&win, false);
        f.double_roundtrip(id);

        let window = f.client(id).window(&surface);
        window.ack_last_and_commit();
        f.double_roundtrip(id);

        let window = f.client(id).window(&surface);
        post_unmaximize = format!(
            "\n\nunmaximize configure:\n{}",
            window.format_recent_configures()
        );
    }

    let snapshot = format!(
        "\
initial configure:
{initial}

post-map configures:
{post_map}{post_unfullscreen}{post_unmaximize}",
    );

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_suffix(snapshot_suffix.join("-"));
    settings.set_description(snapshot_desc.join("\n"));
    let _guard = settings.bind_to_scope();
    assert_snapshot!(snapshot);
}

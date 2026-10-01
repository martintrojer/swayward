include!("runtime_settings/focus_border_and_smart_gap.rs");
include!("runtime_settings/input_policy.rs");
include!("runtime_settings/floating_size.rs");
include!("runtime_settings/titlebar.rs");
include!("runtime_settings/border_and_popup.rs");
include!("runtime_settings/floating_modifier.rs");
include!("runtime_settings/mouse_warping.rs");

/// Oracle: command-fuzz config-only-workspace-layout,
/// config-only-default-orientation, config-only-orientation,
/// config-only-primary-selection and config-only-xwayland. At run time sway
/// searches `command_handlers` and the shared `handlers`, never
/// `config_handlers` (`sway/sway/commands.c:102-110,156-173`), and
/// `orientation` is in no table. All five are unknown over IPC and change
/// nothing.
#[test]
fn config_only_directives_are_unknown_at_runtime_like_sway() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let snapshot = |f: &mut Fixture| {
        let config = f.swayward().config.borrow();
        (
            config.layout.workspace_layout,
            config.layout.default_orientation,
            config.clipboard.disable_primary,
            config.xwayland_satellite.off,
        )
    };
    let before = snapshot(&mut f);

    for (command, name) in [
        ("workspace_layout tabbed", "workspace_layout"),
        ("default_orientation vertical", "default_orientation"),
        ("orientation vertical", "orientation"),
        ("primary_selection disabled", "primary_selection"),
        ("xwayland disable", "xwayland"),
    ] {
        let outcome = crate::command::execute(f.niri_state(), command);
        assert_eq!(
            outcome,
            vec![swayward_ipc::command::parse_error(format!(
                "Unknown/invalid command '{name}'"
            ))],
            "{command}"
        );
    }
    assert_eq!(snapshot(&mut f), before);
}

/// A KDL `workspace "name" { layout { gaps N } }` must reach the workspace
/// however it comes to exist. Sway reads the workspace config inside
/// `workspace_create` (`sway/sway/tree/workspace.c:224-243`), so this holds for
/// a workspace created on demand, not only one created eagerly at startup.
///
/// Regression test: workspaces carrying an output assignment skip eager
/// creation (`src/swayward.rs:1549-1552`), and the lazy creation path used to
/// drop the per-name layout entirely.
#[test]
fn configured_workspace_layout_applies_however_the_workspace_is_created() {
    for assignment in ["", "sway-output-assignment \"fake-1\""] {
        let config = swayward_config::Config::parse_mem(&format!(
            r#"
layout {{
    gaps 10
    border {{ off; }}
}}
workspace "roomy" {{
    {assignment}
    layout {{ gaps 45; }}
}}
"#
        ))
        .unwrap();
        let mut fixture = Fixture::with_config(config);
        fixture.add_output(1, (1280, 800));
        assert!(crate::command::execute(fixture.niri_state(), "workspace roomy")[0].success);
        add_two_tiled_windows(&mut fixture);
        assert_eq!(
            tiled_window_rects_on(&mut fixture, "roomy")[0]["x"],
            45,
            "assignment: {assignment:?}"
        );
    }
}

/// Workspace names in a stable order, for rename assertions.
fn workspace_names(fixture: &mut Fixture) -> Vec<String> {
    let swayward = fixture.swayward();
    let mut names = describe_workspaces(&swayward.layout, &swayward.global_space)
        .into_iter()
        .map(|workspace| workspace.name)
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// Put one window with `app_id` on each named workspace, leaving the last
/// created workspace focused.
fn windows_on_workspaces(fixture: &mut Fixture, plan: &[(&str, &str)]) {
    let client = fixture.add_client();
    for (workspace, app_id) in plan {
        assert!(
            crate::command::execute(fixture.niri_state(), &format!("workspace {workspace}"))[0]
                .success
        );
        let window = fixture.client(client).create_window();
        window.xdg_toplevel.set_app_id((*app_id).into());
        window.commit();
        let surface = window.surface.clone();
        fixture.roundtrip(client);
        let window = fixture.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        fixture.double_roundtrip(client);
    }
}


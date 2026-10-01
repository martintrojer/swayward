use swayward_config::Config;

use super::*;

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
fn sway_default_floating_rules_match_fixed_sizes_and_parents() {
    for (name, min_size, max_size, has_parent, expected_floating) in [
        ("fixed-width", (300, 100), (300, 200), false, true),
        ("fixed-height-zero-width", (0, 200), (0, 200), false, false),
        ("fixed-both", (300, 200), (300, 200), false, true),
        ("dialog", (0, 0), (0, 0), true, true),
    ] {
        let mut f = Fixture::new();
        f.add_output(1, (1280, 720));
        let client = f.add_client();

        let parent = has_parent.then(|| {
            let parent = f.client(client).create_window();
            let surface = parent.surface.clone();
            let toplevel = parent.xdg_toplevel.clone();
            parent.commit();
            f.roundtrip(client);
            let parent = f.client(client).window(&surface);
            parent.attach_new_buffer();
            parent.ack_last_and_commit();
            f.double_roundtrip(client);
            toplevel
        });

        let window = f.client(client).create_window();
        window.xdg_toplevel.set_app_id(name.into());
        window.set_min_size(min_size.0, min_size.1);
        window.set_max_size(max_size.0, max_size.1);
        window.set_parent(parent.as_ref());
        let surface = window.surface.clone();
        window.commit();
        f.roundtrip(client);
        let window = f.client(client).window(&surface);
        window.attach_new_buffer();
        window.ack_last_and_commit();
        f.double_roundtrip(client);

        let swayward = f.swayward();
        let mapped = swayward.layout.focus().unwrap();
        assert_eq!(
            swayward
                .layout
                .active_workspace()
                .unwrap()
                .is_floating(&mapped.window),
            expected_floating,
            "default floating state for {name}"
        );
    }
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

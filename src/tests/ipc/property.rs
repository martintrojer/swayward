use std::collections::HashSet;
use std::sync::atomic::Ordering;

use proptest::prelude::*;
use swayward_ipc::{Node, NodeProperties};
use wayland_client::protocol::wl_surface::WlSurface;

use super::*;

#[derive(Clone, Debug)]
enum Op {
    Command(&'static str),
    ConIdCommand(u8, &'static str),
    Open(u8),
    Close(u8),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        12 => prop::sample::select(COMMANDS).prop_map(Op::Command),
        2 => (0..6u8, prop::sample::select(CON_ID_COMMANDS))
            .prop_map(|(slot, command)| Op::ConIdCommand(slot, command)),
        2 => (0..6u8).prop_map(Op::Open),
        1 => (0..6u8).prop_map(Op::Close),
    ]
}

const CON_ID_COMMANDS: &[&str] = &[
    "focus",
    "floating toggle",
    "move workspace 1",
    "move scratchpad",
    "fullscreen toggle",
    "kill",
];

const COMMANDS: &[&str] = &[
    "focus left",
    "focus right",
    "focus up",
    "focus down",
    "focus parent",
    "focus child",
    "focus next",
    "focus prev",
    "focus tiling",
    "focus floating",
    "move left",
    "move right",
    "move up",
    "move down",
    "move workspace 1",
    "move workspace 2",
    "move workspace next",
    "move workspace prev",
    "move workspace to output left",
    "move workspace to output right",
    "move output left",
    "move output right",
    "move scratchpad",
    "scratchpad show",
    "floating toggle",
    "sticky toggle",
    "fullscreen toggle",
    "layout splith",
    "layout splitv",
    "layout tabbed",
    "layout stacking",
    "layout toggle split",
    "split horizontal",
    "split vertical",
    "resize grow width 10 px",
    "resize shrink height 5 ppt",
    "mark alpha",
    "mark beta",
    "unmark",
    "[con_mark=alpha] focus",
    "[con_mark=beta] move workspace 2",
    "[app_id=app-0] focus",
    "[app_id=app-1] floating toggle",
    "[app_id=app-2] move scratchpad",
    "workspace 1",
    "workspace 2",
    "workspace named",
    "workspace next",
    "workspace prev",
    "swap container with mark alpha",
    "kill",
];

fn map_window(fixture: &mut Fixture, client: super::client::ClientId, slot: u8) -> WlSurface {
    let window = fixture.client(client).create_window();
    window.xdg_toplevel.set_app_id(format!("app-{slot}"));
    window.set_title(&format!("window-{slot}"));
    let surface = window.surface.clone();
    window.commit();
    fixture.roundtrip(client);
    let window = fixture.client(client).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    fixture.double_roundtrip(client);
    surface
}

fn unmap_window(fixture: &mut Fixture, client: super::client::ClientId, surface: &WlSurface) {
    let window = fixture.client(client).window(surface);
    window.attach_null();
    window.commit();
    fixture.double_roundtrip(client);
}

fn collect_window_ids(node: &Node, ids: &mut Vec<i64>) {
    if matches!(node.properties, NodeProperties::View(_)) {
        ids.push(node.id);
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect_window_ids(child, ids);
    }
}

fn assert_focus_ids_are_live(node: &Node) {
    for focus in &node.focus {
        assert!(
            node.nodes
                .iter()
                .chain(&node.floating_nodes)
                .any(|child| child.id == *focus),
            "focus id {focus} is not a child of node {}",
            node.id
        );
    }
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        assert_focus_ids_are_live(child);
    }
}

fn assert_state(fixture: &mut Fixture) {
    fixture.swayward().layout.verify_invariants();
    fixture.niri_state().ipc_refresh_layout();

    let swayward = fixture.swayward();
    let tree = describe_tree(
        &swayward.layout,
        &swayward.global_space,
        &swayward.marks_by_window,
        &swayward.marks_by_container,
    );
    let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);

    let tree_json = serde_json::to_string(&tree).unwrap();
    let workspaces_json = serde_json::to_string(&workspaces).unwrap();
    let tree: Node = serde_json::from_str(&tree_json).unwrap();
    let _: Vec<swayward_ipc::Workspace> = serde_json::from_str(&workspaces_json).unwrap();

    let mut serialized_windows = Vec::new();
    collect_window_ids(&tree, &mut serialized_windows);
    let serialized_windows = serialized_windows.into_iter().collect::<HashSet<_>>();
    assert_eq!(
        serialized_windows.len(),
        swayward.layout.windows().count(),
        "every mapped window must appear exactly once in GET_TREE"
    );
    assert!(swayward.layout.windows().all(|(_, window)| {
        serialized_windows.contains(&crate::ipc::tree::window_id(window.id()))
    }));

    assert_focus_ids_are_live(&tree);
}

fn check_ops(ops: Vec<Op>) {
    let mut fixture = Fixture::new();
    fixture.swayward().clock.set_complete_instantly(true);
    fixture.add_output(1, (1280, 720));
    fixture.add_output_at(2, (1280, 720), Some((1280, 0)));
    let client = fixture.add_client();
    let mut windows: Vec<Option<WlSurface>> = vec![None; 6];
    windows[0] = Some(map_window(&mut fixture, client, 0));
    windows[1] = Some(map_window(&mut fixture, client, 1));
    assert_state(&mut fixture);

    for op in ops {
        match op {
            Op::Command(command) => {
                let _ = crate::command::execute(fixture.niri_state(), command);
                fixture.double_roundtrip(client);
                let closed = fixture
                    .client(client)
                    .state
                    .windows
                    .iter()
                    .filter(|window| {
                        window.close_requested
                            && windows
                                .iter()
                                .flatten()
                                .any(|surface| surface == &window.surface)
                    })
                    .map(|window| window.surface.clone())
                    .collect::<Vec<_>>();
                for surface in closed {
                    unmap_window(&mut fixture, client, &surface);
                    for slot in &mut windows {
                        if slot.as_ref() == Some(&surface) {
                            *slot = None;
                        }
                    }
                }
            }
            Op::ConIdCommand(slot, command) => {
                let mut ids = Vec::new();
                let swayward = fixture.swayward();
                collect_window_ids(
                    &describe_tree(
                        &swayward.layout,
                        &swayward.global_space,
                        &swayward.marks_by_window,
                        &swayward.marks_by_container,
                    ),
                    &mut ids,
                );
                if let Some(id) = ids.get(usize::from(slot) % ids.len().max(1)) {
                    let _ = crate::command::execute(
                        fixture.niri_state(),
                        &format!("[con_id={id}] {command}"),
                    );
                    fixture.double_roundtrip(client);
                }
            }
            Op::Open(slot) => {
                let slot = usize::from(slot);
                if windows[slot].is_none() {
                    windows[slot] = Some(map_window(&mut fixture, client, slot as u8));
                }
            }
            Op::Close(slot) => {
                let slot = usize::from(slot);
                if let Some(surface) = windows[slot].take() {
                    unmap_window(&mut fixture, client, &surface);
                }
            }
        }
        assert_state(&mut fixture);
    }

    for surface in windows.into_iter().flatten() {
        unmap_window(&mut fixture, client, &surface);
    }
    drop(fixture);
}

#[test]
fn repeated_property_fixtures_release_file_descriptors() {
    const CHILD_ENV: &str = "SWAYWARD_FD_REGRESSION_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::ipc::property::repeated_property_fixtures_release_file_descriptors",
                "--nocapture",
            ])
            .env(CHILD_ENV, "1")
            .status()
            .unwrap();
        assert!(status.success(), "isolated fixture fd regression failed");
        return;
    }

    check_ops(vec![Op::Command("focus left")]);
    let states_before = crate::swayward::LIVE_STATE_COUNT.load(Ordering::Relaxed);
    let fds_before = std::fs::read_dir("/proc/self/fd").unwrap().count();
    for _ in 0..200 {
        check_ops(vec![Op::Command("focus left")]);
    }
    let live_states = crate::swayward::LIVE_STATE_COUNT.load(Ordering::Relaxed);
    let leaked_fds = std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .count()
        .saturating_sub(fds_before);
    assert_eq!(live_states, states_before, "fixture State instances leaked");
    assert!(leaked_fds <= 8, "leaked {leaked_fds} file descriptors");
}

#[test]
fn swapping_a_marked_container_does_not_leave_multiple_fullscreen_nodes() {
    check_ops(vec![
        Op::Command("focus left"),
        Op::Command("move workspace 1"),
        Op::Command("split horizontal"),
        Op::Command("workspace 2"),
        Op::Open(2),
        Op::Command("move left"),
        Op::ConIdCommand(0, "fullscreen toggle"),
        Op::Command("move workspace 2"),
        Op::Command("focus parent"),
        Op::Command("mark alpha"),
        Op::Command("move right"),
        Op::Command("fullscreen toggle"),
        Op::Command("[app_id=app-0] focus"),
        Op::Command("swap container with mark alpha"),
    ]);
}

#[test]
fn swapping_with_a_marked_floating_container_is_rejected_safely() {
    check_ops(vec![
        Op::Open(2),
        Op::ConIdCommand(1, "move scratchpad"),
        Op::ConIdCommand(2, "floating toggle"),
        Op::Command("focus left"),
        Op::Command("focus left"),
        Op::Command("floating toggle"),
        Op::Command("focus up"),
        Op::Open(3),
        Op::Command("layout splitv"),
        Op::Command("focus parent"),
        Op::Command("mark alpha"),
        Op::Command("floating toggle"),
        Op::Command("[app_id=app-0] focus"),
        Op::Command("swap container with mark alpha"),
    ]);
}

#[test]
fn killed_windows_can_receive_late_configures() {
    check_ops(vec![
        Op::Command("focus parent"),
        Op::Command("floating toggle"),
        Op::ConIdCommand(0, "kill"),
        Op::ConIdCommand(0, "move scratchpad"),
        Op::Command("focus left"),
        Op::Command("scratchpad show"),
        Op::Command("layout tabbed"),
    ]);
}

#[test]
fn stale_mark_cannot_focus_an_empty_tiling_root() {
    check_ops(vec![
        Op::Command("focus parent"),
        Op::Command("focus right"),
        Op::Close(0),
        Op::Command("mark alpha"),
        Op::Command("move scratchpad"),
        Op::Command("[con_mark=alpha] focus"),
    ]);
}

#[test]
fn sticky_floating_window_stays_active_after_workspace_changes() {
    check_ops(vec![
        Op::Command("floating toggle"),
        Op::Command("sticky toggle"),
        Op::Command("move workspace 2"),
        Op::Command("workspace 2"),
        Op::Command("workspace named"),
    ]);
}

#[test]
fn runtime_default_border_keeps_floating_tile_data_current() {
    check_ops(vec![
        Op::Command("layout tabbed"),
        Op::Command("default_border pixel 2"),
        Op::ConIdCommand(0, "floating toggle"),
    ]);
}

#[test]
fn changing_from_tabbed_to_split_keeps_visible_tiles_consistent() {
    check_ops(vec![
        Op::Command("layout tabbed"),
        Op::Command("layout splith"),
    ]);
}

#[test]
fn closing_a_floating_group_child_keeps_the_resident_root() {
    check_ops(vec![
        Op::Command("layout splitv"),
        Op::Command("floating toggle"),
        Op::Command("focus parent"),
        Op::Command("floating toggle"),
        Op::Command("layout tabbed"),
        Op::Command("layout splith"),
        Op::Close(1),
        Op::Command("focus parent"),
        Op::Command("layout splith"),
    ]);
}

#[test]
fn targeted_scratchpad_window_can_be_moved_after_focus_changes() {
    check_ops(vec![
        Op::ConIdCommand(0, "move scratchpad"),
        Op::ConIdCommand(5, "move scratchpad"),
        Op::Command("focus right"),
        Op::ConIdCommand(0, "move workspace 1"),
    ]);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: if std::env::var_os("RUN_SLOW_TESTS").is_none() {
            eprintln!("ignoring slow test");
            0
        } else {
            ProptestConfig::default().cases
        },
        max_shrink_iters: 10_000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_commands_preserve_compositor_and_ipc_invariants(
        ops in prop::collection::vec(op(), 1..80),
    ) {
        check_ops(ops);
    }
}

use std::collections::HashSet;
use std::sync::atomic::Ordering;

use proptest::prelude::*;
use swayward_ipc::{CommandOutcome, Node, NodeProperties};
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

// Keep at least one executable form of every accepted family in
// tests/sway/compatibility.toml. Variants below exercise forms with distinct
// runtime paths rather than only the census probe.
const COMMANDS: &[&str] = &[
    "assign [app_id=\"app-3\"] workspace number 3",
    "bindcode 38 nop",
    "bindswitch lid:on nop",
    "bindsym Mod4+Return nop",
    "border normal",
    "border pixel 2",
    "border none",
    "border toggle",
    "client.focused_tab_title #333333 #5f676a #ffffff",
    "create_output",
    "default_border pixel 2",
    "default_floating_border normal",
    "exec nop",
    "exec_always nop",
    "exit",
    "floating_maximum_size 900 x 700",
    "floating_minimum_size 75 x 50",
    "floating_modifier Mod4",
    "focus_follows_mouse yes",
    "focus_on_window_activation smart",
    "focus_wrapping workspace",
    "font monospace 10",
    "for_window [app_id=\"app-4\"] border pixel 3",
    "force_display_urgency_hint 500 ms",
    "force_focus_wrapping yes",
    "gaps inner current set 10",
    "gaps outer current plus 2",
    "gaps horizontal current minus 1",
    "gaps vertical all toggle 5",
    "hide_edge_borders none",
    "input type:keyboard xkb_switch_layout next",
    "mode default",
    "mouse_warping output",
    "new_float pixel 2",
    "new_window normal",
    "no_focus [app_id=\"app-5\"]",
    "nop fuzz",
    "opacity set 0.5",
    "output * scale 1",
    "output * transform normal",
    "output * position 0 0",
    "output * power toggle",
    "popup_during_fullscreen smart",
    "reload",
    "rename workspace to fuzzed",
    "resize set 640 480",
    "resize set width 50 ppt",
    "resize set height 300 px",
    "set $fuzz value",
    "shortcuts_inhibitor enable",
    "show_marks yes",
    "smart_borders on",
    "smart_gaps on",
    "splith",
    "splitt",
    "splitv",
    "tiling_drag yes",
    "tiling_drag_threshold 9",
    "title_align center",
    "title_format %title",
    "titlebar_border_thickness 1",
    "titlebar_padding 4 3",
    "unbindcode 38",
    "unbindswitch lid:on",
    "unbindsym Mod4+Return",
    "urgent toggle",
    "workspace_auto_back_and_forth yes",
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
    "move position 50 px 75 px",
    "move position 20 ppt 30 ppt",
    "move position center",
    "move absolute position 100 px 125 px",
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
    "mark --add beta",
    "mark --toggle gamma",
    "unmark",
    "unmark beta",
    "[con_mark=alpha] focus",
    "[con_mark=beta] move workspace 2",
    "[app_id=app-0] focus",
    "[app_id=app-1] floating toggle",
    "[app_id=app-2] move scratchpad",
    "workspace 1",
    "workspace 2",
    "workspace number 3",
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

/// A finished operation sequence: the live fixture plus every command it
/// executed with its outcome, so a named regression can assert the result its
/// name promises rather than only the invariants `assert_state` checks.
struct Run {
    fixture: Fixture,
    client: super::client::ClientId,
    windows: Vec<Option<WlSurface>>,
    outcomes: Vec<(String, Vec<CommandOutcome>)>,
}

impl Run {
    /// The outcome of the last executed command that matches `command`.
    fn last_outcome(&self, command: &str) -> &CommandOutcome {
        let (_, outcome) = self
            .outcomes
            .iter()
            .rev()
            .find(|(executed, _)| executed == command)
            .unwrap_or_else(|| panic!("{command:?} never ran: {:?}", self.outcomes));
        assert_eq!(outcome.len(), 1, "{command}: {outcome:?}");
        &outcome[0]
    }

    fn last_outcome_ending_with(&self, command: &str) -> &CommandOutcome {
        let (_, outcome) = self
            .outcomes
            .iter()
            .rev()
            .find(|(executed, _)| executed.ends_with(command))
            .unwrap_or_else(|| panic!("{command:?} never ran: {:?}", self.outcomes));
        assert_eq!(outcome.len(), 1, "{command}: {outcome:?}");
        &outcome[0]
    }

    fn tree(&mut self) -> Node {
        let swayward = self.fixture.swayward();
        let tree = describe_tree(
            &swayward.layout,
            &swayward.global_space,
            &swayward.marks_by_window,
            &swayward.marks_by_container,
        );
        serde_json::from_value(serde_json::to_value(tree).unwrap()).unwrap()
    }

    fn workspaces(&mut self) -> Vec<swayward_ipc::Workspace> {
        let swayward = self.fixture.swayward();
        let workspaces = describe_workspaces(&swayward.layout, &swayward.global_space);
        serde_json::from_value(serde_json::to_value(workspaces).unwrap()).unwrap()
    }

    fn finish(mut self) {
        for surface in std::mem::take(&mut self.windows).into_iter().flatten() {
            unmap_window(&mut self.fixture, self.client, &surface);
        }
    }
}

fn view_app_id(node: &Node) -> Option<&str> {
    match &node.properties {
        NodeProperties::View(view) => view.app_id.as_deref(),
        _ => None,
    }
}

fn find_app<'a>(node: &'a Node, app_id: &str) -> Option<&'a Node> {
    if view_app_id(node) == Some(app_id) {
        return Some(node);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(|child| find_app(child, app_id))
}

fn app_workspace<'a>(node: &'a Node, app_id: &str) -> Option<&'a Node> {
    if node.node_type == swayward_ipc::NodeType::Workspace && find_app(node, app_id).is_some() {
        return Some(node);
    }
    node.nodes
        .iter()
        .chain(&node.floating_nodes)
        .find_map(|child| app_workspace(child, app_id))
}

fn collect_node_ids(node: &Node, ids: &mut Vec<i64>) {
    ids.push(node.id);
    for child in node.nodes.iter().chain(&node.floating_nodes) {
        collect_node_ids(child, ids);
    }
}

fn count_fullscreen_descendants(node: &Node) -> usize {
    usize::from(
        matches!(
            node.node_type,
            swayward_ipc::NodeType::Con | swayward_ipc::NodeType::FloatingCon
        ) && node.fullscreen_mode != 0,
    ) + node
        .nodes
        .iter()
        .chain(&node.floating_nodes)
        .map(count_fullscreen_descendants)
        .sum::<usize>()
}

fn assert_success(outcome: &CommandOutcome, command: &str) {
    assert!(outcome.success, "{command}: {outcome:?}");
}

fn assert_failure(outcome: &CommandOutcome, command: &str, error: &str) {
    assert!(!outcome.success, "{command}: {outcome:?}");
    assert_eq!(outcome.error.as_deref(), Some(error), "{command}");
}

fn check_ops(ops: Vec<Op>) {
    run_ops(ops).finish();
}

fn run_ops(ops: Vec<Op>) -> Run {
    let mut outcomes = Vec::new();
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
                outcomes.push((
                    command.to_owned(),
                    crate::command::execute(fixture.niri_state(), command),
                ));
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
                    let command = format!("[con_id={id}] {command}");
                    let outcome = crate::command::execute(fixture.niri_state(), &command);
                    outcomes.push((command, outcome));
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

    Run {
        fixture,
        client,
        windows,
        outcomes,
    }
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
    let mut run = run_ops(vec![
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

    assert_success(run.last_outcome("swap container with mark alpha"), "swap");
    let tree = run.tree();
    let workspaces = tree.nodes.iter().flat_map(|output| &output.nodes);
    assert!(workspaces.clone().any(|ws| ws.name.as_deref() == Some("1")));
    for workspace in workspaces {
        assert!(
            count_fullscreen_descendants(workspace) <= 1,
            "multiple fullscreen containers in workspace {:?}",
            workspace.name
        );
    }
    run.finish();
}

#[test]
fn swapping_with_a_marked_floating_container_is_rejected_safely() {
    let mut run = run_ops(vec![
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

    assert_failure(
        run.last_outcome("swap container with mark alpha"),
        "swap",
        "Can only swap with containers and views",
    );
    let tree = run.tree();
    for app_id in ["app-0", "app-1", "app-2", "app-3"] {
        assert!(find_app(&tree, app_id).is_some(), "missing {app_id}");
    }
    run.finish();
}

#[test]
fn showing_scratchpad_after_unfocused_floating_toggle_keeps_unique_trees() {
    let mut run = run_ops(vec![
        Op::Command("focus parent"),
        Op::Command("floating toggle"),
        Op::Open(2),
        Op::Command("focus parent"),
        Op::Command("focus right"),
        Op::ConIdCommand(1, "move scratchpad"),
        Op::Command("floating toggle"),
        Op::Command("scratchpad show"),
    ]);

    assert_success(run.last_outcome("scratchpad show"), "scratchpad show");
    let tree = run.tree();
    let mut ids = Vec::new();
    collect_node_ids(&tree, &mut ids);
    let unique = ids.iter().copied().collect::<HashSet<_>>();
    assert_eq!(ids.len(), unique.len(), "GET_TREE node ids must be unique");
    for app_id in ["app-0", "app-1", "app-2"] {
        assert_eq!(
            app_workspace(&tree, app_id).and_then(|node| node.name.as_deref()),
            Some("1"),
            "{app_id} workspace",
        );
    }
    run.finish();
}

#[test]
fn killed_windows_can_receive_late_configures() {
    let mut run = run_ops(vec![
        Op::Command("focus parent"),
        Op::Command("floating toggle"),
        Op::ConIdCommand(0, "kill"),
        Op::ConIdCommand(0, "move scratchpad"),
        Op::Command("focus left"),
        Op::Command("scratchpad show"),
        Op::Command("layout tabbed"),
    ]);

    assert_success(run.last_outcome_ending_with("kill"), "kill");
    assert_success(
        run.last_outcome_ending_with("move scratchpad"),
        "move scratchpad",
    );
    assert_failure(
        run.last_outcome("layout tabbed"),
        "layout tabbed",
        "Unable to change layout of floating windows",
    );
    let tree = run.tree();
    assert!(find_app(&tree, "app-0").is_none(), "killed window remained");
    assert_eq!(
        app_workspace(&tree, "app-1").and_then(|node| node.name.as_deref()),
        Some("2"),
    );
    run.finish();
}

#[test]
fn stale_mark_cannot_focus_an_empty_tiling_root() {
    let mut run = run_ops(vec![
        Op::Command("focus parent"),
        Op::Command("focus right"),
        Op::Close(0),
        Op::Command("mark alpha"),
        Op::Command("move scratchpad"),
        Op::Command("[con_mark=alpha] focus"),
    ]);

    assert_failure(
        run.last_outcome("mark alpha"),
        "mark alpha",
        "Only containers can have marks",
    );
    assert_failure(
        run.last_outcome("[con_mark=alpha] focus"),
        "[con_mark=alpha] focus",
        "No matching node.",
    );
    let workspaces = run.workspaces();
    assert!(workspaces.iter().any(|ws| ws.name == "1" && ws.focused));
    run.finish();
}

#[test]
fn sticky_floating_window_stays_active_after_workspace_changes() {
    let mut run = run_ops(vec![
        Op::Command("floating toggle"),
        Op::Command("sticky toggle"),
        Op::Command("move workspace 2"),
        Op::Command("workspace 2"),
        Op::Command("workspace named"),
    ]);

    for command in [
        "floating toggle",
        "sticky toggle",
        "move workspace 2",
        "workspace 2",
        "workspace named",
    ] {
        assert_success(run.last_outcome(command), command);
    }
    let tree = run.tree();
    let sticky = find_app(&tree, "app-1").unwrap();
    assert!(sticky.sticky);
    assert_eq!(sticky.node_type, swayward_ipc::NodeType::FloatingCon);
    assert_eq!(
        app_workspace(&tree, "app-1").and_then(|node| node.name.as_deref()),
        Some("named"),
    );
    assert!(run
        .workspaces()
        .iter()
        .any(|ws| ws.name == "named" && ws.focused && ws.visible));
    run.finish();
}

#[test]
fn runtime_default_border_keeps_floating_tile_data_current() {
    let mut run = run_ops(vec![
        Op::Command("layout tabbed"),
        Op::Command("default_border pixel 2"),
        Op::ConIdCommand(0, "floating toggle"),
    ]);

    assert_success(run.last_outcome("layout tabbed"), "layout tabbed");
    assert_success(run.last_outcome("default_border pixel 2"), "default_border");
    assert_success(
        run.last_outcome_ending_with("floating toggle"),
        "floating toggle",
    );
    let tree = run.tree();
    let floating = find_app(&tree, "app-0").unwrap();
    assert_eq!(floating.node_type, swayward_ipc::NodeType::FloatingCon);
    assert_eq!(floating.border, swayward_ipc::NodeBorder::Normal);
    assert!(floating.deco_rect.height > 0, "floating titlebar missing");
    assert_eq!(
        floating.window_rect.width + 2 * floating.current_border_width,
        floating.rect.width,
    );
    run.finish();
}

/// Sway refuses to rename a workspace to another live workspace's name
/// (`sway/sway/commands/rename.c:82-91`).
#[test]
fn rename_does_not_reuse_a_live_empty_workspace_name() {
    let mut run = run_ops(vec![
        Op::Command("workspace 2"),
        Op::ConIdCommand(2, "floating toggle"),
        Op::ConIdCommand(0, "floating toggle"),
        Op::Command("rename workspace to fuzzed"),
        Op::Command("focus right"),
        Op::Command("move workspace to output left"),
        Op::Close(0),
        Op::Open(2),
        Op::ConIdCommand(0, "move scratchpad"),
        Op::Open(3),
        Op::ConIdCommand(1, "move scratchpad"),
        Op::Command("workspace_auto_back_and_forth yes"),
        Op::ConIdCommand(0, "move workspace 1"),
        Op::Command("rename workspace to fuzzed"),
    ]);

    assert_success(
        run.last_outcome("rename workspace to fuzzed"),
        "rename workspace to fuzzed",
    );
    let workspaces = run.workspaces();
    let names = workspaces
        .iter()
        .map(|workspace| workspace.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names.iter().copied().collect::<HashSet<_>>().len(),
        names.len(),
    );
    assert_eq!(
        names.into_iter().collect::<HashSet<_>>(),
        HashSet::from(["2", "fuzzed"]),
    );
    assert!(workspaces
        .iter()
        .any(|workspace| workspace.name == "fuzzed" && workspace.focused));
    let tree = run.tree();
    assert_eq!(
        app_workspace(&tree, "app-1").and_then(|node| node.name.as_deref()),
        Some("fuzzed"),
    );
    run.finish();
}

#[test]
fn changing_from_tabbed_to_split_keeps_visible_tiles_consistent() {
    let mut run = run_ops(vec![
        Op::Command("layout tabbed"),
        Op::Command("layout splith"),
    ]);

    assert_success(run.last_outcome("layout tabbed"), "layout tabbed");
    assert_success(run.last_outcome("layout splith"), "layout splith");
    let tree = run.tree();
    let workspace = app_workspace(&tree, "app-0").unwrap();
    let parent = workspace
        .nodes
        .iter()
        .find(|node| find_app(node, "app-0").is_some())
        .unwrap();
    assert_eq!(parent.layout, swayward_ipc::NodeLayout::SplitH);
    let first = find_app(parent, "app-0").unwrap();
    let second = find_app(parent, "app-1").unwrap();
    assert_eq!(first.rect.width, second.rect.width);
    assert_eq!(first.rect.height, second.rect.height);
    assert_eq!(first.rect.x + first.rect.width, second.rect.x);
    run.finish();
}

#[test]
fn closing_a_floating_group_child_keeps_the_resident_root() {
    let mut run = run_ops(vec![
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

    assert_failure(
        run.last_outcome("layout tabbed"),
        "layout tabbed",
        "Unable to change layout of floating windows",
    );
    assert_success(run.last_outcome("layout splith"), "final layout splith");
    let tree = run.tree();
    assert!(find_app(&tree, "app-1").is_none());
    let workspace = app_workspace(&tree, "app-0").unwrap();
    assert_eq!(workspace.floating_nodes.len(), 1);
    assert!(find_app(&workspace.floating_nodes[0], "app-0").is_some());
    run.finish();
}

#[test]
fn targeted_scratchpad_window_can_be_moved_after_focus_changes() {
    let mut run = run_ops(vec![
        Op::ConIdCommand(0, "move scratchpad"),
        Op::ConIdCommand(5, "move scratchpad"),
        Op::Command("focus right"),
        Op::ConIdCommand(0, "move workspace 1"),
    ]);

    assert_success(
        run.last_outcome_ending_with("move workspace 1"),
        "move workspace 1",
    );
    let tree = run.tree();
    for app_id in ["app-0", "app-1"] {
        assert!(find_app(&tree, app_id).is_some(), "missing {app_id}");
    }
    run.finish();
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

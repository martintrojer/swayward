use std::time::Duration;

use swayward_ipc::{criteria, CommandOutcome};

use super::movement::{
    move_position, move_target_to_mark, move_target_to_workspace, move_tiling_subtree_to_output,
    move_workspace_to_output, output_target,
};
use super::{
    execute, failure, focus, layout, movement, parse_boolean, scratchpad, success, window,
    ClientColorClass, Command, CommandTarget, Direction, OutputTarget,
};
use crate::swayward::State;
use crate::window::mapped::ShortcutsInhibitPolicy;

fn move_target_to_adjacent_output(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
    activate: crate::layout::ActivateWindow,
) {
    let (reference, window) = match target {
        CommandTarget::Window(target) => {
            let Some(found) = state
                .swayward
                .layout
                .windows()
                .find_map(|(monitor, mapped)| {
                    (mapped.id() == target).then(|| {
                        (
                            monitor.map(|monitor| monitor.output()),
                            mapped.window.clone(),
                        )
                    })
                })
            else {
                return;
            };
            found
        }
        CommandTarget::Container(workspace, _) => {
            let Some((_, workspace)) = state.swayward.layout.find_workspace_by_id(workspace) else {
                return;
            };
            let Some(window) = workspace
                .active_window()
                .map(|mapped| mapped.window.clone())
            else {
                return;
            };
            let reference = state
                .swayward
                .layout
                .windows()
                .find_map(|(monitor, mapped)| {
                    (mapped.window == window)
                        .then(|| monitor.map(|monitor| monitor.output()))
                        .flatten()
                });
            (reference, window)
        }
    };
    let destination = OutputTarget::Direction(direction);
    let reference_point = state.swayward.layout.window_center(&window);
    if let Ok(output) = output_target(state, &destination, reference, reference_point) {
        match target {
            CommandTarget::Window(_) => state.swayward.layout.move_window_to_output_from_direction(
                &window,
                &output,
                match direction {
                    Direction::Left => crate::layout::tiling_tree::Direction::Left,
                    Direction::Right => crate::layout::tiling_tree::Direction::Right,
                    Direction::Up => crate::layout::tiling_tree::Direction::Up,
                    Direction::Down => crate::layout::tiling_tree::Direction::Down,
                },
                activate,
            ),
            CommandTarget::Container(workspace, node) => {
                let _ = move_tiling_subtree_to_output(state, workspace, node, &output);
            }
        }
    }
}

pub(super) fn move_direction(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
    pixels: Option<i32>,
    activate: crate::layout::ActivateWindow,
    focused: bool,
) -> CommandOutcome {
    if matches!(
        target,
        CommandTarget::Window(target)
            if state.swayward.layout.windows().any(|(_, mapped)| {
                mapped.id() == target
                    && state.swayward.layout.fullscreen_mode(&mapped.window)
                        == Some(crate::layout::tiling_tree::FullscreenMode::Global)
            })
    ) {
        return success();
    }
    let layout_direction = match direction {
        Direction::Left => crate::layout::tiling_tree::Direction::Left,
        Direction::Right => crate::layout::tiling_tree::Direction::Right,
        Direction::Up => crate::layout::tiling_tree::Direction::Up,
        Direction::Down => crate::layout::tiling_tree::Direction::Down,
    };
    let moved_within_workspace = if focused {
        let moved = match target {
            CommandTarget::Container(workspace, node) => state
                .swayward
                .layout
                .move_tiling_node_in_direction(workspace, node, layout_direction),
            CommandTarget::Window(_) => match direction {
                Direction::Left => state.swayward.layout.move_left(),
                Direction::Right => state.swayward.layout.move_right(),
                Direction::Up => state.swayward.layout.move_up(),
                Direction::Down => state.swayward.layout.move_down(),
            },
        };
        if !moved && state.swayward.layout.focused_fullscreen_mode().is_none() {
            move_target_to_adjacent_output(state, target, direction, activate);
        }
        moved
    } else {
        match target {
            CommandTarget::Window(target) => {
                let window =
                    state.swayward.layout.windows().find_map(|(_, mapped)| {
                        (mapped.id() == target).then(|| mapped.window.clone())
                    });
                let Some(window) = window else {
                    return failure("No matching node.");
                };
                let moved = state.swayward.layout.move_window_in_direction(
                    &window,
                    layout_direction,
                    f64::from(pixels.unwrap_or(10)),
                );
                if !moved {
                    move_target_to_adjacent_output(
                        state,
                        CommandTarget::Window(target),
                        direction,
                        activate,
                    );
                }
                moved
            }
            CommandTarget::Container(workspace, node) => state
                .swayward
                .layout
                .move_tiling_node_in_direction(workspace, node, layout_direction),
        }
    };
    state.swayward.queue_redraw_all();
    if moved_within_workspace && focused {
        if let CommandTarget::Window(window) = target {
            state.ipc_refresh_layout();
            if let Some(server) = &state.swayward.ipc_server {
                server.send_event(swayward_ipc::legacy::Event::WindowMoved {
                    id: crate::ipc::tree::window_id(window),
                });
            }
        }
    }
    success()
}

pub(super) fn set_client_colors(
    state: &mut State,
    class: ClientColorClass,
    colors: swayward_ipc::command::ClientColors,
) {
    let colors = swayward_config::TitlebarColors {
        border_color: swayward_config::Color::from_rgba8_unpremul(
            colors.border[0],
            colors.border[1],
            colors.border[2],
            colors.border[3],
        ),
        background_color: swayward_config::Color::from_rgba8_unpremul(
            colors.background[0],
            colors.background[1],
            colors.background[2],
            colors.background[3],
        ),
        text_color: swayward_config::Color::from_rgba8_unpremul(
            colors.text[0],
            colors.text[1],
            colors.text[2],
            colors.text[3],
        ),
    };
    let mut config = state.swayward.config.borrow_mut();
    let titlebar = &mut config.layout.titlebar;
    *match class {
        ClientColorClass::Focused => &mut titlebar.focused,
        ClientColorClass::FocusedInactive => &mut titlebar.focused_inactive,
        ClientColorClass::FocusedTabTitle => &mut titlebar.focused_tab_title,
        ClientColorClass::Unfocused => &mut titlebar.unfocused,
        ClientColorClass::Urgent => &mut titlebar.urgent,
    } = colors;
    state.swayward.layout.update_config(&config);
    drop(config);
    state.swayward.queue_redraw_all();
}

pub(super) fn execute_targeted(
    state: &mut State,
    command: &Command,
    target: CommandTarget,
) -> CommandOutcome {
    match command {
        Command::Mark {
            add,
            toggle,
            identifier,
        } => mark_target(state, target, identifier, *add, *toggle),
        Command::Unmark(identifier) => unmark_target(state, target, identifier.as_deref()),
        Command::Swap(swap_target) => {
            let outcome = movement::swap_target(state, target, swap_target);
            if !outcome.success {
                return outcome;
            }
        }
        Command::MoveDirection { direction, pixels } => {
            let outcome = move_direction(
                state,
                target,
                *direction,
                *pixels,
                crate::layout::ActivateWindow::No,
                false,
            );
            if !outcome.success {
                return outcome;
            }
        }
        Command::MovePosition(position) => {
            if let Err(error) = move_position(state, Some(target), position) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
        }
        Command::MoveToWorkspace {
            target: workspace_target,
            auto_back_and_forth,
        } => {
            let auto_back_and_forth = *auto_back_and_forth
                && state
                    .swayward
                    .config
                    .borrow()
                    .input
                    .workspace_auto_back_and_forth;
            let outcome = move_target_to_workspace(
                state,
                target,
                workspace_target.clone(),
                true,
                auto_back_and_forth,
            );
            if !outcome.success {
                return outcome;
            }
        }
        Command::MoveToMark(mark) => {
            let outcome = move_target_to_mark(state, target, mark);
            if !outcome.success {
                return outcome;
            }
        }
        Command::MoveWorkspaceToOutput(output_target_name) => {
            let outcome = move_workspace_to_output(state, Some(target), output_target_name);
            if !outcome.success {
                return outcome;
            }
        }
        Command::MoveToOutput(output_target_name) => {
            let CommandTarget::Window(target) = target else {
                return failure("command requires a window target");
            };
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(monitor, mapped)| {
                    (mapped.id() == target).then(|| {
                        (
                            monitor.map(|monitor| monitor.output()),
                            mapped.window.clone(),
                        )
                    })
                });
            let Some((reference, window)) = window else {
                return failure("No matching node.");
            };
            let reference_point = state.swayward.layout.window_center(&window);
            let output = match output_target(state, output_target_name, reference, reference_point)
            {
                Ok(output) => output,
                Err(error) => return failure(error),
            };
            state.swayward.layout.move_to_output(
                Some(&window),
                &output,
                None,
                crate::layout::ActivateWindow::No,
            );
            state.swayward.queue_redraw_all();
        }
        Command::MoveScratchpad => {
            if let Err(error) = scratchpad::move_targeted(state, target) {
                return error;
            }
        }
        Command::ScratchpadShow => {
            if let Err(error) = scratchpad::show_targeted(state, target) {
                return error;
            }
        }
        Command::Fullscreen { mode, global } => {
            if let Err(error) = layout::fullscreen_targeted(state, target, *mode, *global) {
                return error;
            }
        }
        Command::ShortcutsInhibitor(enable) => {
            if let Err(error) = set_shortcuts_inhibitor(state, target, *enable) {
                return error;
            }
        }
        Command::Sticky(value) => {
            if let Err(error) = window::sticky(state, target, value) {
                return error;
            }
        }
        Command::SetClientColors { class, colors } => {
            set_client_colors(state, *class, *colors);
        }
        Command::SetLayoutOption(_) => return failure("command cannot be applied to a container"),
        Command::Opacity(value) | Command::OpacityRelative(value) => {
            let relative = matches!(command, Command::OpacityRelative(_));
            if let Err(error) = window::opacity(state, target, *value, relative) {
                return error;
            }
        }
        Command::TitleFormat(format) => {
            if let Err(error) = window::title_format(state, target, format) {
                return error;
            }
        }
        Command::Border(border) => {
            if let Err(error) = window::border(state, target, border) {
                return error;
            }
        }
        Command::Floating(mode) => {
            if let Err(error) = window::floating(state, target, mode) {
                return error;
            }
            // Re-run this window's `for_window` commands now that its float
            // state has changed, so a `tiling` or `floating` criterion is
            // re-evaluated rather than only being applied at map time. i3
            // encodes the same idea through its tiling_from and floating_from
            // provenance criteria.
            if let CommandTarget::Window(window) = target {
                rerun_for_window_rules(state, window);
            }
        }
        Command::Urgent(value) => {
            let CommandTarget::Window(target) = target else {
                return failure("Only views can be urgent");
            };
            let urgent = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, window)| (window.id() == target).then(|| window.is_urgent()));
            let Some(urgent) = urgent else {
                return failure("No matching node.");
            };
            let urgent = parse_boolean(value, urgent);
            state.swayward.set_window_urgent(target, urgent);
            state.swayward.queue_redraw_all();
        }
        Command::Kill => {
            if let Err(error) = window::kill(state, target) {
                return error;
            }
        }
        Command::ResizeSet { width, height } => {
            if let Err(error) = window::resize_set(state, target, *width, *height) {
                return error;
            }
        }
        Command::Resize {
            grow,
            axis,
            first,
            second,
        } => {
            if let Err(error) = window::resize(state, target, *grow, *axis, *first, *second) {
                return error;
            }
        }
        Command::Focus => {
            if let Err(error) = focus::targeted(state, target) {
                return error;
            }
        }
        Command::FocusWorkspace => {
            if let Err(error) = focus::targeted_workspace(state, target) {
                return error;
            }
        }
        Command::FocusDirection(direction) => {
            if let Err(error) = focus::targeted_direction(state, target, *direction) {
                return error;
            }
        }
        Command::FocusOutput(identifier) => {
            if let Err(error) = focus::output(state, identifier) {
                return error;
            }
        }
        Command::Layout(value) => {
            if let Err(error) = layout::targeted(state, target, *value) {
                return error;
            }
        }
        Command::LayoutToggle(toggle) => {
            if let Err(error) = layout::toggle_targeted(state, target, toggle) {
                return error;
            }
        }
        Command::LayoutDefault => {
            if let Err(error) = layout::default_targeted(state, target) {
                return error;
            }
        }
        Command::Split(value) => {
            if let Err(error) = layout::split_targeted(state, target, *value) {
                return error;
            }
        }
        Command::RenameWorkspace { old, new_name } => {
            // Only the `rename workspace to <new>` form reads the matched
            // container's workspace. The `<old>` and `number <n>` forms resolve
            // by name regardless of criteria, and sway still runs the handler
            // once per match, so the second pass finds the old name gone and
            // fails (`sway/sway/commands/rename.c:35-58`).
            let resolved = match old {
                Some(target) => state
                    .swayward
                    .layout
                    .rename_sway_workspace(Some(target.clone()), new_name.clone()),
                None => match target_workspace(state, target) {
                    Some(workspace) => state
                        .swayward
                        .layout
                        .rename_sway_workspace_by_id(workspace, new_name.clone()),
                    // Sway's NULL workspace lands on the same message, because
                    // `!workspace` is the branch that reports it
                    // (`sway/sway/commands/rename.c:60-63`).
                    None => Err("There is no workspace with that name".to_owned()),
                },
            };
            if let Err(error) = resolved {
                return swayward_ipc::command::parse_error(error);
            }
            state.swayward.queue_redraw_all();
        }
        Command::Nop => {}
        _ => return failure("criteria targets are not implemented for this command yet"),
    }
    state.ipc_refresh_layout();
    success()
}

/// The workspace a criteria-matched target lives on.
///
/// Sway sets `handler_context.workspace` from the matched node before running
/// the handler, taking a container's `pending.workspace`
/// (`sway/sway/commands.c:181-202`). A hidden scratchpad container has a NULL
/// workspace there (`sway/sway/tree/container.c:1458`), so it resolves to
/// nothing rather than to the focused workspace.
fn target_workspace(
    state: &State,
    target: CommandTarget,
) -> Option<crate::layout::workspace::WorkspaceId> {
    match target {
        CommandTarget::Container(workspace, _) => Some(workspace),
        CommandTarget::Window(id) => state.swayward.layout.windows().find_map(|(_, mapped)| {
            (mapped.id() == id)
                .then(|| state.swayward.layout.window_workspace_id(&mapped.window))
                .flatten()
        }),
    }
}

pub(super) fn tiling_target(
    state: &State,
    target: CommandTarget,
    floating_error: &str,
) -> Result<
    (
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
    CommandOutcome,
> {
    match target {
        CommandTarget::Container(workspace, node) => Ok((workspace, node)),
        CommandTarget::Window(window) => {
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == window).then(|| mapped.window.clone()))
                .ok_or_else(|| failure("No matching node."))?;
            state
                .swayward
                .layout
                .tiling_target_for_window(&window)
                .ok_or_else(|| failure(floating_error))
        }
    }
}

pub(super) fn focused_target(state: &State) -> Option<CommandTarget> {
    let workspace = state.swayward.layout.active_workspace()?;
    if let Some(node) = workspace
        .focused_container_node()
        .filter(|node| workspace.is_tiling_split(*node))
    {
        return Some(CommandTarget::Container(workspace.id(), node));
    }
    focused_id(state).map(CommandTarget::Window)
}

pub(super) fn mark_target(
    state: &mut State,
    target: CommandTarget,
    mark: &str,
    add: bool,
    toggle: bool,
) {
    let had_mark = match target {
        CommandTarget::Window(window) => state
            .swayward
            .marks_by_window
            .get(&window)
            .is_some_and(|marks| marks.iter().any(|existing| existing == mark)),
        CommandTarget::Container(workspace, node) => state
            .swayward
            .marks_by_container
            .get(&(workspace, node))
            .is_some_and(|marks| marks.iter().any(|existing| existing == mark)),
    };
    if !add {
        let node_id = match target {
            CommandTarget::Window(window) => crate::ipc::tree::window_id(window),
            CommandTarget::Container(_, node) => crate::ipc::tree::container_id(node),
        };
        let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
            &state.swayward.layout,
            &state.swayward.global_space,
            &state.swayward.marks_by_window,
            &state.swayward.marks_by_container,
        ))
        .unwrap_or_default();
        let container = crate::ipc::server::find_node_by_id(&tree, node_id).cloned();
        unmark_target(state, target, None);
        if let (Some(server), Some(mut container)) = (&state.swayward.ipc_server, container) {
            container["marks"] = serde_json::json!([]);
            server.send_event(swayward_ipc::legacy::Event::SwayWindowChanged {
                change: "mark".into(),
                container,
            });
        }
    }
    unmark_globally(state, Some(mark));
    if !toggle || !had_mark {
        match target {
            CommandTarget::Window(window) => state.swayward.set_mark(window, mark, true, false),
            CommandTarget::Container(workspace, node) => state
                .swayward
                .marks_by_container
                .entry((workspace, node))
                .or_default()
                .push(mark.to_owned()),
        }
    }
    if let CommandTarget::Container(_, node) = target {
        let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
            &state.swayward.layout,
            &state.swayward.global_space,
            &state.swayward.marks_by_window,
            &state.swayward.marks_by_container,
        ))
        .unwrap_or_default();
        if let (Some(server), Some(container)) = (
            &state.swayward.ipc_server,
            crate::ipc::server::find_node_by_id(&tree, crate::ipc::tree::container_id(node)),
        ) {
            server.send_event(swayward_ipc::legacy::Event::SwayWindowChanged {
                change: "mark".into(),
                container: container.clone(),
            });
        }
    }
    refresh_titlebar_marks(state);
    if let CommandTarget::Window(window) = target {
        // Sway rechecks only command criteria for the marked view here;
        // `view_execute_criteria` skips rules that this view already ran.
        run_for_window(state, window);
    }
}

pub(super) fn unmark_globally(state: &mut State, mark: Option<&str>) {
    state.swayward.unmark(None, mark);
    if let Some(mark) = mark {
        for marks in state.swayward.marks_by_container.values_mut() {
            marks.retain(|existing| existing != mark);
        }
    } else {
        state.swayward.marks_by_container.clear();
    }
}

fn refresh_titlebar_marks(state: &mut State) {
    let marks = state.swayward.marks_by_window.clone();
    state.swayward.layout.with_windows_mut(|mapped, _| {
        mapped.set_titlebar_marks(marks.get(&mapped.id()).cloned().unwrap_or_default());
    });
}

pub(super) fn unmark_target(state: &mut State, target: CommandTarget, mark: Option<&str>) {
    match target {
        CommandTarget::Window(window) => state.swayward.unmark(Some(window), mark),
        CommandTarget::Container(workspace, node) => {
            if let Some(mark) = mark {
                if let Some(marks) = state
                    .swayward
                    .marks_by_container
                    .get_mut(&(workspace, node))
                {
                    marks.retain(|existing| existing != mark);
                }
            } else {
                state.swayward.marks_by_container.remove(&(workspace, node));
            }
        }
    }
    refresh_titlebar_marks(state);
}

pub(super) fn set_shortcuts_inhibitor(
    state: &mut State,
    target: CommandTarget,
    enable: bool,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("Only views can have shortcuts inhibitors"));
    };
    let mut surface = None;
    state.swayward.layout.with_windows_mut(|window, _| {
        if window.id() == target {
            window.set_shortcuts_inhibit_policy(if enable {
                ShortcutsInhibitPolicy::Enable
            } else {
                ShortcutsInhibitPolicy::Disable
            });
            surface = Some(window.toplevel().wl_surface().clone());
        }
    });
    let surface = surface.ok_or_else(|| failure("No matching node."))?;
    if !enable {
        if let Some(inhibitor) = state
            .swayward
            .keyboard_shortcuts_inhibiting_surfaces
            .get(&surface)
        {
            inhibitor.inactivate();
        }
    }
    Ok(())
}

fn focused_id(state: &State) -> Option<crate::window::mapped::MappedId> {
    state.swayward.layout.focus().map(|mapped| mapped.id())
}

pub(super) fn focused_con_id(state: &State) -> Option<u64> {
    match focused_target(state)? {
        CommandTarget::Container(_, node) => Some(crate::ipc::tree::container_id(node) as u64),
        CommandTarget::Window(window) => Some(crate::ipc::tree::window_id(window) as u64),
    }
}

type WindowSnapshot = (
    crate::window::mapped::MappedId,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    Option<Duration>,
    Option<i32>,
    Option<crate::swayward::SecurityContextMetadata>,
    Option<std::sync::Arc<str>>,
);

fn snapshot_info<'a>(state: &'a State, snapshot: &'a WindowSnapshot) -> criteria::WindowInfo<'a> {
    criteria::WindowInfo {
        title: snapshot.1.as_deref(),
        shell: Some("xdg_shell"),
        app_id: snapshot.2.as_deref(),
        marks: state
            .swayward
            .marks_by_window
            .get(&snapshot.0)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        con_id: crate::ipc::tree::window_id(snapshot.0) as u64,
        floating: snapshot.4,
        urgent_since: snapshot.5,
        workspace: snapshot.3.as_deref(),
        pid: snapshot.6.and_then(|pid| u32::try_from(pid).ok()),
        sandbox_engine: snapshot
            .7
            .as_ref()
            .and_then(|context| context.sandbox_engine.as_deref()),
        sandbox_app_id: snapshot
            .7
            .as_ref()
            .and_then(|context| context.app_id.as_deref()),
        sandbox_instance_id: snapshot
            .7
            .as_ref()
            .and_then(|context| context.instance_id.as_deref()),
        tag: snapshot.8.as_deref(),
    }
}

pub(super) fn matching_targets(state: &State, criteria: &criteria::Criteria) -> Vec<CommandTarget> {
    use crate::utils::with_toplevel_role;

    let focused_id = focused_id(state);
    let mut snapshots = Vec::new();
    state
        .swayward
        .layout
        .with_windows(|mapped, _, workspace_id, _| {
            let (title, app_id) = with_toplevel_role(mapped.toplevel(), |role| {
                (role.title.clone(), role.app_id.clone())
            });
            let workspace = workspace_id.and_then(|id| {
                state
                    .swayward
                    .layout
                    .workspaces()
                    .find_map(|(_, _, ws)| (ws.id() == id).then(|| ws.sway_name()).flatten())
            });
            snapshots.push((
                mapped.id(),
                title,
                app_id,
                workspace,
                mapped.is_floating(),
                mapped.urgent_since(),
                mapped.credentials().map(|c| c.pid),
                mapped.security_context().cloned(),
                mapped.tag(),
            ));
        });
    let focused = snapshots
        .iter()
        .find(|snapshot| Some(snapshot.0) == focused_id);
    let focused_info = focused
        .map(|snapshot| snapshot_info(state, snapshot))
        .unwrap_or_default();
    let mut targets = snapshots
        .iter()
        .filter(|snapshot| criteria.matches(&snapshot_info(state, snapshot), &focused_info))
        .map(|snapshot| CommandTarget::Window(snapshot.0))
        .collect::<Vec<_>>();
    if let Some(order) = criteria.urgent() {
        targets.sort_by_key(|target| {
            let CommandTarget::Window(id) = target else {
                return None;
            };
            snapshots
                .iter()
                .find(|snapshot| snapshot.0 == *id)
                .and_then(|snapshot| snapshot.5)
        });
        if matches!(order, criteria::Urgent::Latest) {
            targets.reverse();
        }
        targets.truncate(1);
    }
    for (_, _, workspace) in state.swayward.layout.workspaces() {
        let trees = std::iter::once((workspace.ipc_tiling_tree(), false)).chain(
            workspace
                .ipc_floating_trees()
                .map(|(_, tree, _)| (tree, true)),
        );
        for (tree, floating) in trees {
            for (node, value) in tree.nodes() {
                if matches!(value, crate::layout::tiling_tree::IpcNodeKind::Leaf) {
                    if floating {
                        let window = tree.window_for_node(node).unwrap();
                        let mapped = workspace
                            .windows()
                            .find(|mapped| mapped.window == *window)
                            .unwrap();
                        let (title, app_id) = with_toplevel_role(mapped.toplevel(), |role| {
                            (role.title.clone(), role.app_id.clone())
                        });
                        let snapshot = (
                            mapped.id(),
                            title,
                            app_id,
                            workspace.sway_name(),
                            true,
                            mapped.urgent_since(),
                            mapped.credentials().map(|c| c.pid),
                            mapped.security_context().cloned(),
                            mapped.tag(),
                        );
                        if criteria.matches(&snapshot_info(state, &snapshot), &focused_info)
                            && !targets.contains(&CommandTarget::Window(mapped.id()))
                        {
                            targets.push(CommandTarget::Window(mapped.id()));
                        }
                    }
                } else {
                    let marks = state
                        .swayward
                        .marks_by_container
                        .get(&(workspace.id(), node))
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    if criteria
                        .matches_container(crate::ipc::tree::container_id(node) as u64, marks)
                    {
                        targets.push(CommandTarget::Container(workspace.id(), node));
                    }
                }
            }
        }
    }
    targets
}

fn matching_ids(
    state: &State,
    criteria: &criteria::Criteria,
) -> Vec<crate::window::mapped::MappedId> {
    matching_targets(state, criteria)
        .into_iter()
        .filter_map(|target| match target {
            CommandTarget::Window(id) => Some(id),
            CommandTarget::Container(_, _) => None,
        })
        .collect()
}

/// Re-resolve and re-run the window rules for one window.
///
/// Called when a window's float state changes, so criteria that test that state
/// see the new value.
fn rerun_for_window_rules(state: &mut State, id: crate::window::mapped::MappedId) {
    let commands = {
        let config = state.swayward.config.borrow();
        let rules = &config.window_rules;
        state
            .swayward
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == id)
            .map(|(_, mapped)| {
                crate::window::ResolvedWindowRules::compute(
                    rules,
                    crate::window::WindowRef::Mapped(mapped),
                    false,
                )
                .sway_for_window_commands
            })
            .unwrap_or_default()
    };
    for command in commands {
        let targeted = format!("[con_id={}] {command}", crate::ipc::tree::window_id(id));
        let _ = execute(state, &targeted);
    }
}

/// Execute newly matching runtime `for_window` criteria once for this window.
///
/// Sway records a criterion before executing its command, which also prevents a
/// mark-producing rule from recursively executing itself.
pub fn run_for_window(state: &mut State, id: crate::window::mapped::MappedId) {
    let commands = state
        .swayward
        .for_window
        .iter()
        .filter_map(|(raw, command, criteria)| {
            let key = (id, raw.clone(), command.clone());
            (!state.swayward.executed_for_window.contains(&key)
                && matching_ids(state, criteria).contains(&id))
            .then_some(key)
        })
        .collect::<Vec<_>>();
    for (id, raw, command) in commands {
        let targeted = format!("[con_id={}] {command}", crate::ipc::tree::window_id(id));
        state
            .swayward
            .executed_for_window
            .insert((id, raw, command));
        let _ = execute(state, &targeted);
    }
}

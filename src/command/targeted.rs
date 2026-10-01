use std::time::Duration;

use swayward_ipc::{criteria, CommandOutcome};

use super::movement::{
    move_position, move_target_to_mark, move_target_to_workspace, move_tiling_subtree_to_output,
    move_workspace_to_output, output_target,
};
use super::{
    execute, failure, focus, layout, movement, scratchpad, success, window, ClientColorClass,
    Command, CommandTarget, Direction, OutputTarget,
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
        move_focused_direction(state, target, direction, layout_direction, activate)
    } else {
        match move_targeted_direction(state, target, direction, layout_direction, pixels, activate)
        {
            Ok(moved) => moved,
            Err(outcome) => return outcome,
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

/// `move <direction>` on the focused container: tiled moves follow the
/// focus, and a move that leaves the workspace edge crosses outputs.
fn move_focused_direction(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
    layout_direction: crate::layout::tiling_tree::Direction,
    activate: crate::layout::ActivateWindow,
) -> bool {
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
}

/// `move <direction>` on a criteria target.
fn move_targeted_direction(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
    layout_direction: crate::layout::tiling_tree::Direction,
    pixels: Option<i32>,
    activate: crate::layout::ActivateWindow,
) -> Result<bool, CommandOutcome> {
    let target = match target {
        CommandTarget::Window(target) => target,
        CommandTarget::Container(workspace, node) => {
            return Ok(state.swayward.layout.move_tiling_node_in_direction(
                workspace,
                node,
                layout_direction,
            ));
        }
    };
    let window = state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
    let Some(window) = window else {
        return Err(failure("No matching node."));
    };
    let moved = state.swayward.layout.move_window_in_direction(
        &window,
        layout_direction,
        f64::from(pixels.unwrap_or(10)),
    );
    if !moved {
        move_target_to_adjacent_output(state, CommandTarget::Window(target), direction, activate);
    }
    Ok(moved)
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

fn run_targeted(
    state: &mut State,
    command: &Command,
    target: CommandTarget,
) -> super::HandlerResult {
    match command {
        Command::Mark {
            add,
            toggle,
            identifier,
        } => {
            mark_target(state, target, identifier, *add, *toggle);
            Ok(None)
        }
        Command::Unmark(identifier) => {
            unmark_target(state, target, identifier.as_deref());
            Ok(None)
        }
        Command::Swap(swap_target) => {
            super::handled_outcome(movement::swap_target(state, target, swap_target))
        }
        Command::MoveDirection { direction, pixels } => super::handled_outcome(move_direction(
            state,
            target,
            *direction,
            *pixels,
            crate::layout::ActivateWindow::No,
            false,
        )),
        Command::MovePosition(position) => {
            move_position(state, Some(target), position).map_err(failure)?;
            state.swayward.queue_redraw_all();
            Ok(None)
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
            super::handled_outcome(move_target_to_workspace(
                state,
                target,
                workspace_target.clone(),
                true,
                auto_back_and_forth,
            ))
        }
        Command::MoveToMark(mark) => {
            super::handled_outcome(move_target_to_mark(state, target, mark))
        }
        Command::MoveWorkspaceToOutput(output) => {
            super::handled_outcome(move_workspace_to_output(state, Some(target), output))
        }
        Command::MoveToOutput(output) => movement::to_output_targeted(state, target, output),
        Command::MoveScratchpad => super::handled(scratchpad::move_targeted(state, target)),
        Command::ScratchpadShow => super::handled(scratchpad::show_targeted(state, target)),
        Command::Fullscreen { mode, global } => {
            super::handled(layout::fullscreen_targeted(state, target, *mode, *global))
        }
        Command::ShortcutsInhibitor(enable) => {
            super::handled(set_shortcuts_inhibitor(state, target, *enable))
        }
        Command::Sticky(value) => super::handled(window::sticky(state, target, value)),
        Command::SetClientColors { class, colors } => {
            set_client_colors(state, *class, *colors);
            Ok(None)
        }
        Command::SetLayoutOption(_) => Err(failure("command cannot be applied to a container")),
        Command::Opacity(value) => super::handled(window::opacity(state, target, *value, false)),
        Command::OpacityRelative(value) => {
            super::handled(window::opacity(state, target, *value, true))
        }
        Command::TitleFormat(format) => super::handled(window::title_format(state, target, format)),
        Command::Border(border) => super::handled(window::border(state, target, border)),
        Command::Floating(mode) => super::handled(window::floating(state, target, mode)),
        Command::Urgent(value) => super::handled(window::urgent(state, target, value)),
        Command::Kill => super::handled(window::kill(state, target)),
        Command::ResizeSet { width, height } => {
            super::handled(window::resize_set(state, target, *width, *height))
        }
        Command::Resize {
            grow,
            axis,
            first,
            second,
        } => super::handled(window::resize(state, target, *grow, *axis, *first, *second)),
        Command::Focus => super::handled(focus::targeted(state, target)),
        Command::FocusWorkspace => super::handled(focus::targeted_workspace(state, target)),
        Command::FocusDirection(direction) => {
            super::handled(focus::targeted_direction(state, target, *direction))
        }
        Command::FocusOutput(identifier) => super::handled(focus::output(state, identifier)),
        Command::Layout(value) => super::handled(layout::targeted(state, target, *value)),
        Command::LayoutToggle(toggle) => {
            super::handled(layout::toggle_targeted(state, target, toggle))
        }
        Command::LayoutDefault => super::handled(layout::default_targeted(state, target)),
        Command::Split(value) => super::handled(layout::split_targeted(state, target, *value)),
        Command::RenameWorkspace { old, new_name } => {
            super::workspace::rename_targeted(state, target, old.as_ref(), new_name)
        }
        Command::Nop => Ok(None),
        Command::FocusParent
        | Command::FocusChild
        | Command::FocusNext
        | Command::FocusPrev
        | Command::FocusNextSibling
        | Command::FocusPrevSibling
        | Command::FocusFloating
        | Command::FocusTiling
        | Command::FocusModeToggle
        | Command::Workspace { .. }
        | Command::AssignWorkspace { .. }
        | Command::Reload
        | Command::Exit
        | Command::CreateOutput
        | Command::InputSwitchLayout { .. }
        | Command::Output { .. }
        | Command::Gaps { .. }
        | Command::GapsDefaults { .. }
        | Command::WorkspaceGaps { .. }
        | Command::Mode { .. }
        | Command::Set { .. }
        | Command::Bind { .. }
        | Command::SwitchBind { .. }
        | Command::Exec { .. }
        | Command::Assign { .. }
        | Command::NoFocus { .. }
        | Command::ForWindow { .. } => Err(failure(
            "criteria targets are not implemented for this command yet",
        )),
    }
}

pub(super) fn execute_targeted(
    state: &mut State,
    command: &Command,
    target: CommandTarget,
) -> CommandOutcome {
    match run_targeted(state, command, target) {
        Ok(_) => {
            state.ipc_refresh_layout();
            success()
        }
        Err(outcome) => outcome,
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
            let window =
                super::mapped_window(state, window).ok_or_else(|| failure("No matching node."))?;
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
        CommandTarget::Container(_, node) => state
            .swayward
            .marks_by_container
            .get(&node)
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
            CommandTarget::Container(_, node) => state
                .swayward
                .marks_by_container
                .entry(node)
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
        CommandTarget::Container(_, node) => {
            if let Some(mark) = mark {
                if let Some(marks) = state.swayward.marks_by_container.get_mut(&node) {
                    marks.retain(|existing| existing != mark);
                }
            } else {
                state.swayward.marks_by_container.remove(&node);
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

/// The criteria-visible state of one window, copied out so matching does not
/// hold a layout borrow.
struct WindowSnapshot {
    id: crate::window::mapped::MappedId,
    title: Option<String>,
    app_id: Option<String>,
    workspace: Option<String>,
    floating: bool,
    urgent_since: Option<Duration>,
    pid: Option<i32>,
    security: Option<crate::swayward::SecurityContextMetadata>,
    tag: Option<std::sync::Arc<str>>,
}

impl WindowSnapshot {
    fn new(mapped: &crate::window::Mapped, workspace: Option<String>, floating: bool) -> Self {
        let (title, app_id) = crate::utils::with_toplevel_role(mapped.toplevel(), |role| {
            (role.title.clone(), role.app_id.clone())
        });
        Self {
            id: mapped.id(),
            title,
            app_id,
            workspace,
            floating,
            urgent_since: mapped.urgent_since(),
            pid: mapped.credentials().map(|c| c.pid),
            security: mapped.security_context().cloned(),
            tag: mapped.tag(),
        }
    }

    fn info<'a>(&'a self, state: &'a State) -> criteria::WindowInfo<'a> {
        let security = self.security.as_ref();
        criteria::WindowInfo {
            title: self.title.as_deref(),
            shell: Some("xdg_shell"),
            app_id: self.app_id.as_deref(),
            marks: state
                .swayward
                .marks_by_window
                .get(&self.id)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            con_id: crate::ipc::tree::window_id(self.id) as u64,
            floating: self.floating,
            urgent_since: self.urgent_since,
            workspace: self.workspace.as_deref(),
            pid: self.pid.and_then(|pid| u32::try_from(pid).ok()),
            sandbox_engine: security.and_then(|context| context.sandbox_engine.as_deref()),
            sandbox_app_id: security.and_then(|context| context.app_id.as_deref()),
            sandbox_instance_id: security.and_then(|context| context.instance_id.as_deref()),
            tag: self.tag.as_deref(),
        }
    }
}

pub(super) fn matching_targets(state: &State, criteria: &criteria::Criteria) -> Vec<CommandTarget> {
    let workspace_names = state
        .swayward
        .layout
        .workspaces()
        .map(|(_, _, ws)| (ws.id(), ws.sway_name()))
        .collect::<std::collections::HashMap<_, _>>();
    let focused_id = focused_id(state);
    let mut snapshots = Vec::new();
    state
        .swayward
        .layout
        .with_windows(|mapped, _, workspace_id, _| {
            let workspace = workspace_id
                .and_then(|id| workspace_names.get(&id).cloned())
                .flatten();
            snapshots.push(WindowSnapshot::new(mapped, workspace, mapped.is_floating()));
        });
    let focused = snapshots
        .iter()
        .find(|snapshot| Some(snapshot.id) == focused_id);
    let focused_info = focused
        .map(|snapshot| snapshot.info(state))
        .unwrap_or_default();
    let mut targets = snapshots
        .iter()
        .filter(|snapshot| criteria.matches(&snapshot.info(state), &focused_info))
        .map(|snapshot| CommandTarget::Window(snapshot.id))
        .collect::<Vec<_>>();
    if let Some(order) = criteria.urgent() {
        targets.sort_by_key(|target| {
            let CommandTarget::Window(id) = target else {
                return None;
            };
            snapshots
                .iter()
                .find(|snapshot| snapshot.id == *id)
                .and_then(|snapshot| snapshot.urgent_since)
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
                        // The snapshot and the window list come from the same
                        // workspace borrow, so both lookups succeed. A leaf that
                        // did not resolve would be unmatchable, never a reason
                        // to take the compositor down on a criteria command.
                        let Some(mapped) = tree.window_for_node(node).and_then(|window| {
                            workspace.windows().find(|mapped| mapped.window == *window)
                        }) else {
                            warn!("criteria: floating leaf {node:?} has no mapped window");
                            continue;
                        };
                        let snapshot = WindowSnapshot::new(mapped, workspace.sway_name(), true);
                        if criteria.matches(&snapshot.info(state), &focused_info)
                            && !targets.contains(&CommandTarget::Window(mapped.id()))
                        {
                            targets.push(CommandTarget::Window(mapped.id()));
                        }
                    }
                } else {
                    let marks = state
                        .swayward
                        .marks_by_container
                        .get(&node)
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
    // A hidden scratchpad group's containers stay matchable, as in sway's
    // criteria walk (`sway/sway/tree/root.c:250-257`). The group has no
    // workspace, so a match targets the group through one of its windows;
    // the scratchpad commands act on the whole group from any of them.
    for (node, window) in state.swayward.layout.scratchpad_tree_nodes() {
        let marks = state
            .swayward
            .marks_by_container
            .get(&node)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if criteria.matches_container(crate::ipc::tree::container_id(node) as u64, marks) {
            if let Some(mapped) = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.window == *window).then(|| mapped.id()))
            {
                let target = CommandTarget::Window(mapped);
                if !targets.contains(&target) {
                    targets.push(target);
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

pub(super) fn mark_focused(
    state: &mut State,
    add: bool,
    toggle: bool,
    identifier: &str,
) -> super::HandlerResult {
    if state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| workspace.is_workspace_focused())
    {
        return Err(swayward_ipc::command::parse_error(
            "Only containers can have marks",
        ));
    }
    let Some(target) = focused_target(state) else {
        return Err(swayward_ipc::command::parse_error(
            "Only containers can have marks",
        ));
    };
    mark_target(state, target, identifier, add, toggle);
    Ok(None)
}

pub(super) fn unmark_focused(state: &mut State, identifier: Option<&str>) -> super::HandlerResult {
    unmark_globally(state, identifier);
    Ok(None)
}

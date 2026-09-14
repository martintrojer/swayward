use swayward_config::Action;
use swayward_ipc::command::parse_error;
pub use swayward_ipc::command::{
    parse, parse_boolean, BorderStyle, Command, Direction, Layout, LayoutToggle, LayoutToggleEntry,
    MovePosition, OutputTarget, ParsedCommand, ResizeAmount, ResizeAxis, ResizeUnit, SwapTarget,
    Toggle, WorkspaceTarget,
};
use swayward_ipc::legacy::{PositionChange, SizeChange};
use swayward_ipc::{criteria, CommandOutcome};

use crate::swayward::State;
use crate::utils::spawning::spawn_sh;

mod focus;
mod layout;
mod movement;
mod scratchpad;
mod window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandTarget {
    Window(crate::window::mapped::MappedId),
    Container(
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
}

pub fn execute(state: &mut State, input: &str) -> Vec<CommandOutcome> {
    let mut parsed = parse(input);
    for item in &mut parsed {
        if let Ok(command) = item {
            if let Some(raw) = &command.criteria {
                if let Err(error) = criteria::Criteria::parse(raw, focused_con_id(state)) {
                    *item = Err(parse_error(error));
                }
            }
        }
    }
    let mut retained_targets = None;
    parsed
        .into_iter()
        .map(|parsed| match parsed {
            Ok(parsed) => {
                if parsed.criteria_start {
                    retained_targets = None;
                }
                execute_one(state, parsed, &mut retained_targets)
            }
            Err(error) => error,
        })
        .collect()
}

fn execute_one(
    state: &mut State,
    parsed: ParsedCommand,
    retained_targets: &mut Option<Vec<CommandTarget>>,
) -> CommandOutcome {
    let targets = match parsed.criteria.as_deref() {
        Some(raw) => match criteria::Criteria::parse(raw, focused_con_id(state)) {
            Ok(criteria) => retained_targets
                .get_or_insert_with(|| matching_targets(state, &criteria))
                .clone(),
            Err(error) => return failure(error),
        },
        None => Vec::new(),
    };
    if parsed.criteria.is_some() {
        if targets.is_empty() {
            return failure("No matching node.");
        }
        if let Command::Unmark(identifier) = &parsed.command {
            for target in targets {
                unmark_target(state, target, identifier.as_deref());
            }
            return success();
        }
        for target in targets {
            let outcome = execute_targeted(state, &parsed.command, target);
            if !outcome.success {
                return outcome;
            }
        }
        return success();
    }

    if let Command::Mark {
        add,
        toggle,
        identifier,
    } = &parsed.command
    {
        let Some(target) = focused_target(state) else {
            unmark_globally(state, Some(identifier));
            return success();
        };
        mark_target(state, target, identifier, *add, *toggle);
        return success();
    }

    let action = match parsed.command {
        Command::Swap(target) => {
            let Some(source) = focused_target(state) else {
                return failure("Can only swap with containers and views");
            };
            let outcome = movement::swap_target(state, source, &target);
            if !outcome.success {
                return outcome;
            }
            None
        }
        Command::Focus => None,
        Command::FocusWorkspace => return failure("No container to focus was specified."),
        Command::FocusDirection(direction) => focus::direction(state, direction),
        Command::FocusOutput(identifier) => {
            if let Err(error) = focus::output(state, &identifier) {
                return error;
            }
            None
        }
        Command::FocusParent => {
            focus::parent(state);
            None
        }
        Command::FocusChild => {
            focus::child(state);
            None
        }
        Command::FocusNext => Some(Action::FocusColumnRightOrFirst),
        Command::FocusPrev => Some(Action::FocusColumnLeftOrLast),
        Command::FocusFloating => Some(focus::floating(state)),
        Command::FocusTiling => Some(focus::tiling(state)),
        Command::FocusModeToggle => Some(Action::SwitchFocusBetweenFloatingAndTiling),
        Command::MoveDirection { direction, pixels } => {
            let Some(workspace) = state.swayward.layout.active_workspace() else {
                return failure("Cannot move workspaces in a direction");
            };
            let fullscreen_floating = workspace.active_floating_is_fullscreen();
            let floating = workspace.floating_is_active() || fullscreen_floating;
            if floating {
                if fullscreen_floating {
                    return failure("Cannot move fullscreen floating container");
                }
                let pixels = f64::from(pixels.unwrap_or(10));
                let (x, y) = match direction {
                    Direction::Left => (-pixels, 0.),
                    Direction::Right => (pixels, 0.),
                    Direction::Up => (0., -pixels),
                    Direction::Down => (0., pixels),
                };
                state.swayward.layout.move_floating_window(
                    None,
                    PositionChange::AdjustFixed(x),
                    PositionChange::AdjustFixed(y),
                    true,
                );
                state.swayward.queue_redraw_all();
                None
            } else {
                Some(match direction {
                    Direction::Left => Action::MoveColumnLeft,
                    Direction::Right => Action::MoveColumnRight,
                    Direction::Up => Action::MoveWindowUp,
                    Direction::Down => Action::MoveWindowDown,
                })
            }
        }
        Command::MovePosition(position) => {
            if let Err(error) = move_position(state, None, &position) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::MoveToWorkspace(target) => {
            if state.swayward.layout.global_fullscreen_active()
                && state
                    .swayward
                    .layout
                    .focused_window_is_fullscreen_or_child()
            {
                return failure("Can't move fullscreen global container");
            }
            let Some(focused) = focused_target(state) else {
                return success();
            };
            let outcome = move_target_to_workspace(state, focused, target, false);
            if !outcome.success {
                return outcome;
            }
            None
        }
        Command::MoveToMark(mark) => {
            let Some(source) = focused_target(state) else {
                return success();
            };
            let outcome = move_target_to_mark(state, source, &mark);
            if !outcome.success {
                return outcome;
            }
            None
        }
        Command::MoveToOutput(target) => {
            let focused = state
                .swayward
                .layout
                .focus_with_output()
                .map(|(window, output)| (window.window.clone(), output.clone()));
            let reference = focused
                .as_ref()
                .and_then(|(window, _)| state.swayward.layout.window_center(window));
            let reference_output = focused.as_ref().map(|(_, output)| output);
            let output = match output_target(state, &target, reference_output, reference) {
                Ok(output) => output,
                Err(error) => return failure(error),
            };
            state.swayward.layout.move_to_output(
                None,
                &output,
                None,
                crate::layout::ActivateWindow::Smart,
            );
            state.swayward.queue_redraw_all();
            None
        }
        Command::MoveWorkspaceToOutput(target) => {
            let output = match output_target(state, &target, None, None) {
                Ok(output) => output,
                Err(error) => return failure(error),
            };
            state.swayward.layout.move_workspace_to_output(&output);
            state.swayward.queue_redraw_all();
            None
        }
        Command::MoveScratchpad => {
            scratchpad::move_focused(state);
            None
        }
        Command::ScratchpadShow => {
            scratchpad::show(state);
            None
        }
        Command::LayoutDefault => {
            if let Err(error) = layout::default(state) {
                return error;
            }
            None
        }
        Command::LayoutToggle(cycle) => {
            if let Err(error) = layout::toggle(state, &cycle) {
                return error;
            }
            None
        }
        Command::Layout(value) => {
            if let Err(error) = layout::set(state, value) {
                return error;
            }
            None
        }
        Command::Split(value) => {
            if let Err(error) = layout::split(state, value) {
                return error;
            }
            None
        }
        Command::Fullscreen { mode, global } => {
            layout::fullscreen(state, mode, global);
            None
        }
        Command::Sticky(value) => {
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
                return failure("No current container");
            };
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return success();
            }
            if !state.swayward.layout.set_window_sticky(&window, &value) {
                return failure("Expected output to have a workspace");
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Border(border) => {
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
                return failure("Only views can have borders");
            };
            if let Err(error) =
                state
                    .swayward
                    .layout
                    .set_window_border(&window, border.style, border.width)
            {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Floating(mode) => {
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
                return success();
            };
            match mode {
                Toggle::Enable => state
                    .swayward
                    .layout
                    .set_window_floating(Some(&window), true),
                Toggle::Disable => state
                    .swayward
                    .layout
                    .set_window_floating(Some(&window), false),
                Toggle::Toggle => state.swayward.layout.toggle_window_floating(Some(&window)),
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Workspace {
            target,
            auto_back_and_forth,
        } => {
            let auto_back_and_forth = auto_back_and_forth
                && state
                    .swayward
                    .config
                    .borrow()
                    .input
                    .workspace_auto_back_and_forth;
            let result = if auto_back_and_forth {
                state
                    .swayward
                    .layout
                    .activate_sway_workspace_auto_back_and_forth(target)
            } else {
                state.swayward.layout.activate_sway_workspace(target)
            };
            if let Err(error) = result {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::AssignWorkspace { target, output } => {
            if let Err(error) = state.swayward.layout.assign_sway_workspace(target, &output) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::RenameWorkspace { old, new_name } => {
            if let Err(error) = state.swayward.layout.rename_sway_workspace(old, new_name) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Kill => {
            let windows = state
                .swayward
                .layout
                .active_workspace()
                .and_then(|workspace| {
                    workspace.is_workspace_focused().then(|| {
                        workspace
                            .windows()
                            .map(|window| window.id().get())
                            .collect::<Vec<_>>()
                    })
                });
            if let Some(windows) = windows {
                for window in windows {
                    state.do_action(Action::CloseWindowById(window), false);
                }
                None
            } else {
                Some(Action::CloseWindow)
            }
        }
        Command::Resize {
            grow,
            axis,
            first,
            second,
        } => {
            let floating = state
                .swayward
                .layout
                .focus()
                .is_some_and(|mapped| mapped.is_floating());
            let selected = select_resize_amount(first, second, floating);
            let sign = if grow { 1 } else { -1 };
            let amount = selected.amount.saturating_mul(sign);
            let change = match selected.unit {
                ResizeUnit::Default if floating => SizeChange::AdjustFixed(amount),
                ResizeUnit::Pixels => SizeChange::AdjustFixed(amount),
                ResizeUnit::Default | ResizeUnit::PercentagePoints => {
                    SizeChange::AdjustProportion(f64::from(amount))
                }
            };
            match axis {
                ResizeAxis::Width => Some(Action::SetWindowWidth(change)),
                ResizeAxis::Height => Some(Action::SetWindowHeight(change)),
                direction => {
                    let edge = match direction {
                        ResizeAxis::Up => crate::utils::ResizeEdge::TOP,
                        ResizeAxis::Down => crate::utils::ResizeEdge::BOTTOM,
                        ResizeAxis::Left => crate::utils::ResizeEdge::LEFT,
                        ResizeAxis::Right => crate::utils::ResizeEdge::RIGHT,
                        ResizeAxis::Width | ResizeAxis::Height => unreachable!(),
                    };
                    state.swayward.layout.resize_window_edge(None, edge, change);
                    None
                }
            }
        }
        Command::Reload => {
            let Some(watcher) = &state.swayward.config_file_watcher else {
                return failure("config reload is not available without a config file watcher");
            };
            watcher.load_config(None);
            None
        }
        Command::Mode(mode) => {
            if mode != "default"
                && !state
                    .swayward
                    .config
                    .borrow()
                    .binding_modes
                    .iter()
                    .any(|binding_mode| binding_mode.name == mode)
            {
                return failure(format!("Unknown binding mode '{mode}'"));
            }
            state.swayward.binding_mode = mode.clone();
            if let Some(server) = &state.swayward.ipc_server {
                server.send_event(swayward_ipc::legacy::Event::BindingModeChanged {
                    mode,
                    pango_markup: false,
                });
            }
            None
        }
        Command::Nop => None,
        Command::Exec(command) => {
            let (token, _) = state.swayward.activation_state.create_external_token(None);
            spawn_sh(command, Some(token.clone()));
            None
        }
        Command::Mark { .. } => unreachable!(),
        Command::Unmark(identifier) => {
            if parsed.criteria.is_some() {
                for target in targets {
                    unmark_target(state, target, identifier.as_deref());
                }
            } else {
                unmark_globally(state, identifier.as_deref());
            }
            None
        }
        Command::ForWindow { criteria, command } => {
            let parsed = match criteria::Criteria::parse(&criteria, focused_con_id(state)) {
                Ok(criteria) => criteria,
                Err(error) => return failure(error),
            };
            if !state
                .swayward
                .for_window
                .iter()
                .any(|(raw, existing, _)| raw == &criteria && existing == &command)
            {
                state.swayward.for_window.push((criteria, command, parsed));
            }
            None
        }
    };

    if let Some(action) = action {
        state.do_action(action, false);
    }
    state.ipc_refresh_layout();
    success()
}

use movement::{
    move_position, move_target_to_mark, move_target_to_workspace, output_target,
    output_target_by_name_or_direction, select_resize_amount,
};

fn execute_targeted(state: &mut State, command: &Command, target: CommandTarget) -> CommandOutcome {
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
            let layout_direction = match direction {
                Direction::Left => crate::layout::tiling_tree::Direction::Left,
                Direction::Right => crate::layout::tiling_tree::Direction::Right,
                Direction::Up => crate::layout::tiling_tree::Direction::Up,
                Direction::Down => crate::layout::tiling_tree::Direction::Down,
            };
            match target {
                CommandTarget::Window(target) => {
                    let window = state.swayward.layout.windows().find_map(|(_, mapped)| {
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
                        let destination = OutputTarget::Direction(*direction);
                        let reference = state
                            .swayward
                            .layout
                            .windows()
                            .find_map(|(monitor, mapped)| {
                                (mapped.id() == target)
                                    .then(|| monitor.map(|monitor| monitor.output()))
                            })
                            .flatten();
                        let reference_point = state.swayward.layout.window_center(&window);
                        if let Ok(output) =
                            output_target(state, &destination, reference, reference_point)
                        {
                            state.swayward.layout.move_to_output(
                                Some(&window),
                                &output,
                                None,
                                crate::layout::ActivateWindow::No,
                            );
                        }
                    }
                }
                CommandTarget::Container(workspace, node) => {
                    state.swayward.layout.move_tiling_node_in_direction(
                        workspace,
                        node,
                        layout_direction,
                    );
                }
            }
            state.swayward.queue_redraw_all();
        }
        Command::MovePosition(position) => {
            let CommandTarget::Window(target) = target else {
                return failure("command requires a window target");
            };
            if let Err(error) = move_position(state, Some(target), position) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
        }
        Command::MoveToWorkspace(workspace_target) => {
            let outcome = move_target_to_workspace(state, target, workspace_target.clone(), true);
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
                crate::layout::ActivateWindow::Smart,
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
            let CommandTarget::Window(target) = target else {
                return failure("command requires a window target");
            };
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
            };
            let current = state.swayward.layout.fullscreen_mode(&window);
            let enabled = match mode {
                Toggle::Enable => true,
                Toggle::Disable => false,
                Toggle::Toggle => current.is_none(),
            };
            state.swayward.layout.set_fullscreen_mode(
                &window,
                enabled.then_some(if *global {
                    crate::layout::tiling_tree::FullscreenMode::Global
                } else {
                    crate::layout::tiling_tree::FullscreenMode::Workspace
                }),
            );
            state.swayward.queue_redraw_all();
        }
        Command::Sticky(value) => {
            if let Err(error) = window::sticky(state, target, value) {
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
        }
        Command::Kill => {
            if let Err(error) = window::kill(state, target) {
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
        Command::Layout(value) => {
            if let Err(error) = layout::targeted(state, target, *value) {
                return error;
            }
        }
        Command::Nop => {}
        _ => return failure("criteria targets are not implemented for this command yet"),
    }
    state.ipc_refresh_layout();
    success()
}

fn focused_target(state: &State) -> Option<CommandTarget> {
    let workspace = state.swayward.layout.active_workspace()?;
    if let Some(node) = workspace
        .focused_container_node()
        .filter(|node| workspace.is_tiling_split(*node))
    {
        return Some(CommandTarget::Container(workspace.id(), node));
    }
    focused_id(state).map(CommandTarget::Window)
}

fn mark_target(state: &mut State, target: CommandTarget, mark: &str, add: bool, toggle: bool) {
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
        unmark_target(state, target, None);
    }
    unmark_globally(state, Some(mark));
    if toggle && had_mark {
        return;
    }
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

fn unmark_globally(state: &mut State, mark: Option<&str>) {
    state.swayward.unmark(None, mark);
    if let Some(mark) = mark {
        for marks in state.swayward.marks_by_container.values_mut() {
            marks.retain(|existing| existing != mark);
        }
    } else {
        state.swayward.marks_by_container.clear();
    }
}

fn unmark_target(state: &mut State, target: CommandTarget, mark: Option<&str>) {
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
}

fn focused_id(state: &State) -> Option<crate::window::mapped::MappedId> {
    state.swayward.layout.focus().map(|mapped| mapped.id())
}

fn focused_con_id(state: &State) -> Option<u64> {
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
    bool,
    Option<i32>,
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
        id: Some(crate::ipc::tree::window_id(snapshot.0) as u64),
        floating: snapshot.4,
        urgent: snapshot.5,
        workspace: snapshot.3.as_deref(),
        pid: snapshot.6.and_then(|pid| u32::try_from(pid).ok()),
        ..Default::default()
    }
}

fn matching_targets(state: &State, criteria: &criteria::Criteria) -> Vec<CommandTarget> {
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
                mapped.is_urgent(),
                mapped.credentials().map(|c| c.pid),
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
    for (_, _, workspace) in state.swayward.layout.workspaces() {
        for (node, value) in workspace.ipc_tiling_tree().nodes() {
            if matches!(value, crate::layout::tiling_tree::IpcNodeKind::Split) {
                let marks = state
                    .swayward
                    .marks_by_container
                    .get(&(workspace.id(), node))
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                if criteria.matches_container(crate::ipc::tree::container_id(node) as u64, marks) {
                    targets.push(CommandTarget::Container(workspace.id(), node));
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

pub fn run_for_window(state: &mut State, id: crate::window::mapped::MappedId) {
    let commands = state
        .swayward
        .for_window
        .iter()
        .filter_map(|(_, command, criteria)| {
            matching_ids(state, criteria)
                .contains(&id)
                .then_some(command.clone())
        })
        .collect::<Vec<_>>();
    for command in commands {
        let targeted = format!("[con_id={}] {command}", crate::ipc::tree::window_id(id));
        let _ = execute(state, &targeted);
    }
}

fn success() -> CommandOutcome {
    CommandOutcome {
        success: true,
        error: None,
        parse_error: None,
    }
}

fn failure(error: impl Into<String>) -> CommandOutcome {
    CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(input: &str) -> Command {
        parse(input).into_iter().next().unwrap().unwrap().command
    }

    #[test]
    fn parses_workspace_rename_forms() {
        assert_eq!(
            command("rename workspace number 5 to 7: web"),
            Command::RenameWorkspace {
                old: Some(WorkspaceTarget::Number("5".into())),
                new_name: "7: web".into(),
            }
        );
        assert_eq!(
            command("rename workspace to mail"),
            Command::RenameWorkspace {
                old: None,
                new_name: "mail".into(),
            }
        );
    }

    #[test]
    fn parses_focus_output_with_multi_word_name() {
        assert_eq!(
            parse("focus output left monitor")[0]
                .as_ref()
                .unwrap()
                .command,
            Command::FocusOutput("left monitor".into())
        );
        assert_eq!(
            parse("focus output")[0]
                .as_ref()
                .unwrap_err()
                .error
                .as_deref(),
            Some("Expected 'focus output <direction|name>'.")
        );
    }

    #[test]
    fn parses_sway_focus_modes() {
        for input in ["focus tiling", "focus floating", "focus mode_toggle"] {
            assert!(parse(input)[0].is_ok(), "{input}");
        }
    }

    #[test]
    fn parses_standalone_split_aliases_with_no_arguments() {
        for (alias, layout) in [
            ("splith", Layout::SplitH),
            ("splitv", Layout::SplitV),
            ("splitt", Layout::ToggleSplit),
        ] {
            assert_eq!(command(alias), Command::Split(Some(layout)));
            assert!(parse(&format!("{alias} extra"))[0].is_err());
        }
    }

    #[test]
    fn parses_sticky_with_exactly_one_argument() {
        assert_eq!(command("sticky enabled"), Command::Sticky("enabled".into()));
        for input in ["sticky", "sticky enable extra"] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some("Expected 'sticky <enable|disable|toggle>'")
            );
        }
    }

    #[test]
    fn move_output_uses_the_first_target_and_ignores_extra_names() {
        assert_eq!(
            command("move window to output fake-1 fake-2"),
            Command::MoveToOutput(OutputTarget::Name("fake-1".into()))
        );
    }

    #[test]
    fn parses_fullscreen_with_sway_boolean_vocabulary() {
        for value in ["1", "yes", "on", "true", "enable", "enabled", "active"] {
            assert_eq!(
                command(&format!("fullscreen {value}")),
                Command::Fullscreen {
                    mode: Toggle::Enable,
                    global: false,
                }
            );
        }
        for value in [
            "0", "no", "off", "false", "disable", "disabled", "inactive", "nope",
        ] {
            assert_eq!(
                command(&format!("fullscreen {value}")),
                Command::Fullscreen {
                    mode: Toggle::Disable,
                    global: false,
                }
            );
        }
        assert_eq!(
            command("fullscreen global"),
            Command::Fullscreen {
                mode: Toggle::Toggle,
                global: true,
            }
        );
        assert_eq!(
            command("fullscreen yes global"),
            Command::Fullscreen {
                mode: Toggle::Enable,
                global: true,
            }
        );
        assert_eq!(
            command("fullscreen toggle nope"),
            Command::Fullscreen {
                mode: Toggle::Toggle,
                global: false,
            }
        );
    }

    #[test]
    fn parses_every_supported_command_family() {
        assert_eq!(command("focus"), Command::Focus);
        assert_eq!(command("focus workspace"), Command::FocusWorkspace);
        assert_eq!(
            command("focus left"),
            Command::FocusDirection(Direction::Left)
        );
        assert_eq!(command("focus parent"), Command::FocusParent);
        assert_eq!(command("focus floating"), Command::FocusFloating);
        assert_eq!(command("focus tiling"), Command::FocusTiling);
        assert_eq!(command("focus mode_toggle"), Command::FocusModeToggle);
        assert_eq!(
            command("move right 12 px"),
            Command::MoveDirection {
                direction: Direction::Right,
                pixels: Some(12)
            }
        );
        assert_eq!(
            command("move to workspace number 3:web"),
            Command::MoveToWorkspace(WorkspaceTarget::Number("3:web".into()))
        );
        assert_eq!(
            command("move window to output left"),
            Command::MoveToOutput(OutputTarget::Direction(Direction::Left))
        );
        assert_eq!(
            command("move container output HDMI-A-1"),
            Command::MoveToOutput(OutputTarget::Name("HDMI-A-1".into()))
        );
        for input in [
            "move mark target",
            "move to mark target",
            "move window mark target",
            "move window to mark target",
            "move container mark target",
            "move container to mark target",
        ] {
            assert_eq!(
                command(input),
                Command::MoveToMark("target".into()),
                "{input}"
            );
        }
        assert_eq!(
            command("move workspace to output right"),
            Command::MoveWorkspaceToOutput(OutputTarget::Direction(Direction::Right))
        );
        assert_eq!(
            command("move workspace output DP-1"),
            Command::MoveWorkspaceToOutput(OutputTarget::Name("DP-1".into()))
        );
        assert_eq!(command("move scratchpad"), Command::MoveScratchpad);
        assert_eq!(command("move to scratchpad"), Command::MoveScratchpad);
        assert_eq!(command("scratchpad show"), Command::ScratchpadShow);
        assert_eq!(
            command("swap container with con_id 42"),
            Command::Swap(SwapTarget::ConId("42".into()))
        );
        for input in [
            "swap",
            "swap window with con_id 42",
            "swap container to con_id 42",
            "swap container with nope 42",
        ] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some("Expected 'swap container with id|con_id|mark <arg>'"),
                "{input}"
            );
        }
        assert_eq!(command("layout stacked"), Command::Layout(Layout::Stacked));
        assert_eq!(command("layout default"), Command::LayoutDefault);
        assert_eq!(
            command("layout toggle split"),
            Command::LayoutToggle(LayoutToggle::Split)
        );
        assert_eq!(
            command("layout toggle"),
            Command::LayoutToggle(LayoutToggle::Default)
        );
        assert_eq!(
            command("layout toggle all"),
            Command::LayoutToggle(LayoutToggle::All)
        );
        assert_eq!(
            command("layout toggle splitv garbage stacking tabbed"),
            Command::LayoutToggle(LayoutToggle::Cycle(vec![
                LayoutToggleEntry::Layout(Layout::SplitV),
                LayoutToggleEntry::Layout(Layout::Stacked),
                LayoutToggleEntry::Layout(Layout::Tabbed),
            ]))
        );
        assert!(parse("layout toggle stacked")[0].is_err());
        assert_eq!(
            command("layout toggle stacking splitv garbage tabbed"),
            Command::LayoutToggle(LayoutToggle::Cycle(vec![
                LayoutToggleEntry::Layout(Layout::Stacked),
                LayoutToggleEntry::Layout(Layout::SplitV),
                LayoutToggleEntry::Layout(Layout::Tabbed),
            ]))
        );
        assert_eq!(command("split none"), Command::Split(None));
        assert_eq!(
            command("fullscreen enable global"),
            Command::Fullscreen {
                mode: Toggle::Enable,
                global: true
            }
        );
        assert_eq!(
            command("floating toggle"),
            Command::Floating(Toggle::Toggle)
        );
        assert_eq!(
            command("border toggle 10"),
            Command::Border(swayward_ipc::command::Border {
                style: BorderStyle::Toggle,
                width: Some(10)
            })
        );
        assert_eq!(
            command("workspace next_on_output"),
            Command::Workspace {
                target: WorkspaceTarget::NextOnOutput,
                auto_back_and_forth: true,
            }
        );
        assert_eq!(
            command("workspace number 2:chat"),
            Command::Workspace {
                target: WorkspaceTarget::Number("2:chat".into()),
                auto_back_and_forth: true,
            }
        );
        assert_eq!(command("kill"), Command::Kill);
        assert_eq!(command("kill window"), Command::Kill);
        assert_eq!(command("kill client extra arguments"), Command::Kill);
        assert_eq!(
            command("resize shrink height 10 ppt"),
            Command::Resize {
                grow: false,
                axis: ResizeAxis::Height,
                first: ResizeAmount {
                    amount: 10,
                    unit: ResizeUnit::PercentagePoints,
                },
                second: None,
            }
        );
        assert_eq!(command("reload"), Command::Reload);
        assert_eq!(command("nop anything is ignored"), Command::Nop);
        assert_eq!(
            command("exec --no-startup-id notify-send 'hello; world'"),
            Command::Exec("notify-send 'hello; world'".into())
        );
        assert_eq!(
            command("exec_always echo hi"),
            Command::Exec("echo hi".into())
        );
    }

    #[test]
    fn splits_chains_outside_quotes() {
        let parsed = parse("focus left, move right; exec echo 'a,b;c'");
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0].as_ref().unwrap().command,
            Command::FocusDirection(Direction::Left)
        );
        assert_eq!(
            parsed[1].as_ref().unwrap().command,
            Command::MoveDirection {
                direction: Direction::Right,
                pixels: None
            }
        );
        assert_eq!(
            parsed[2].as_ref().unwrap().command,
            Command::Exec("echo 'a,b;c'".into())
        );
    }

    #[test]
    fn comma_keeps_criteria_and_semicolon_clears_it() {
        let parsed = parse(r#"[app_id="foo,bar"] focus left, focus right; focus up"#);
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0].as_ref().unwrap().criteria.as_deref(),
            Some(r#"[app_id="foo,bar"]"#)
        );
        assert_eq!(
            parsed[1].as_ref().unwrap().criteria.as_deref(),
            Some(r#"[app_id="foo,bar"]"#)
        );
        assert_eq!(parsed[2].as_ref().unwrap().criteria, None);
    }

    #[test]
    fn rejects_invalid_criteria_before_executing_commands() {
        for input in [r#"[bogus=\"x\"] nop"#, r#"[app_id=\"(\"] nop"#, "[] nop"] {
            let error = parse(input).into_iter().next().unwrap().unwrap_err();
            assert_eq!(error.parse_error, Some(true), "{input}");
        }
    }

    #[test]
    fn parses_sway_move_positions() {
        let px = |amount| ResizeAmount {
            amount,
            unit: ResizeUnit::Pixels,
        };
        let ppt = |amount| ResizeAmount {
            amount,
            unit: ResizeUnit::PercentagePoints,
        };
        assert_eq!(
            command("move position 5 px 15px"),
            Command::MovePosition(MovePosition::Coordinates {
                x: px(5),
                y: px(15),
                absolute: false,
            })
        );
        assert_eq!(
            command("move position 20 ppt 30ppt"),
            Command::MovePosition(MovePosition::Coordinates {
                x: ppt(20),
                y: ppt(30),
                absolute: false,
            })
        );
        assert_eq!(
            command("move absolute position center"),
            Command::MovePosition(MovePosition::Center { absolute: true })
        );
        for pointer in ["cursor", "mouse", "pointer"] {
            assert_eq!(
                command(&format!("move position {pointer}")),
                Command::MovePosition(MovePosition::Pointer)
            );
        }
    }

    #[test]
    fn parses_sway_move_distances() {
        for (input, pixels) in [
            ("move left", None),
            ("move left 20", Some(20)),
            ("move left 20 px", Some(20)),
            ("move left 20 PX", Some(20)),
            ("move left 20px", Some(20)),
            ("move left px", Some(0)),
            ("move left -20px", Some(-20)),
            ("move left 25 ppt", Some(25)),
        ] {
            assert_eq!(
                command(input),
                Command::MoveDirection {
                    direction: Direction::Left,
                    pixels,
                },
                "{input}"
            );
        }
        for input in ["move left 20ppt", "move left 20wat"] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some("Invalid distance specified"),
                "{input}"
            );
        }
    }

    #[test]
    fn parses_sway_resize_adjust_forms() {
        assert_eq!(
            command("resize grow up 10 px or 25 ppt"),
            Command::Resize {
                grow: true,
                axis: ResizeAxis::Up,
                first: ResizeAmount {
                    amount: 10,
                    unit: ResizeUnit::Pixels,
                },
                second: Some(ResizeAmount {
                    amount: 25,
                    unit: ResizeUnit::PercentagePoints,
                }),
            }
        );
        assert_eq!(
            command("resize shrink left 10px"),
            Command::Resize {
                grow: false,
                axis: ResizeAxis::Left,
                first: ResizeAmount {
                    amount: 10,
                    unit: ResizeUnit::Pixels,
                },
                second: None,
            }
        );
        assert_eq!(
            command("resize grow right"),
            Command::Resize {
                grow: true,
                axis: ResizeAxis::Right,
                first: ResizeAmount {
                    amount: 10,
                    unit: ResizeUnit::Default,
                },
                second: None,
            }
        );
        assert_eq!(
            command("resize grow width 10px or 10ppt"),
            Command::Resize {
                grow: true,
                axis: ResizeAxis::Width,
                first: ResizeAmount {
                    amount: 10,
                    unit: ResizeUnit::Pixels,
                },
                second: Some(ResizeAmount {
                    amount: 10,
                    unit: ResizeUnit::PercentagePoints,
                }),
            }
        );
    }

    #[test]
    fn parser_is_case_insensitive() {
        assert_eq!(
            command("FOCUS LEFT"),
            Command::FocusDirection(Direction::Left)
        );
        assert_eq!(
            command("resize GROW width 5 PPT"),
            Command::Resize {
                grow: true,
                axis: ResizeAxis::Width,
                first: ResizeAmount {
                    amount: 5,
                    unit: ResizeUnit::PercentagePoints,
                },
                second: None,
            }
        );
    }

    #[test]
    fn parses_workspace_names_with_spaces() {
        assert_eq!(
            command("workspace number 3: web browser"),
            Command::Workspace {
                target: WorkspaceTarget::Number("3: web browser".into()),
                auto_back_and_forth: true,
            }
        );
        assert_eq!(
            command("workspace 'mail and chat'"),
            Command::Workspace {
                target: WorkspaceTarget::Name("mail and chat".into()),
                auto_back_and_forth: true,
            }
        );
    }

    #[test]
    fn malformed_and_unknown_commands_are_parse_errors() {
        for input in [
            "focus sideways",
            "resize grow width nope px",
            "frobnicate",
            "[app_id=foo focus left",
            "fullscreen enable global extra",
            "exec",
        ] {
            let error = parse(input).into_iter().next().unwrap().unwrap_err();
            assert!(!error.success, "{input}");
            assert_eq!(error.parse_error, Some(true), "{input}");
            assert!(error.error.is_some(), "{input}");
        }
    }
}

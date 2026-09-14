use swayward_config::Action;
use swayward_ipc::command::parse_error;
pub use swayward_ipc::command::{
    parse, parse_boolean, BorderStyle, Command, Direction, Layout, LayoutToggle, LayoutToggleEntry,
    MovePosition, OutputTarget, ParsedCommand, ResizeAmount, ResizeAxis, ResizeUnit, Toggle,
    WorkspaceTarget,
};
use swayward_ipc::legacy::{PositionChange, SizeChange};
use swayward_ipc::{criteria, CommandOutcome};

use crate::swayward::State;
use crate::utils::spawning::spawn_sh;

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
        Command::Focus => None,
        Command::FocusDirection(direction) => {
            let action = match direction {
                Direction::Left => Action::FocusColumnOrMonitorLeft,
                Direction::Right => Action::FocusColumnOrMonitorRight,
                Direction::Up => Action::FocusWindowOrMonitorUp,
                Direction::Down => Action::FocusWindowOrMonitorDown,
            };
            let changed = match direction {
                Direction::Left => state.swayward.layout.focus_left(),
                Direction::Right => state.swayward.layout.focus_right(),
                Direction::Up => state.swayward.layout.focus_up(),
                Direction::Down => state.swayward.layout.focus_down(),
            };
            if changed {
                state.swayward.queue_redraw_all();
                None
            } else if state.swayward.layout.global_fullscreen_active()
                || direction == Direction::Left
                    && state.swayward.layout.focused_fullscreen_mode().is_some()
            {
                None
            } else {
                Some(action)
            }
        }
        Command::FocusOutput(identifier) => {
            let output = match output_target_by_name_or_direction(state, &identifier) {
                Ok(output) => output,
                Err(error) => return failure(error),
            };
            if let Some(output) = output {
                state.swayward.layout.focus_output(&output);
                state.swayward.queue_redraw_all();
            }
            None
        }
        Command::FocusParent => {
            state.swayward.layout.focus_parent();
            state.swayward.queue_redraw_all();
            None
        }
        Command::FocusChild => {
            state.swayward.layout.focus_child();
            state.swayward.queue_redraw_all();
            None
        }
        Command::FocusNext => Some(Action::FocusColumnRightOrFirst),
        Command::FocusPrev => Some(Action::FocusColumnLeftOrLast),
        Command::FocusFloating => {
            state.swayward.layout.disable_active_workspace_fullscreen();
            Some(Action::FocusFloating)
        }
        Command::FocusTiling => {
            state.swayward.layout.disable_active_workspace_fullscreen();
            Some(Action::FocusTiling)
        }
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
            state.swayward.layout.move_to_scratchpad(None);
            state.swayward.queue_redraw_all();
            None
        }
        Command::ScratchpadShow => {
            state.swayward.layout.show_scratchpad(None);
            state.swayward.queue_redraw_all();
            None
        }
        Command::LayoutDefault => {
            if state
                .swayward
                .layout
                .active_workspace()
                .is_some_and(|workspace| workspace.floating_is_active())
            {
                return failure("Unable to change layout of floating windows");
            }
            state.swayward.layout.restore_focused_split_layout();
            state.swayward.queue_redraw_all();
            None
        }
        Command::LayoutToggle(cycle) => {
            if state
                .swayward
                .layout
                .active_workspace()
                .is_some_and(|workspace| workspace.floating_is_active())
            {
                return failure("Unable to change layout of floating windows");
            }
            state.swayward.layout.toggle_focused_layout(&cycle);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Layout(layout) => {
            if state
                .swayward
                .layout
                .active_workspace()
                .is_some_and(|workspace| workspace.floating_is_active())
            {
                return failure("Unable to change layout of floating windows");
            }
            match layout {
                Layout::SplitH => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::SplitH),
                Layout::SplitV => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::SplitV),
                Layout::Tabbed => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::Tabbed),
                Layout::Stacked => state
                    .swayward
                    .layout
                    .set_focused_layout(crate::layout::tiling_tree::Layout::Stacked),
                Layout::ToggleSplit => state.swayward.layout.toggle_focused_layout_split(),
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(Some(Layout::SplitH)) => {
            state
                .swayward
                .layout
                .split_focused(crate::layout::tiling_tree::Layout::SplitH);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(Some(Layout::SplitV)) => {
            state
                .swayward
                .layout
                .split_focused(crate::layout::tiling_tree::Layout::SplitV);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(Some(Layout::ToggleSplit)) => {
            state.swayward.layout.toggle_focused_split();
            state.swayward.queue_redraw_all();
            None
        }
        Command::Split(None) => return failure("container flattening is not implemented yet"),
        Command::Split(Some(Layout::Tabbed | Layout::Stacked)) => {
            return failure("invalid split layout");
        }
        Command::Fullscreen { mode, global } => {
            let current = state.swayward.layout.focused_fullscreen_mode();
            let enabled = match mode {
                Toggle::Enable => true,
                Toggle::Disable => false,
                Toggle::Toggle => current.is_none(),
            };
            let fullscreen = enabled.then_some(if global {
                crate::layout::tiling_tree::FullscreenMode::Global
            } else {
                crate::layout::tiling_tree::FullscreenMode::Workspace
            });
            state
                .swayward
                .layout
                .set_focused_fullscreen_mode(fullscreen);
            state.swayward.queue_redraw_all();
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
        Command::Workspace(target) => {
            if let Err(error) = state.swayward.layout.activate_sway_workspace(target) {
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

fn parse_output_direction(value: &str) -> Option<Direction> {
    match value.to_ascii_lowercase().as_str() {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    }
}

fn output_target_by_name_or_direction(
    state: &State,
    identifier: &str,
) -> Result<Option<smithay::output::Output>, String> {
    if let Some(output) = state.swayward.output_by_name_match(identifier) {
        return Ok(Some(output.clone()));
    }
    let Some(direction) = parse_output_direction(identifier) else {
        return Err("There is no output with that name.".into());
    };
    let Some(reference) = state.swayward.layout.active_output() else {
        return Err("No focused workspace to base directions off of.".into());
    };
    Ok(match direction {
        Direction::Left => state.swayward.output_left_of(reference),
        Direction::Right => state.swayward.output_right_of(reference),
        Direction::Up => state.swayward.output_up_of(reference),
        Direction::Down => state.swayward.output_down_of(reference),
    })
}

fn output_target(
    state: &State,
    target: &OutputTarget,
    reference: Option<&smithay::output::Output>,
    reference_point: Option<smithay::utils::Point<i32, smithay::utils::Logical>>,
) -> Result<smithay::output::Output, String> {
    let output = match target {
        OutputTarget::Name(name) if name.eq_ignore_ascii_case("current") => {
            state.swayward.layout.active_output().cloned()
        }
        OutputTarget::Name(name) => state.swayward.output_by_name_match(name).cloned(),
        OutputTarget::Direction(direction) => match (direction, reference) {
            (Direction::Left, Some(output)) => state.swayward.output_left_of_point(
                output,
                reference_point.unwrap_or_else(|| {
                    crate::utils::center(
                        state.swayward.global_space.output_geometry(output).unwrap(),
                    )
                }),
            ),
            (Direction::Right, Some(output)) => state.swayward.output_right_of_point(
                output,
                reference_point.unwrap_or_else(|| {
                    crate::utils::center(
                        state.swayward.global_space.output_geometry(output).unwrap(),
                    )
                }),
            ),
            (Direction::Up, Some(output)) => state.swayward.output_up_of_point(
                output,
                reference_point.unwrap_or_else(|| {
                    crate::utils::center(
                        state.swayward.global_space.output_geometry(output).unwrap(),
                    )
                }),
            ),
            (Direction::Down, Some(output)) => state.swayward.output_down_of_point(
                output,
                reference_point.unwrap_or_else(|| {
                    crate::utils::center(
                        state.swayward.global_space.output_geometry(output).unwrap(),
                    )
                }),
            ),
            (Direction::Left, None) => state.swayward.output_left(),
            (Direction::Right, None) => state.swayward.output_right(),
            (Direction::Up, None) => state.swayward.output_up(),
            (Direction::Down, None) => state.swayward.output_down(),
        },
    };
    output.ok_or_else(|| {
        format!(
            "Can't find output with name/direction '{}'",
            output_target_name(target)
        )
    })
}

fn output_target_name(target: &OutputTarget) -> &str {
    match target {
        OutputTarget::Name(name) => name,
        OutputTarget::Direction(Direction::Left) => "left",
        OutputTarget::Direction(Direction::Right) => "right",
        OutputTarget::Direction(Direction::Up) => "up",
        OutputTarget::Direction(Direction::Down) => "down",
    }
}

fn move_position(
    state: &mut State,
    target: Option<crate::window::mapped::MappedId>,
    position: &MovePosition,
) -> Result<(), &'static str> {
    let window = match target {
        Some(target) => Some(
            state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()))
                .ok_or("No matching node.")?,
        ),
        None => None,
    };
    let workspace = window
        .as_ref()
        .and_then(|window| {
            state
                .swayward
                .layout
                .workspaces()
                .find_map(|(_, _, workspace)| workspace.has_window(window).then_some(workspace))
        })
        .or_else(|| state.swayward.layout.active_workspace())
        .ok_or("Only floating containers can be moved to an absolute position")?;
    if !window
        .as_ref()
        .map_or(workspace.floating_is_active(), |window| {
            workspace.is_floating(window)
        })
    {
        return Err("Only floating containers can be moved to an absolute position");
    }
    let target_geometry = || {
        let id = window
            .as_ref()
            .or_else(|| state.swayward.layout.focus().map(|mapped| &mapped.window))?;
        state
            .swayward
            .layout
            .workspaces()
            .find_map(|(monitor, _, workspace)| {
                workspace
                    .tiles_with_ipc_layouts()
                    .find(|(tile, _)| &tile.window().window == id)
                    .map(|(tile, _)| {
                        let output_origin = monitor.map_or_else(Default::default, |monitor| {
                            monitor.output().current_location().to_f64()
                        });
                        (
                            tile.tile_size(),
                            output_origin + workspace.working_area().loc,
                        )
                    })
            })
    };
    let (x, y) = match *position {
        MovePosition::Coordinates { x, y, absolute } => {
            if absolute
                && (x.unit == ResizeUnit::PercentagePoints
                    || y.unit == ResizeUnit::PercentagePoints)
            {
                return Err("Cannot move to absolute positions by ppt");
            }
            let coordinate = |amount: ResizeAmount, extent: f64| match amount.unit {
                ResizeUnit::Default | ResizeUnit::Pixels => f64::from(amount.amount),
                ResizeUnit::PercentagePoints => extent * f64::from(amount.amount) / 100.,
            };
            let offset: smithay::utils::Point<f64, smithay::utils::Logical> = if absolute {
                let Some((_, workspace_origin)) = target_geometry() else {
                    return Err("Only floating containers can be moved to an absolute position");
                };
                (-workspace_origin.x, -workspace_origin.y).into()
            } else {
                Default::default()
            };
            (
                PositionChange::SetFixed(coordinate(x, workspace.working_area().size.w) + offset.x),
                PositionChange::SetFixed(coordinate(y, workspace.working_area().size.h) + offset.y),
            )
        }
        MovePosition::Center { absolute: false } => {
            state.swayward.layout.center_window(window.as_ref());
            return Ok(());
        }
        MovePosition::Center { absolute: true } => {
            let root = state
                .swayward
                .global_space
                .outputs()
                .filter_map(|output| state.swayward.global_space.output_geometry(output))
                .reduce(|root, output| root.merge(output));
            let Some(root) = root else { return Ok(()) };
            let Some((tile_size, workspace_origin)) = target_geometry() else {
                return Err("Only floating containers can be moved to an absolute position");
            };
            let root_center = crate::utils::center(root).to_f64();
            let position = root_center - tile_size.downscale(2.) - workspace_origin;
            (
                PositionChange::SetFixed(position.x),
                PositionChange::SetFixed(position.y),
            )
        }
        MovePosition::Pointer => {
            let pointer = state
                .swayward
                .seat
                .get_pointer()
                .ok_or("No cursor device")?
                .current_location();
            let Some((tile_size, workspace_origin)) = target_geometry() else {
                return Err("Only floating containers can be moved to an absolute position");
            };
            let position = pointer - tile_size.downscale(2.) - workspace_origin;
            (
                PositionChange::SetFixed(position.x),
                PositionChange::SetFixed(position.y),
            )
        }
    };
    state
        .swayward
        .layout
        .move_floating_window(window.as_ref(), x, y, true);
    Ok(())
}

fn select_resize_amount(
    first: ResizeAmount,
    second: Option<ResizeAmount>,
    floating: bool,
) -> ResizeAmount {
    let preferred = if floating {
        ResizeUnit::Pixels
    } else {
        ResizeUnit::PercentagePoints
    };
    [Some(first), second]
        .into_iter()
        .flatten()
        .find(|amount| amount.unit == preferred)
        .or_else(|| {
            [Some(first), second]
                .into_iter()
                .flatten()
                .find(|amount| amount.unit == ResizeUnit::Default)
        })
        .unwrap_or(first)
}

fn move_target_to_workspace(
    state: &mut State,
    target: CommandTarget,
    workspace_target: WorkspaceTarget,
    preserve_empty_workspace: bool,
) -> CommandOutcome {
    let result = match target {
        CommandTarget::Window(target) => {
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
            };
            state
                .swayward
                .layout
                .move_window_to_sway_workspace(&window, workspace_target)
                .map(|_| ())
        }
        CommandTarget::Container(workspace, node) => {
            let (target_workspace, remapped) =
                match state.swayward.layout.move_tiling_subtree_to_sway_workspace(
                    workspace,
                    node,
                    workspace_target,
                    preserve_empty_workspace,
                ) {
                    Ok(moved) => moved,
                    Err(error) => return failure(error),
                };
            for (old, new) in remapped {
                if let Some(marks) = state.swayward.marks_by_container.remove(&(workspace, old)) {
                    state
                        .swayward
                        .marks_by_container
                        .insert((target_workspace, new), marks);
                }
            }
            Ok(())
        }
    };
    if let Err(error) = result {
        return failure(error);
    }
    state.swayward.queue_redraw_all();
    success()
}

fn marked_target(state: &State, mark: &str) -> Option<CommandTarget> {
    state
        .swayward
        .marks_by_window
        .iter()
        .find_map(|(window, marks)| {
            marks
                .iter()
                .any(|existing| existing == mark)
                .then_some(*window)
        })
        .map(CommandTarget::Window)
        .or_else(|| {
            state
                .swayward
                .marks_by_container
                .iter()
                .find_map(|(&(workspace, node), marks)| {
                    marks
                        .iter()
                        .any(|existing| existing == mark)
                        .then_some(CommandTarget::Container(workspace, node))
                })
        })
}

fn move_target_to_mark(state: &mut State, source: CommandTarget, mark: &str) -> CommandOutcome {
    let Some(destination) = marked_target(state, mark) else {
        return failure(format!("Mark '{mark}' not found"));
    };
    let destination =
        match destination {
            CommandTarget::Container(workspace, node) => (workspace, node),
            CommandTarget::Window(window) => {
                let mapped =
                    state.swayward.layout.windows().find_map(|(_, mapped)| {
                        (mapped.id() == window).then(|| mapped.window.clone())
                    });
                let mapped = mapped.or_else(|| {
                    state
                        .swayward
                        .layout
                        .scratchpad_windows()
                        .find_map(|mapped| (mapped.id() == window).then(|| mapped.window.clone()))
                });
                let Some(mapped) = mapped else {
                    return failure("No matching node.");
                };
                if state.swayward.layout.is_scratchpad_hidden(&mapped) {
                    let CommandTarget::Window(source) = source else {
                        return failure(
                            "moving container subtrees to scratchpad is not implemented yet",
                        );
                    };
                    let source = state.swayward.layout.windows().find_map(|(_, mapped)| {
                        (mapped.id() == source).then(|| mapped.window.clone())
                    });
                    let Some(source) = source else {
                        return failure("No matching node.");
                    };
                    state.swayward.layout.move_to_scratchpad(Some(&source));
                    state.swayward.queue_redraw_all();
                    return success();
                }
                if let Some(target) = state.swayward.layout.tiling_target_for_window(&mapped) {
                    target
                } else {
                    let Some(workspace) = state.swayward.layout.window_workspace_id(&mapped) else {
                        return failure("No matching node.");
                    };
                    let CommandTarget::Window(source) = source else {
                        return failure(
                            "moving container subtrees to floating marks is not implemented yet",
                        );
                    };
                    let source = state.swayward.layout.windows().find_map(|(_, mapped)| {
                        (mapped.id() == source).then(|| mapped.window.clone())
                    });
                    let Some(source) = source else {
                        return failure("No matching node.");
                    };
                    if let Err(error) = state
                        .swayward
                        .layout
                        .move_window_to_workspace_id(&source, workspace)
                    {
                        return failure(error);
                    }
                    state.swayward.queue_redraw_all();
                    return success();
                }
            }
        };
    let source = match source {
        CommandTarget::Container(workspace, node) => (workspace, node),
        CommandTarget::Window(window) => {
            let Some(mapped) = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == window).then(|| mapped.window.clone()))
            else {
                return failure("No matching node.");
            };
            if state
                .swayward
                .layout
                .workspaces()
                .any(|(_, _, ws)| ws.is_floating(&mapped))
            {
                if let Err(error) = state
                    .swayward
                    .layout
                    .move_window_to_workspace_id(&mapped, destination.0)
                {
                    return failure(error);
                }
                state.swayward.queue_redraw_all();
                return success();
            }
            let Some(source) = state.swayward.layout.tiling_target_for_window(&mapped) else {
                return failure("No matching node.");
            };
            source
        }
    };
    let remapped = match state.swayward.layout.move_tiling_subtree_to_node(
        source.0,
        source.1,
        destination.0,
        destination.1,
    ) {
        Ok(remapped) => remapped,
        Err(error) => return failure(error),
    };
    for (old, new) in remapped {
        if let Some(marks) = state.swayward.marks_by_container.remove(&(source.0, old)) {
            state
                .swayward
                .marks_by_container
                .insert((destination.0, new), marks);
        }
    }
    state.swayward.queue_redraw_all();
    success()
}

fn execute_targeted(state: &mut State, command: &Command, target: CommandTarget) -> CommandOutcome {
    match command {
        Command::Mark {
            add,
            toggle,
            identifier,
        } => mark_target(state, target, identifier, *add, *toggle),
        Command::Unmark(identifier) => unmark_target(state, target, identifier.as_deref()),
        Command::MoveDirection { direction, pixels } => {
            let direction = match direction {
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
                    state.swayward.layout.move_window_in_direction(
                        &window,
                        direction,
                        f64::from(pixels.unwrap_or(10)),
                    );
                }
                CommandTarget::Container(workspace, node) => {
                    state
                        .swayward
                        .layout
                        .move_tiling_node_in_direction(workspace, node, direction);
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
            state.swayward.layout.move_to_scratchpad(Some(&window));
            state.swayward.queue_redraw_all();
        }
        Command::ScratchpadShow => {
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
            if !state.swayward.layout.is_scratchpad_window(&window) {
                return failure("Container is not in scratchpad.");
            }
            state.swayward.layout.show_scratchpad(Some(&window));
            state.swayward.queue_redraw_all();
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
            let CommandTarget::Window(target) = target else {
                return failure("No current container");
            };
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
            };
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return success();
            }
            if !state.swayward.layout.set_window_sticky(&window, value) {
                return failure("Expected output to have a workspace");
            }
            state.swayward.queue_redraw_all();
        }
        Command::Border(border) => {
            let CommandTarget::Window(target) = target else {
                return failure("Only views can have borders");
            };
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
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
        }
        Command::Floating(mode) => {
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
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return failure("Can't change floating on hidden scratchpad container");
            }
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
        }
        Command::Kill => {
            let CommandTarget::Window(target) = target else {
                return failure("command requires a window target");
            };
            state.do_action(Action::CloseWindowById(target.get()), false);
        }
        Command::Resize {
            grow,
            axis,
            first,
            second,
        } => {
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
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return failure("Cannot resize a hidden scratchpad container");
            }
            let floating = state
                .swayward
                .layout
                .windows()
                .any(|(_, mapped)| mapped.id() == target && mapped.is_floating());
            let selected = select_resize_amount(*first, *second, floating);
            let sign = if *grow { 1 } else { -1 };
            let amount = selected.amount.saturating_mul(sign);
            let change = match selected.unit {
                ResizeUnit::Default if floating => SizeChange::AdjustFixed(amount),
                ResizeUnit::Pixels => SizeChange::AdjustFixed(amount),
                ResizeUnit::Default | ResizeUnit::PercentagePoints => {
                    SizeChange::AdjustProportion(f64::from(amount))
                }
            };
            match axis {
                ResizeAxis::Width => state
                    .swayward
                    .layout
                    .set_window_width(Some(&window), change),
                ResizeAxis::Height => state
                    .swayward
                    .layout
                    .set_window_height(Some(&window), change),
                direction => {
                    let edge = match direction {
                        ResizeAxis::Up => crate::utils::ResizeEdge::TOP,
                        ResizeAxis::Down => crate::utils::ResizeEdge::BOTTOM,
                        ResizeAxis::Left => crate::utils::ResizeEdge::LEFT,
                        ResizeAxis::Right => crate::utils::ResizeEdge::RIGHT,
                        ResizeAxis::Width | ResizeAxis::Height => unreachable!(),
                    };
                    state
                        .swayward
                        .layout
                        .resize_window_edge(Some(&window), edge, change);
                }
            }
        }
        Command::Focus => match target {
            CommandTarget::Window(target) => {
                let window =
                    state.swayward.layout.windows().find_map(|(_, mapped)| {
                        (mapped.id() == target).then(|| mapped.window.clone())
                    });
                let Some(window) = window else {
                    return failure("No matching node.");
                };
                state.swayward.layout.activate_window(&window);
            }
            CommandTarget::Container(workspace, node) => {
                if !state.swayward.layout.focus_tiling_node(workspace, node) {
                    return failure("No matching node.");
                }
            }
        },
        Command::FocusDirection(direction) => {
            let CommandTarget::Window(target) = target else {
                return failure("directional focus requires a window target");
            };
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()));
            let Some(window) = window else {
                return failure("No matching node.");
            };
            state.swayward.layout.activate_window(&window);
            state.do_action(
                match direction {
                    Direction::Left => Action::FocusColumnLeft,
                    Direction::Right => Action::FocusColumnRight,
                    Direction::Up => Action::FocusWindowUp,
                    Direction::Down => Action::FocusWindowDown,
                },
                false,
            );
        }
        Command::Layout(layout) => {
            let node = match target {
                CommandTarget::Container(_, node) => node,
                CommandTarget::Window(target) => {
                    let floating = state
                        .swayward
                        .layout
                        .windows()
                        .any(|(_, mapped)| mapped.id() == target && mapped.is_floating());
                    return failure(if floating {
                        "Unable to change layout of floating windows"
                    } else {
                        "command requires a container target"
                    });
                }
            };
            let layout = match layout {
                Layout::SplitH => crate::layout::tiling_tree::Layout::SplitH,
                Layout::SplitV => crate::layout::tiling_tree::Layout::SplitV,
                Layout::Tabbed => crate::layout::tiling_tree::Layout::Tabbed,
                Layout::Stacked => crate::layout::tiling_tree::Layout::Stacked,
                Layout::ToggleSplit => {
                    return failure("targeted toggle split is not implemented yet")
                }
            };
            state.swayward.layout.set_tiling_node_layout(node, layout);
            state.swayward.queue_redraw_all();
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
    state
        .swayward
        .layout
        .focused_tiling_node()
        .map(|id| crate::ipc::tree::container_id(id) as u64)
        .or_else(|| focused_id(state).map(|id| crate::ipc::tree::window_id(id) as u64))
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
            Command::Workspace(WorkspaceTarget::NextOnOutput)
        );
        assert_eq!(
            command("workspace number 2:chat"),
            Command::Workspace(WorkspaceTarget::Number("2:chat".into()))
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
            Command::Workspace(WorkspaceTarget::Number("3: web browser".into()))
        );
        assert_eq!(
            command("workspace 'mail and chat'"),
            Command::Workspace(WorkspaceTarget::Name("mail and chat".into()))
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

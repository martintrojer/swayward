use swayward_ipc::legacy::PositionChange;
use swayward_ipc::CommandOutcome;

use super::{
    failure, success, CommandTarget, Direction, MovePosition, OutputTarget, ResizeAmount,
    ResizeUnit, WorkspaceTarget,
};
use crate::swayward::State;

fn parse_output_direction(value: &str) -> Option<Direction> {
    match value.to_ascii_lowercase().as_str() {
        "left" => Some(Direction::Left),
        "right" => Some(Direction::Right),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    }
}

pub(super) fn output_target_by_name_or_direction(
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

pub(super) fn output_target(
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

pub(super) fn move_position(
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

pub(super) fn select_resize_amount(
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

pub(super) fn move_target_to_workspace(
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

pub(super) fn move_target_to_mark(
    state: &mut State,
    source: CommandTarget,
    mark: &str,
) -> CommandOutcome {
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

use swayward_ipc::legacy::PositionChange;
use swayward_ipc::CommandOutcome;

use super::{
    failure, success, CommandTarget, Direction, MovePosition, OutputTarget, ResizeAmount,
    ResizeUnit, SwapTarget, WorkspaceTarget,
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
) -> Result<Option<smithay::output::Output>, CommandOutcome> {
    if let Some(output) = state.swayward.output_by_name_match(identifier) {
        return Ok(Some(output.clone()));
    }
    let Some(direction) = parse_output_direction(identifier) else {
        return Err(swayward_ipc::command::parse_error(
            "There is no output with that name.",
        ));
    };
    let Some(reference) = state.swayward.layout.active_output() else {
        return Err(failure("No focused workspace to base directions off of."));
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
            (direction, Some(output)) => {
                let reference = match reference_point {
                    Some(point) => point,
                    None => state
                        .swayward
                        .global_space
                        .output_geometry(output)
                        .map(crate::utils::center)
                        .ok_or("Reference output has no geometry")?,
                };
                let (horizontal, positive) = match direction {
                    Direction::Left => (true, false),
                    Direction::Right => (true, true),
                    Direction::Up => (false, false),
                    Direction::Down => (false, true),
                };
                state
                    .swayward
                    .output_in_direction_wrapping(output, reference, horizontal, positive)
            }
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

pub(crate) fn clamp_pointer_position(
    mut position: smithay::utils::Point<f64, smithay::utils::Logical>,
    size: smithay::utils::Size<f64, smithay::utils::Logical>,
    output: Option<smithay::utils::Rectangle<f64, smithay::utils::Logical>>,
) -> smithay::utils::Point<f64, smithay::utils::Logical> {
    let Some(output) = output else {
        return position;
    };
    let right = output.loc.x + output.size.w;
    let bottom = output.loc.y + output.size.h;
    position.x = position.x.max(output.loc.x);
    position.y = position.y.max(output.loc.y);
    if position.x + size.w > right {
        position.x = right - size.w;
    }
    if position.y + size.h > bottom {
        position.y = bottom - size.h;
    }
    position
}

type LogicalSize = smithay::utils::Size<f64, smithay::utils::Logical>;
type LogicalPoint = smithay::utils::Point<f64, smithay::utils::Logical>;

fn move_target_geometry(
    state: &State,
    window: Option<&smithay::desktop::Window>,
) -> Option<(LogicalSize, LogicalPoint)> {
    let id = window.or_else(|| state.swayward.layout.focus().map(|mapped| &mapped.window))?;
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
}

fn coordinate(amount: ResizeAmount, extent: f64) -> f64 {
    match amount.unit {
        ResizeUnit::Default | ResizeUnit::Pixels => f64::from(amount.amount),
        ResizeUnit::PercentagePoints => extent * f64::from(amount.amount) / 100.,
    }
}

fn move_to_pointer(
    state: &mut State,
    window: Option<&smithay::desktop::Window>,
) -> Result<(), &'static str> {
    let pointer = state
        .swayward
        .seat
        .get_pointer()
        .ok_or("No cursor device")?
        .current_location();
    let id = window.cloned().or_else(|| {
        state
            .swayward
            .layout
            .focus()
            .map(|mapped| mapped.window.clone())
    });
    let Some(id) = id else {
        return Err("Only floating containers can be moved to an absolute position");
    };
    let Some((tile_size, workspace_origin)) = move_target_geometry(state, window) else {
        return Err("Only floating containers can be moved to an absolute position");
    };
    let cursor_output = state
        .swayward
        .global_space
        .output_under(pointer)
        .next()
        .cloned();
    let output_geometry = cursor_output
        .as_ref()
        .and_then(|output| state.swayward.global_space.output_geometry(output));
    let position = clamp_pointer_position(
        pointer - tile_size.downscale(2.),
        tile_size,
        output_geometry.map(|output| output.to_f64()),
    );
    let window_output = state
        .swayward
        .layout
        .windows()
        .find_map(|(monitor, mapped)| {
            (mapped.window == id)
                .then(|| monitor.map(|monitor| monitor.output().clone()))
                .flatten()
        });
    if let Some(output) = cursor_output
        .as_ref()
        .filter(|output| window_output.as_ref() != Some(*output))
    {
        state.swayward.layout.move_to_output(
            Some(&id),
            output,
            None,
            crate::layout::ActivateWindow::Yes,
        );
    }
    let workspace_origin = output_geometry
        .map(|output| output.loc.to_f64())
        .unwrap_or(workspace_origin);
    let position = position - workspace_origin;
    state.swayward.layout.move_floating_window(
        Some(&id),
        PositionChange::SetFixed(position.x),
        PositionChange::SetFixed(position.y),
        true,
    );
    Ok(())
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
        None => state
            .swayward
            .layout
            .focus()
            .map(|mapped| mapped.window.clone()),
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
    let (x, y) = match *position {
        MovePosition::Coordinates { x, y, absolute } => {
            if absolute
                && (x.unit == ResizeUnit::PercentagePoints
                    || y.unit == ResizeUnit::PercentagePoints)
            {
                return Err("Cannot move to absolute positions by ppt");
            }
            let offset: LogicalPoint = if absolute {
                let Some((_, workspace_origin)) = move_target_geometry(state, window.as_ref())
                else {
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
            let Some((tile_size, workspace_origin)) = move_target_geometry(state, window.as_ref())
            else {
                return Err("Only floating containers can be moved to an absolute position");
            };
            let root_center = crate::utils::center(root).to_f64();
            let position = root_center - tile_size.downscale(2.) - workspace_origin;
            (
                PositionChange::SetFixed(position.x),
                PositionChange::SetFixed(position.y),
            )
        }
        MovePosition::Pointer => return move_to_pointer(state, window.as_ref()),
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

fn workspace_to_move(
    state: &State,
    target: Option<CommandTarget>,
) -> Result<Option<crate::layout::workspace::WorkspaceId>, CommandOutcome> {
    Ok(match target {
        Some(CommandTarget::Window(target)) => {
            let Some((window, workspace)) = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| {
                    (mapped.id() == target).then(|| {
                        (
                            mapped.window.clone(),
                            state.swayward.layout.window_workspace_id(&mapped.window),
                        )
                    })
                })
                .or_else(|| {
                    state
                        .swayward
                        .layout
                        .scratchpad_windows()
                        .find(|mapped| mapped.id() == target)
                        .map(|mapped| (mapped.window.clone(), None))
                })
            else {
                return Err(failure("No matching node."));
            };
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return Ok(None);
            }
            workspace
        }
        Some(CommandTarget::Container(workspace, _)) => Some(workspace),
        None => state
            .swayward
            .layout
            .active_workspace()
            .map(|workspace| workspace.id()),
    })
}

pub(super) fn move_workspace_to_output(
    state: &mut State,
    target: Option<CommandTarget>,
    output_target_name: &OutputTarget,
) -> CommandOutcome {
    let workspace_id = match workspace_to_move(state, target) {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return success(),
        Err(error) => return error,
    };
    let Some(reference) = state
        .swayward
        .layout
        .workspaces()
        .find(|(_, _, workspace)| workspace.id() == workspace_id)
        .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output().clone()))
    else {
        return failure("No workspace to move");
    };
    let reference_point = state
        .swayward
        .global_space
        .output_geometry(&reference)
        .map(crate::utils::center);
    let output = match output_target(state, output_target_name, Some(&reference), reference_point) {
        Ok(output) => output,
        Err(error) => return failure(error),
    };
    state
        .swayward
        .layout
        .move_workspace_to_output_by_id(workspace_id, Some(reference), &output);
    state.swayward.queue_redraw_all();
    success()
}

pub(super) fn move_tiling_subtree_to_output(
    state: &mut State,
    source_workspace: crate::layout::workspace::WorkspaceId,
    node: crate::layout::tiling_tree::NodeId,
    output: &smithay::output::Output,
) -> CommandOutcome {
    let Some(target_workspace) = state.swayward.layout.active_workspace_id_for_output(output)
    else {
        return failure("target output has no workspace");
    };
    let target = WorkspaceTarget::Name(
        state
            .swayward
            .layout
            .find_workspace_by_id(target_workspace)
            .and_then(|(_, workspace)| workspace.sway_name())
            .unwrap_or_else(|| target_workspace.get().to_string()),
    );
    move_target_to_workspace(
        state,
        CommandTarget::Container(source_workspace, node),
        target,
        false,
        false,
    )
}

pub(super) fn move_target_to_workspace(
    state: &mut State,
    target: CommandTarget,
    workspace_target: WorkspaceTarget,
    preserve_empty_workspace: bool,
    auto_back_and_forth: bool,
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
                .move_window_to_sway_workspace(&window, workspace_target, auto_back_and_forth)
                .map(|_| ())
        }
        CommandTarget::Container(workspace, node) => {
            let (target_workspace, remapped) =
                match state.swayward.layout.move_tiling_subtree_to_sway_workspace(
                    workspace,
                    node,
                    workspace_target,
                    preserve_empty_workspace,
                    auto_back_and_forth,
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

fn swap_kind_value(target: &SwapTarget) -> (&'static str, &str) {
    match target {
        SwapTarget::Id(value) => ("id", value),
        SwapTarget::ConId(value) => ("con_id", value),
        SwapTarget::Mark(value) => ("mark", value),
    }
}

fn swap_destination(state: &State, target: &SwapTarget) -> Result<CommandTarget, CommandOutcome> {
    let (kind, value, destination) = match target {
        SwapTarget::Id(_) => {
            return Err(failure(
                "swap container with id is unsupported because X11 window IDs are unavailable",
            ));
        }
        SwapTarget::ConId(value) => (
            "con_id",
            value,
            value.parse::<i64>().ok().and_then(|id| {
                state
                    .swayward
                    .layout
                    .workspaces()
                    .find_map(|(_, _, workspace)| {
                        workspace
                            .ipc_tiling_tree()
                            .nodes()
                            .into_iter()
                            .find_map(|(node, _)| {
                                (crate::ipc::tree::container_id(node) == id)
                                    .then_some(CommandTarget::Container(workspace.id(), node))
                            })
                    })
                    .or_else(|| {
                        state.swayward.layout.windows().find_map(|(_, mapped)| {
                            (crate::ipc::tree::window_id(mapped.id()) == id)
                                .then_some(CommandTarget::Window(mapped.id()))
                        })
                    })
            }),
        ),
        SwapTarget::Mark(value) => ("mark", value, marked_target(state, value)),
    };
    destination.ok_or_else(|| failure(format!("Failed to find {kind} '{value}'")))
}

type TilingEndpoint = (
    crate::layout::workspace::WorkspaceId,
    crate::layout::tiling_tree::NodeId,
);

fn resolve_swap_endpoint(
    state: &State,
    target: CommandTarget,
    source: bool,
    missing: impl FnOnce() -> CommandOutcome,
) -> Result<TilingEndpoint, CommandOutcome> {
    match target {
        CommandTarget::Container(workspace, node)
            if if source {
                state
                    .swayward
                    .layout
                    .active_workspace()
                    .is_some_and(|active| {
                        active.id() == workspace && active.contains_swap_node(node)
                    })
            } else {
                state.swayward.layout.workspaces().any(|(_, _, candidate)| {
                    candidate.id() == workspace && candidate.contains_swap_node(node)
                })
            } =>
        {
            Ok((workspace, node))
        }
        CommandTarget::Container(_, _) => Err(missing()),
        CommandTarget::Window(window) => {
            let Some(mapped) = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == window).then(|| mapped.window.clone()))
            else {
                return Err(missing());
            };
            state
                .swayward
                .layout
                .swap_target_for_window(&mapped)
                .ok_or_else(|| failure("Can only swap with containers and views"))
        }
    }
}

fn remap_container_marks(
    state: &mut State,
    source: crate::layout::workspace::WorkspaceId,
    destination: crate::layout::workspace::WorkspaceId,
    remapped: impl IntoIterator<
        Item = (
            crate::layout::tiling_tree::NodeId,
            crate::layout::tiling_tree::NodeId,
        ),
    >,
) {
    for (old, new) in remapped {
        if let Some(marks) = state.swayward.marks_by_container.remove(&(source, old)) {
            state
                .swayward
                .marks_by_container
                .insert((destination, new), marks);
        }
    }
}

pub(super) fn swap_target(
    state: &mut State,
    source: CommandTarget,
    target: &SwapTarget,
) -> CommandOutcome {
    let destination = match swap_destination(state, target) {
        Ok(destination) => destination,
        Err(error) => return error,
    };
    if source == destination {
        return failure("Cannot swap a container with itself");
    }
    let (source_workspace, source_node) = match resolve_swap_endpoint(state, source, true, || {
        failure("Can only swap with containers and views")
    }) {
        Ok(endpoint) => endpoint,
        Err(error) => return error,
    };
    let (destination_workspace, destination_node) =
        match resolve_swap_endpoint(state, destination, false, || {
            let (kind, value) = swap_kind_value(target);
            failure(format!("Failed to find {kind} '{value}'"))
        }) {
            Ok(endpoint) => endpoint,
            Err(error) => return error,
        };
    if state
        .swayward
        .layout
        .is_tiling_root(source_workspace, source_node)
        || state
            .swayward
            .layout
            .is_tiling_root(destination_workspace, destination_node)
    {
        return failure("Can only swap with containers and views");
    }
    if source_workspace != destination_workspace {
        if !state
            .swayward
            .layout
            .workspace_contains_tiling_node(source_workspace, source_node)
            || !state
                .swayward
                .layout
                .workspace_contains_tiling_node(destination_workspace, destination_node)
        {
            return failure("Can only swap with containers and views");
        }
        let remapped = match state.swayward.layout.swap_tiling_nodes_between_workspaces(
            source_workspace,
            source_node,
            destination_workspace,
            destination_node,
        ) {
            Ok(remapped) => remapped,
            Err(error) => return failure(error),
        };
        remap_container_marks(
            state,
            source_workspace,
            destination_workspace,
            remapped.first,
        );
        remap_container_marks(
            state,
            destination_workspace,
            source_workspace,
            remapped.second,
        );
    } else if let Err(error) =
        state
            .swayward
            .layout
            .swap_tiling_nodes(source_workspace, source_node, destination_node)
    {
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

enum MarkDestination {
    Scratchpad,
    Floating(crate::layout::workspace::WorkspaceId),
    Tiling(TilingEndpoint),
}

fn mapped_window(
    state: &State,
    id: crate::window::mapped::MappedId,
) -> Option<smithay::desktop::Window> {
    state
        .swayward
        .layout
        .windows()
        .find_map(|(_, mapped)| (mapped.id() == id).then(|| mapped.window.clone()))
}

fn resolve_mark_destination(
    state: &State,
    destination: CommandTarget,
) -> Result<MarkDestination, CommandOutcome> {
    let CommandTarget::Window(window) = destination else {
        let CommandTarget::Container(workspace, node) = destination else {
            unreachable!()
        };
        return Ok(MarkDestination::Tiling((workspace, node)));
    };
    let mapped = mapped_window(state, window).or_else(|| {
        state
            .swayward
            .layout
            .scratchpad_windows()
            .find_map(|mapped| (mapped.id() == window).then(|| mapped.window.clone()))
    });
    let Some(mapped) = mapped else {
        return Err(failure("No matching node."));
    };
    if state.swayward.layout.is_scratchpad_hidden(&mapped) {
        return Ok(MarkDestination::Scratchpad);
    }
    if let Some(target) = state.swayward.layout.tiling_target_for_window(&mapped) {
        return Ok(MarkDestination::Tiling(target));
    }
    state
        .swayward
        .layout
        .window_workspace_id(&mapped)
        .map(MarkDestination::Floating)
        .ok_or_else(|| failure("No matching node."))
}

fn move_window_to_mark_workspace(
    state: &mut State,
    source: CommandTarget,
    workspace: Option<crate::layout::workspace::WorkspaceId>,
) -> CommandOutcome {
    let CommandTarget::Window(source) = source else {
        return failure(if workspace.is_some() {
            "moving container subtrees to floating marks is not implemented yet"
        } else {
            "moving container subtrees to scratchpad is not implemented yet"
        });
    };
    let Some(source) = mapped_window(state, source) else {
        return failure("No matching node.");
    };
    let result = match workspace {
        Some(workspace) => state
            .swayward
            .layout
            .move_window_to_workspace_id(&source, workspace),
        None => {
            state.swayward.layout.move_to_scratchpad(Some(&source));
            Ok(())
        }
    };
    if let Err(error) = result {
        return failure(error);
    }
    state.swayward.queue_redraw_all();
    success()
}

pub(super) fn move_target_to_mark(
    state: &mut State,
    source: CommandTarget,
    mark: &str,
) -> CommandOutcome {
    let Some(destination) = marked_target(state, mark) else {
        return failure(format!("Mark '{mark}' not found"));
    };
    let destination = match resolve_mark_destination(state, destination) {
        Ok(MarkDestination::Scratchpad) => {
            return move_window_to_mark_workspace(state, source, None)
        }
        Ok(MarkDestination::Floating(workspace)) => {
            return move_window_to_mark_workspace(state, source, Some(workspace));
        }
        Ok(MarkDestination::Tiling(destination)) => destination,
        Err(error) => return error,
    };
    let source = match source {
        CommandTarget::Container(workspace, node) => (workspace, node),
        CommandTarget::Window(window) => {
            let Some(mapped) = mapped_window(state, window) else {
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
    remap_container_marks(state, source.0, destination.0, remapped);
    state.swayward.queue_redraw_all();
    success()
}

#[cfg(test)]
mod tests {
    use smithay::utils::{Point, Rectangle, Size};

    use super::clamp_pointer_position;

    #[test]
    fn pointer_position_clamps_to_cursor_output_but_not_outside_outputs() {
        let output = Rectangle::new(Point::from((1000., 50.)), Size::from((800., 600.)));
        let size = Size::from((300., 200.));
        assert_eq!(
            clamp_pointer_position(Point::from((900., -50.)), size, Some(output)),
            Point::from((1000., 50.))
        );
        assert_eq!(
            clamp_pointer_position(Point::from((1700., 550.)), size, Some(output)),
            Point::from((1500., 450.))
        );
        assert_eq!(
            clamp_pointer_position(Point::from((850., 300.)), size, None),
            Point::from((850., 300.))
        );
    }
}

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
            let size = workspace
                .floating_tree_root_for_window(id)
                .and_then(|root| workspace.floating().tree_rect(root))
                .map(|rect| rect.size)
                .or_else(|| {
                    workspace
                        .tiles_with_ipc_layouts()
                        .find(|(tile, _)| &tile.window().window == id)
                        .map(|(tile, _)| tile.tile_size())
                })?;
            let output_origin = monitor.map_or_else(Default::default, |monitor| {
                monitor.output().current_location().to_f64()
            });
            Some((size, output_origin + workspace.working_area().loc))
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
    target: Option<CommandTarget>,
    position: &MovePosition,
) -> Result<(), &'static str> {
    let (window, floating_root) = match target {
        Some(CommandTarget::Window(target)) => (
            state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == target).then(|| mapped.window.clone()))
                .ok_or("No matching node.")?,
            false,
        ),
        Some(CommandTarget::Container(workspace, node)) => {
            let workspace = state
                .swayward
                .layout
                .find_workspace_by_id(workspace)
                .map(|(_, workspace)| workspace)
                .ok_or("No matching node.")?;
            if workspace.floating().tree_root_for_node(node) != Some(node) {
                return Err("command requires a window target");
            }
            let window = workspace
                .floating()
                .tree(node)
                .and_then(crate::layout::tiling_tree::TilingTree::active_window)
                .map(|mapped| mapped.window.clone())
                .ok_or("No matching node.")?;
            (window, true)
        }
        None => {
            let workspace = state
                .swayward
                .layout
                .active_workspace()
                .ok_or("Only floating containers can be moved to an absolute position")?;
            let floating_root = workspace
                .focused_floating_tree_root()
                .is_some_and(|root| workspace.focused_container_node() == Some(root));
            let window = workspace
                .active_window()
                .map(|mapped| mapped.window.clone())
                .ok_or("Only floating containers can be moved to an absolute position")?;
            (window, floating_root)
        }
    };
    let workspace = state
        .swayward
        .layout
        .workspaces()
        .find_map(|(_, _, workspace)| workspace.has_window(&window).then_some(workspace))
        .ok_or("Only floating containers can be moved to an absolute position")?;
    if !(floating_root || workspace.is_floating(&window)) {
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
                let Some((_, workspace_origin)) = move_target_geometry(state, Some(&window)) else {
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
            state.swayward.layout.center_window(Some(&window));
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
            let Some((tile_size, workspace_origin)) = move_target_geometry(state, Some(&window))
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
        MovePosition::Pointer => return move_to_pointer(state, Some(&window)),
    };
    state
        .swayward
        .layout
        .move_floating_window(Some(&window), x, y, true);
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
            let (_, remapped) = match state.swayward.layout.move_tiling_subtree_to_sway_workspace(
                workspace,
                node,
                workspace_target,
                preserve_empty_workspace,
                auto_back_and_forth,
            ) {
                Ok(moved) => moved,
                Err(error) => return failure(error),
            };
            state.swayward.remap_container_marks(remapped);
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
        state
            .swayward
            .remap_container_marks(remapped.first.into_iter().chain(remapped.second));
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
                .find_map(|(&node, marks)| {
                    marks
                        .iter()
                        .any(|existing| existing == mark)
                        .then_some(node)
                })
                .and_then(|node| {
                    let workspace = state.swayward.layout.workspace_containing_node(node)?;
                    Some(CommandTarget::Container(workspace, node))
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
    state.swayward.remap_container_marks(remapped);
    state.swayward.queue_redraw_all();
    success()
}

pub(super) fn swap_focused(state: &mut State, target: SwapTarget) -> super::HandlerResult {
    let Some(source) = super::targeted::focused_target(state) else {
        return Err(failure("Can only swap with containers and views"));
    };
    super::handled_outcome(swap_target(state, source, &target))
}

pub(super) fn direction_focused(
    state: &mut State,
    direction: Direction,
    pixels: Option<i32>,
) -> super::HandlerResult {
    let Some(workspace) = state.swayward.layout.active_workspace() else {
        return Err(failure("Cannot move workspaces in a direction"));
    };
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(super::command_failure(
            "Cannot move workspaces in a direction",
        ));
    };
    if matches!(target, CommandTarget::Container(workspace, node)
        if state.swayward.layout.is_tiling_root(workspace, node))
    {
        return Err(super::command_failure(
            "Cannot move workspaces in a direction",
        ));
    }
    let fullscreen_floating = workspace.active_floating_is_fullscreen();
    if workspace.floating_is_active() || fullscreen_floating {
        if fullscreen_floating {
            return Err(failure("Cannot move fullscreen floating container"));
        }
        if state
            .swayward
            .layout
            .focused_leaf_is_only_child_of_floating_tree_root()
        {
            return Err(success());
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
        Ok(None)
    } else {
        super::handled_outcome(super::targeted::move_direction(
            state,
            target,
            direction,
            pixels,
            crate::layout::ActivateWindow::Smart,
            true,
        ))
    }
}

pub(super) fn position_focused(state: &mut State, position: MovePosition) -> super::HandlerResult {
    move_position(state, None, &position).map_err(failure)?;
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn to_workspace_focused(
    state: &mut State,
    target: WorkspaceTarget,
    auto_back_and_forth: bool,
) -> super::HandlerResult {
    if state.swayward.layout.global_fullscreen_active()
        && state
            .swayward
            .layout
            .focused_window_is_fullscreen_or_child()
    {
        return Err(failure("Can't move fullscreen global container"));
    }
    let Some(focused) = super::targeted::focused_target(state) else {
        return Err(super::command_failure("Can't move an empty workspace"));
    };
    let auto_back_and_forth = auto_back_and_forth
        && state
            .swayward
            .config
            .borrow()
            .input
            .workspace_auto_back_and_forth;
    super::handled_outcome(move_target_to_workspace(
        state,
        focused,
        target,
        false,
        auto_back_and_forth,
    ))
}

pub(super) fn to_mark_focused(state: &mut State, mark: &str) -> super::HandlerResult {
    let Some(source) = super::targeted::focused_target(state) else {
        return Err(success());
    };
    super::handled_outcome(move_target_to_mark(state, source, mark))
}

pub(super) fn to_output_focused(state: &mut State, target: &OutputTarget) -> super::HandlerResult {
    let focused_target = super::targeted::focused_target(state);
    let focused = state
        .swayward
        .layout
        .focus_with_output()
        .map(|(window, output)| (window.window.clone(), output.clone()));
    let reference = focused
        .as_ref()
        .and_then(|(window, _)| state.swayward.layout.window_center(window));
    let reference_output = focused.as_ref().map(|(_, output)| output);
    let output = output_target(state, target, reference_output, reference).map_err(failure)?;
    if let Some(CommandTarget::Container(workspace, node)) = focused_target {
        super::handled_outcome(move_tiling_subtree_to_output(
            state, workspace, node, &output,
        ))?;
    } else {
        state.swayward.layout.move_to_output(
            None,
            &output,
            None,
            crate::layout::ActivateWindow::No,
        );
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn workspace_to_output_focused(
    state: &mut State,
    target: &OutputTarget,
) -> super::HandlerResult {
    super::handled_outcome(move_workspace_to_output(state, None, target))
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

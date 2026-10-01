use swayward_config::Action;
use swayward_ipc::command::Border;
use swayward_ipc::legacy::SizeChange;
use swayward_ipc::CommandOutcome;

use super::{
    failure, parse_boolean, tiling_target, CommandTarget, ResizeAmount, ResizeAxis, ResizeUnit,
    Toggle,
};
use crate::swayward::State;

fn target_window(
    state: &State,
    target: CommandTarget,
    container_error: &str,
) -> Result<smithay::desktop::Window, CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure(container_error));
    };
    super::mapped_window(state, target).ok_or_else(|| failure("No matching node."))
}

fn apply_sticky(
    state: &mut State,
    target: Option<CommandTarget>,
    window: &smithay::desktop::Window,
    value: &str,
) -> Result<(), CommandOutcome> {
    if state.swayward.layout.is_scratchpad_hidden(window) {
        return Ok(());
    }
    let applied = match target {
        Some(CommandTarget::Container(workspace, node)) => state
            .swayward
            .layout
            .set_floating_group_sticky(workspace, node, value),
        Some(CommandTarget::Window(_)) | None => None,
    };
    if !applied.unwrap_or_else(|| state.swayward.layout.set_window_sticky(window, value)) {
        return Err(failure("Expected output to have a workspace"));
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn sticky(
    state: &mut State,
    target: CommandTarget,
    value: &str,
) -> Result<(), CommandOutcome> {
    let window = match target {
        CommandTarget::Container(workspace, node) => {
            if state
                .swayward
                .layout
                .set_split_sticky(workspace, node, value)
            {
                return Ok(());
            }
            state
                .swayward
                .layout
                .window_in_node(workspace, node)
                .ok_or_else(|| failure("No matching node."))?
        }
        CommandTarget::Window(_) => target_window(state, target, "No matching node.")?,
    };
    apply_sticky(state, Some(target), &window, value)
}

pub(super) fn urgent(
    state: &mut State,
    target: CommandTarget,
    value: &str,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("Only views can be urgent"));
    };
    let urgent = state
        .swayward
        .layout
        .windows()
        .find_map(|(_, window)| (window.id() == target).then(|| window.is_urgent()))
        .ok_or_else(|| failure("No matching node."))?;
    let urgent = parse_boolean(value, urgent);
    state.swayward.set_window_urgent(target, urgent);
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn opacity(
    state: &mut State,
    target: CommandTarget,
    value: f32,
    relative: bool,
) -> Result<(), CommandOutcome> {
    let windows = match target {
        CommandTarget::Window(target) => {
            state.swayward.layout.windows().find_map(|(_, mapped)| {
                (mapped.id() == target).then(|| vec![mapped.window.clone()])
            })
        }
        CommandTarget::Container(workspace, node) => {
            state.swayward.layout.tiling_node_windows(workspace, node)
        }
    }
    .ok_or_else(|| failure("No matching node."))?;

    let mut result = Ok(());
    state.swayward.layout.with_windows_mut(|mapped, _| {
        if windows.contains(&mapped.window) {
            let opacity = if relative {
                mapped.command_opacity() + value
            } else {
                value
            };
            if (0. ..=1.).contains(&opacity) {
                mapped.set_command_opacity(opacity);
            } else {
                result = Err(failure("opacity value out of bounds"));
            }
        }
    });
    if result.is_ok() {
        state.swayward.queue_redraw_all();
    }
    result
}

pub(super) fn title_format(
    state: &mut State,
    target: CommandTarget,
    format: &str,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        let CommandTarget::Container(workspace, node) = target else {
            unreachable!()
        };
        if !state
            .swayward
            .layout
            .set_tiling_node_title_format(workspace, node, format.to_owned())
        {
            return Err(failure("No matching node."));
        }
        state.swayward.queue_redraw_all();
        return Ok(());
    };
    let mut window = None;
    state.swayward.layout.with_windows_mut(|mapped, _| {
        if mapped.id() == target {
            mapped.set_title_format(format.to_owned());
            window = Some(mapped.window.clone());
        }
    });
    let Some(window) = window else {
        return Err(failure("No matching node."));
    };
    state.swayward.layout.update_window(&window, None);
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn border(
    state: &mut State,
    target: CommandTarget,
    border: &Border,
) -> Result<(), CommandOutcome> {
    let window = target_window(state, target, "Only views can have borders")?;
    state
        .swayward
        .layout
        .set_window_border(&window, border.style, border.width)
        .map_err(failure)?;
    state.swayward.queue_redraw_all();
    Ok(())
}

fn set_container_floating(
    state: &mut State,
    workspace: crate::layout::workspace::WorkspaceId,
    node: crate::layout::tiling_tree::NodeId,
    mode: Toggle,
) -> Result<(), CommandOutcome> {
    let floating = match mode {
        Toggle::Enable => true,
        Toggle::Disable => false,
        Toggle::Toggle => state
            .swayward
            .layout
            .active_workspace()
            .is_some_and(|workspace| workspace.tiling().contains(node)),
    };
    let Some(root) = state
        .swayward
        .layout
        .set_container_floating(workspace, node, floating)
    else {
        return Err(failure("No matching node."));
    };
    state.ipc_refresh_layout();
    state.ipc_emit_window_change(
        "floating",
        crate::ipc::tree::container_id(root),
        |container| {
            if floating {
                container["type"] = "floating_con".into();
                container["floating"] = "user_on".into();
            }
        },
    );
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn floating(
    state: &mut State,
    target: CommandTarget,
    mode: &Toggle,
) -> Result<(), CommandOutcome> {
    if let CommandTarget::Container(workspace, node) = target {
        set_container_floating(state, workspace, node, *mode)?;
        return Ok(());
    }
    let window = target_window(state, target, "No matching node.")?;
    if state.swayward.layout.is_scratchpad_hidden(&window) {
        return Err(failure(
            "Can't change floating on hidden scratchpad container",
        ));
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
    Ok(())
}

pub(super) fn kill(state: &mut State, target: CommandTarget) -> Result<(), CommandOutcome> {
    let targets = match target {
        CommandTarget::Window(target) => vec![target.get()],
        CommandTarget::Container(_, _) => {
            let (workspace, node) =
                tiling_target(state, target, "command requires a tiling target")?;
            let windows = state
                .swayward
                .layout
                .tiling_node_windows(workspace, node)
                .ok_or_else(|| failure("No matching node."))?;
            state
                .swayward
                .layout
                .windows()
                .filter(|(_, mapped)| windows.contains(&mapped.window))
                .map(|(_, mapped)| mapped.id().get())
                .collect()
        }
    };
    for target in targets {
        state.do_action(Action::CloseWindowById(target), false);
    }
    Ok(())
}

fn set_size_change(amount: ResizeAmount, floating: bool) -> Option<SizeChange> {
    (amount.amount > 0).then(|| match amount.unit {
        ResizeUnit::Pixels | ResizeUnit::Default if floating => SizeChange::SetFixed(amount.amount),
        ResizeUnit::Pixels => SizeChange::SetFixed(amount.amount),
        ResizeUnit::Default | ResizeUnit::PercentagePoints => {
            SizeChange::SetProportion(f64::from(amount.amount))
        }
    })
}

pub(super) fn resize_set(
    state: &mut State,
    target: CommandTarget,
    width: Option<ResizeAmount>,
    height: Option<ResizeAmount>,
) -> Result<(), CommandOutcome> {
    let floating = match target {
        CommandTarget::Window(target) => state
            .swayward
            .layout
            .windows()
            .any(|(_, mapped)| mapped.id() == target && mapped.is_floating()),
        CommandTarget::Container(_, _) => false,
    };
    let width = width.and_then(|amount| set_size_change(amount, floating));
    let height = height.and_then(|amount| set_size_change(amount, floating));
    match target {
        CommandTarget::Window(_) => {
            let window = target_window(state, target, "command requires a window target")?;
            if state.swayward.layout.is_scratchpad_hidden(&window) {
                return Err(failure("Cannot resize a hidden scratchpad container"));
            }
            state
                .swayward
                .layout
                .set_window_size_sway(&window, width, height);
        }
        CommandTarget::Container(workspace, node) => {
            state
                .swayward
                .layout
                .set_tiling_node_size_sway(workspace, node, width, height);
        }
    }
    Ok(())
}

enum ResolvedResizeTarget {
    Window {
        window: smithay::desktop::Window,
        floating: bool,
    },
    Container {
        workspace: crate::layout::workspace::WorkspaceId,
        node: crate::layout::tiling_tree::NodeId,
    },
}

impl ResolvedResizeTarget {
    fn resolve(state: &State, target: CommandTarget) -> Result<Self, CommandOutcome> {
        match target {
            CommandTarget::Window(target) => {
                let Some(mapped) = state
                    .swayward
                    .layout
                    .windows()
                    .find_map(|(_, mapped)| (mapped.id() == target).then_some(mapped))
                else {
                    return Err(failure("No matching node."));
                };
                let window = mapped.window.clone();
                let floating = mapped.is_floating();
                if state.swayward.layout.is_scratchpad_hidden(&window) {
                    return Err(failure("Cannot resize a hidden scratchpad container"));
                }
                Ok(Self::Window { window, floating })
            }
            CommandTarget::Container(workspace, node) => Ok(Self::Container { workspace, node }),
        }
    }

    fn is_floating(&self) -> bool {
        matches!(self, Self::Window { floating: true, .. })
    }
}

fn resize_change(
    grow: bool,
    first: ResizeAmount,
    second: Option<ResizeAmount>,
    floating: bool,
) -> SizeChange {
    let selected = super::movement::select_resize_amount(first, second, floating);
    let sign = if grow { 1 } else { -1 };
    let amount = selected.amount.saturating_mul(sign);
    match selected.unit {
        ResizeUnit::Default if floating => SizeChange::AdjustFixed(amount),
        ResizeUnit::Pixels => SizeChange::AdjustFixed(amount),
        ResizeUnit::Default | ResizeUnit::PercentagePoints => {
            SizeChange::AdjustProportion(f64::from(amount))
        }
    }
}

fn resize_edge(axis: ResizeAxis) -> crate::utils::ResizeEdge {
    match axis {
        ResizeAxis::Up => crate::utils::ResizeEdge::TOP,
        ResizeAxis::Down => crate::utils::ResizeEdge::BOTTOM,
        ResizeAxis::Left => crate::utils::ResizeEdge::LEFT,
        ResizeAxis::Right => crate::utils::ResizeEdge::RIGHT,
        ResizeAxis::Width | ResizeAxis::Height => unreachable!(),
    }
}

fn apply_resize(
    state: &mut State,
    target: ResolvedResizeTarget,
    axis: ResizeAxis,
    change: SizeChange,
) -> Option<bool> {
    match (target, axis) {
        (ResolvedResizeTarget::Window { window, .. }, ResizeAxis::Width) => state
            .swayward
            .layout
            .set_window_width(Some(&window), change),
        (ResolvedResizeTarget::Window { window, .. }, ResizeAxis::Height) => state
            .swayward
            .layout
            .set_window_height(Some(&window), change),
        (ResolvedResizeTarget::Container { workspace, node }, ResizeAxis::Width) => state
            .swayward
            .layout
            .resize_tiling_node(workspace, node, true, change),
        (ResolvedResizeTarget::Container { workspace, node }, ResizeAxis::Height) => state
            .swayward
            .layout
            .resize_tiling_node(workspace, node, false, change),
        (ResolvedResizeTarget::Window { window, .. }, direction) => state
            .swayward
            .layout
            .resize_window_edge(Some(&window), resize_edge(direction), change),
        (ResolvedResizeTarget::Container { workspace, node }, direction) => state
            .swayward
            .layout
            .resize_tiling_node_edge(workspace, node, resize_edge(direction), change),
    }
}

pub(super) fn resize(
    state: &mut State,
    target: CommandTarget,
    grow: bool,
    axis: ResizeAxis,
    first: ResizeAmount,
    second: Option<ResizeAmount>,
) -> Result<(), CommandOutcome> {
    let target = ResolvedResizeTarget::resolve(state, target)?;
    let change = resize_change(grow, first, second, target.is_floating());
    if apply_resize(state, target, axis, change) == Some(false) {
        return Err(swayward_ipc::command::parse_error(
            "Cannot resize any further",
        ));
    }
    Ok(())
}

pub(super) fn opacity_focused(
    state: &mut State,
    value: f32,
    relative: bool,
) -> super::HandlerResult {
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(failure("No current container"));
    };
    super::handled(opacity(state, target, value, relative))
}

pub(super) fn title_format_focused(state: &mut State, format: &str) -> super::HandlerResult {
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(swayward_ipc::command::parse_error(
            "Only valid containers can have a title_format",
        ));
    };
    super::handled(title_format(state, target, format))
}

pub(super) fn shortcuts_inhibitor_focused(state: &mut State, enable: bool) -> super::HandlerResult {
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(failure("Only views can have shortcuts inhibitors"));
    };
    super::handled(super::targeted::set_shortcuts_inhibitor(
        state, target, enable,
    ))
}

pub(super) fn sticky_focused(state: &mut State, value: &str) -> super::HandlerResult {
    if state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| workspace.is_workspace_focused())
    {
        return Err(super::failure("No current container"));
    }
    let target = super::targeted::focused_target(state);
    let container_window = match target {
        Some(CommandTarget::Container(workspace, node)) => {
            if state
                .swayward
                .layout
                .set_split_sticky(workspace, node, value)
            {
                return Err(super::success());
            }
            state.swayward.layout.window_in_node(workspace, node)
        }
        _ => None,
    };
    let window = container_window.or_else(|| {
        state
            .swayward
            .layout
            .focus()
            .map(|mapped| mapped.window.clone())
    });
    let Some(window) = window else {
        return Err(super::failure("No current container"));
    };
    if state.swayward.layout.is_scratchpad_hidden(&window) {
        return Err(super::success());
    }
    super::handled(apply_sticky(state, target, &window, value))
}

pub(super) fn border_focused(state: &mut State, border: &Border) -> super::HandlerResult {
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(failure("Only views can have borders"));
    };
    super::handled(self::border(state, target, border))
}

pub(super) fn floating_focused(state: &mut State, mode: Toggle) -> super::HandlerResult {
    if let Some(CommandTarget::Container(workspace, node)) = super::targeted::focused_target(state)
    {
        set_container_floating(state, workspace, node, mode)?;
        return Err(super::success());
    }
    let Some(window) = state
        .swayward
        .layout
        .focus()
        .map(|mapped| mapped.window.clone())
    else {
        return Err(swayward_ipc::command::parse_error(
            "Can't float an empty workspace",
        ));
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
    Ok(None)
}

pub(super) fn urgent_focused(state: &mut State, value: &str) -> super::HandlerResult {
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(failure("No current container"));
    };
    super::handled(urgent(state, target, value))
}

/// `kill` with no criteria. With the workspace itself focused it closes every
/// view on it; otherwise it closes the focused container and its children
/// (`cmd_kill`, sway/sway/commands/kill.c:15-30).
pub(super) fn kill_focused(state: &mut State) -> super::HandlerResult {
    let workspace_windows = state
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
    if let Some(windows) = workspace_windows {
        for window in windows {
            state.do_action(Action::CloseWindowById(window), false);
        }
    } else if let Some(target) = super::targeted::focused_target(state) {
        kill(state, target)?;
    }
    Ok(None)
}

pub(super) fn resize_set_focused(
    state: &mut State,
    width: Option<ResizeAmount>,
    height: Option<ResizeAmount>,
) -> super::HandlerResult {
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(swayward_ipc::command::parse_error("Cannot resize nothing"));
    };
    super::handled(resize_set(state, target, width, height))
}

pub(super) fn resize_focused(
    state: &mut State,
    grow: bool,
    axis: ResizeAxis,
    first: ResizeAmount,
    second: Option<ResizeAmount>,
) -> super::HandlerResult {
    if state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| workspace.is_workspace_focused())
    {
        return Err(swayward_ipc::command::parse_error("Cannot resize nothing"));
    }
    let Some(target) = super::targeted::focused_target(state) else {
        return Err(swayward_ipc::command::parse_error("Cannot resize nothing"));
    };
    super::handled(resize(state, target, grow, axis, first, second))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_set_preserves_units_and_nonpositive_sentinels() {
        let amount = |amount, unit| ResizeAmount { amount, unit };
        assert_eq!(
            set_size_change(amount(50, ResizeUnit::Default), false),
            Some(SizeChange::SetProportion(50.))
        );
        assert_eq!(
            set_size_change(amount(50, ResizeUnit::Default), true),
            Some(SizeChange::SetFixed(50))
        );
        assert_eq!(
            set_size_change(amount(50, ResizeUnit::PercentagePoints), true),
            Some(SizeChange::SetProportion(50.))
        );
        assert_eq!(set_size_change(amount(0, ResizeUnit::Pixels), false), None);
        assert_eq!(
            set_size_change(amount(-1, ResizeUnit::PercentagePoints), true),
            None
        );
    }
}

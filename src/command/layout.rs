use swayward_ipc::CommandOutcome;

use super::{failure, tiling_target, CommandTarget, Layout, LayoutToggle, Toggle};
use crate::layout::tiling_tree::NodeId;
use crate::layout::workspace::WorkspaceId;
use crate::swayward::State;

fn remap_marks(state: &mut State, remapped: Option<(WorkspaceId, Vec<(NodeId, NodeId)>)>) {
    let Some((workspace, remapped)) = remapped else {
        return;
    };
    for (old, new) in remapped {
        if let Some(marks) = state.swayward.marks_by_container.remove(&(workspace, old)) {
            state
                .swayward
                .marks_by_container
                .entry((workspace, new))
                .or_default()
                .extend(marks);
        }
    }
}

fn reject_floating(state: &State) -> Result<(), CommandOutcome> {
    if state
        .swayward
        .layout
        .active_workspace()
        .is_some_and(|workspace| workspace.floating_is_active())
    {
        Err(failure("Unable to change layout of floating windows"))
    } else {
        Ok(())
    }
}

pub(super) fn default(state: &mut State) -> Result<(), CommandOutcome> {
    reject_floating(state)?;
    let remapped = state.swayward.layout.restore_focused_split_layout();
    remap_marks(state, remapped);
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn toggle(state: &mut State, cycle: &LayoutToggle) -> Result<(), CommandOutcome> {
    reject_floating(state)?;
    let remapped = state.swayward.layout.toggle_focused_layout(cycle);
    remap_marks(state, remapped);
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn set(state: &mut State, layout: Layout) -> Result<(), CommandOutcome> {
    reject_floating(state)?;
    let remapped = match layout {
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
    };
    remap_marks(state, remapped);
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn split(state: &mut State, layout: Option<Layout>) -> Result<(), CommandOutcome> {
    match layout {
        Some(Layout::SplitH) => state
            .swayward
            .layout
            .split_focused(crate::layout::tiling_tree::Layout::SplitH),
        Some(Layout::SplitV) => state
            .swayward
            .layout
            .split_focused(crate::layout::tiling_tree::Layout::SplitV),
        Some(Layout::ToggleSplit) => state.swayward.layout.toggle_focused_split(),
        None => {
            let remapped = state.swayward.layout.flatten_focused_parent();
            if remapped.is_none() {
                return Err(failure(
                    "Can only flatten a child container with no siblings",
                ));
            }
            remap_marks(state, remapped);
        }
        Some(Layout::Tabbed | Layout::Stacked) => return Err(failure("invalid split layout")),
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn split_targeted(
    state: &mut State,
    target: CommandTarget,
    layout: Option<Layout>,
) -> Result<(), CommandOutcome> {
    let (workspace, node) = tiling_target(state, target, "Unable to split floating windows")?;
    let changed = match layout {
        Some(Layout::SplitH) => state.swayward.layout.split_tiling_node(
            workspace,
            node,
            crate::layout::tiling_tree::Layout::SplitH,
        ),
        Some(Layout::SplitV) => state.swayward.layout.split_tiling_node(
            workspace,
            node,
            crate::layout::tiling_tree::Layout::SplitV,
        ),
        Some(Layout::ToggleSplit) => state
            .swayward
            .layout
            .toggle_tiling_node_split(workspace, node),
        None => {
            let remapped = state
                .swayward
                .layout
                .flatten_tiling_node_parent(workspace, node);
            if remapped.is_none() {
                return Err(failure(
                    "Can only flatten a child container with no siblings",
                ));
            }
            remap_marks(state, remapped);
            true
        }
        Some(Layout::Tabbed | Layout::Stacked) => return Err(failure("invalid split layout")),
    };
    if !changed {
        return Err(failure("No matching node."));
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn fullscreen(state: &mut State, mode: Toggle, global: bool) {
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
}

fn layout_target(
    state: &State,
    target: CommandTarget,
) -> Result<(WorkspaceId, NodeId, bool), CommandOutcome> {
    let container = matches!(target, CommandTarget::Container(_, _));
    let (workspace, node) =
        tiling_target(state, target, "Unable to change layout of floating windows")?;
    Ok((workspace, node, container))
}

pub(super) fn targeted(
    state: &mut State,
    target: CommandTarget,
    layout: Layout,
) -> Result<(), CommandOutcome> {
    let (workspace, node, container) = layout_target(state, target)?;
    let layout = match layout {
        Layout::SplitH => crate::layout::tiling_tree::Layout::SplitH,
        Layout::SplitV => crate::layout::tiling_tree::Layout::SplitV,
        Layout::Tabbed => crate::layout::tiling_tree::Layout::Tabbed,
        Layout::Stacked => crate::layout::tiling_tree::Layout::Stacked,
        Layout::ToggleSplit => return Err(failure("targeted toggle split is not implemented yet")),
    };
    let changed = if container {
        state
            .swayward
            .layout
            .set_tiling_node_layout_exact(workspace, node, layout)
    } else {
        state
            .swayward
            .layout
            .set_tiling_target_layout(workspace, node, layout)
    };
    if !changed {
        return Err(failure("No matching node."));
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn toggle_targeted(
    state: &mut State,
    target: CommandTarget,
    toggle: &LayoutToggle,
) -> Result<(), CommandOutcome> {
    let (workspace, node, container) = layout_target(state, target)?;
    if !state
        .swayward
        .layout
        .toggle_tiling_target_layout(workspace, node, toggle, container)
    {
        return Err(failure("No matching node."));
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn default_targeted(
    state: &mut State,
    target: CommandTarget,
) -> Result<(), CommandOutcome> {
    let (workspace, node, container) = layout_target(state, target)?;
    if !state
        .swayward
        .layout
        .restore_tiling_target_layout(workspace, node, container)
    {
        return Err(failure("No matching node."));
    }
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn fullscreen_targeted(
    state: &mut State,
    target: CommandTarget,
    mode: Toggle,
    global: bool,
) -> Result<(), CommandOutcome> {
    let (workspace, node) = tiling_target(state, target, "command requires a tiling target")?;
    let current = state
        .swayward
        .layout
        .tiling_node_fullscreen_mode(workspace, node)
        .ok_or_else(|| failure("No matching node."))?;
    let enabled = match mode {
        Toggle::Enable => true,
        Toggle::Disable => false,
        Toggle::Toggle => current.is_none(),
    };
    let mode = enabled.then_some(if global {
        crate::layout::tiling_tree::FullscreenMode::Global
    } else {
        crate::layout::tiling_tree::FullscreenMode::Workspace
    });
    state
        .swayward
        .layout
        .set_tiling_node_fullscreen_mode(workspace, node, mode);
    state.swayward.queue_redraw_all();
    Ok(())
}

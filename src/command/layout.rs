use swayward_ipc::CommandOutcome;

use super::{failure, CommandTarget, Layout, LayoutToggle, Toggle};
use crate::swayward::State;

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
    state.swayward.layout.restore_focused_split_layout();
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn toggle(state: &mut State, cycle: &LayoutToggle) -> Result<(), CommandOutcome> {
    reject_floating(state)?;
    state.swayward.layout.toggle_focused_layout(cycle);
    state.swayward.queue_redraw_all();
    Ok(())
}

pub(super) fn set(state: &mut State, layout: Layout) -> Result<(), CommandOutcome> {
    reject_floating(state)?;
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
        None => return Err(failure("container flattening is not implemented yet")),
        Some(Layout::Tabbed | Layout::Stacked) => return Err(failure("invalid split layout")),
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

pub(super) fn targeted(
    state: &mut State,
    target: CommandTarget,
    layout: Layout,
) -> Result<(), CommandOutcome> {
    let node = match target {
        CommandTarget::Container(_, node) => node,
        CommandTarget::Window(target) => {
            let floating = state
                .swayward
                .layout
                .windows()
                .any(|(_, mapped)| mapped.id() == target && mapped.is_floating());
            return Err(failure(if floating {
                "Unable to change layout of floating windows"
            } else {
                "command requires a container target"
            }));
        }
    };
    let layout = match layout {
        Layout::SplitH => crate::layout::tiling_tree::Layout::SplitH,
        Layout::SplitV => crate::layout::tiling_tree::Layout::SplitV,
        Layout::Tabbed => crate::layout::tiling_tree::Layout::Tabbed,
        Layout::Stacked => crate::layout::tiling_tree::Layout::Stacked,
        Layout::ToggleSplit => return Err(failure("targeted toggle split is not implemented yet")),
    };
    state.swayward.layout.set_tiling_node_layout(node, layout);
    state.swayward.queue_redraw_all();
    Ok(())
}

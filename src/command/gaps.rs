use super::HandlerResult;
use crate::swayward::State;

pub(super) fn update(
    state: &mut State,
    inner: bool,
    sides: [bool; 4],
    all: bool,
    operation: swayward_ipc::command::GapOperation,
    amount: i32,
) -> HandlerResult {
    if all {
        for workspace in state.swayward.layout.workspaces_mut() {
            workspace.update_gaps(inner, sides, operation, amount);
        }
    } else if let Some(workspace) = state.swayward.layout.active_workspace_mut() {
        workspace.update_gaps(inner, sides, operation, amount);
    }
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn defaults(
    state: &mut State,
    inner: bool,
    sides: [bool; 4],
    amount: i32,
) -> HandlerResult {
    // Sway's two-argument form writes the GLOBAL DEFAULT that later
    // workspaces inherit, and does not touch any existing workspace
    // (`sway/sway/commands/gaps.c:48-91`). Live workspaces in swayward
    // pin their own gaps over base options in `Workspace::update_config`
    // (`src/layout/workspace.rs:515-521`), so writing the default here
    // cannot disturb them and cannot fight `gaps ... set`, which writes
    // the per-workspace state instead.
    {
        let mut config = state.swayward.config.borrow_mut();
        let layout = &mut config.layout;
        if inner {
            // Sway floors inner gaps at zero (`gaps.c:63`).
            layout.gaps = f64::from(amount.max(0));
        } else {
            let outer = &mut layout.outer_gaps;
            for (selected, value) in sides.into_iter().zip([
                &mut outer.left,
                &mut outer.right,
                &mut outer.top,
                &mut outer.bottom,
            ]) {
                if selected {
                    *value = f64::from(amount);
                }
            }
            // Sway clamps a negative outer gap to -inner so windows cannot
            // leave the workspace (`gaps.c:30-43`).
            let floor = -layout.gaps;
            let outer = &mut layout.outer_gaps;
            for value in [
                &mut outer.left,
                &mut outer.right,
                &mut outer.top,
                &mut outer.bottom,
            ] {
                *value = value.max(floor);
            }
            layout.outer_gaps_configured = true;
        }
    }
    // Re-apply through the config path so a later workspace picks the new
    // default up, exactly as the other live config settings do.
    let config = state.swayward.config.clone();
    state.swayward.layout.update_config(&config.borrow());
    state.swayward.queue_redraw_all();
    Ok(None)
}

pub(super) fn workspace(
    state: &mut State,
    name: String,
    inner: bool,
    sides: [bool; 4],
    amount: i32,
) -> HandlerResult {
    // A per-workspace-name default. Sway stores it on the workspace CONFIG and
    // applies it when a workspace of that name is created
    // (`sway/sway/commands/workspace.c:57-117`;
    // `sway/sway/tree/workspace.c:224-242`), so like sway this does not
    // retroactively change a workspace that already exists.
    {
        let mut config = state.swayward.config.borrow_mut();
        let index = config
            .workspaces
            .iter()
            .position(|ws| ws.name.0.eq_ignore_ascii_case(&name))
            .unwrap_or_else(|| {
                config.workspaces.push(swayward_config::Workspace {
                    name: swayward_config::workspace::WorkspaceName(name.clone()),
                    sway_output_assignment: None,
                    open_on_output: None,
                    layout: None,
                });
                config.workspaces.len() - 1
            });
        let entry = &mut config.workspaces[index];
        let layout = entry.layout.get_or_insert_with(|| {
            swayward_config::WorkspaceLayoutPart(swayward_config::LayoutPart::default())
        });
        if inner {
            layout.0.gaps = Some(swayward_config::FloatOrInt(f64::from(amount.max(0))));
        } else {
            let outer = layout.0.outer_gaps.get_or_insert_with(Default::default);
            for (selected, value) in sides.into_iter().zip([
                &mut outer.left,
                &mut outer.right,
                &mut outer.top,
                &mut outer.bottom,
            ]) {
                if selected {
                    *value = Some(swayward_config::FloatOrInt(f64::from(amount)));
                }
            }
        }
    }
    // Refresh the stored configs so a workspace created later sees it.
    // Existing workspaces keep their own pinned gaps.
    let config = state.swayward.config.clone();
    state.swayward.layout.update_config(&config.borrow());
    Ok(None)
}

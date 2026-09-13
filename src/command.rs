use swayward_config::Action;
use swayward_ipc::command::parse_error;
pub use swayward_ipc::command::{
    parse, Command, Direction, Layout, ParsedCommand, ResizeAxis, ResizeUnit, Toggle,
    WorkspaceTarget,
};
use swayward_ipc::legacy::SizeChange;
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

    if matches!(parsed.command, Command::Mark { .. }) && focused_target(state).is_none() {
        return success();
    }

    let action = match parsed.command {
        Command::FocusDirection(Direction::Left) => Some(Action::FocusColumnLeft),
        Command::FocusDirection(Direction::Right) => Some(Action::FocusColumnRight),
        Command::FocusDirection(Direction::Up) => Some(Action::FocusWindowUp),
        Command::FocusDirection(Direction::Down) => Some(Action::FocusWindowDown),
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
        Command::MoveDirection {
            direction,
            pixels: None,
        }
        | Command::MoveDirection {
            direction,
            pixels: Some(10),
        } => Some(match direction {
            Direction::Left => Action::MoveColumnLeft,
            Direction::Right => Action::MoveColumnRight,
            Direction::Up => Action::MoveWindowUp,
            Direction::Down => Action::MoveWindowDown,
        }),
        Command::MoveDirection {
            pixels: Some(_), ..
        } => return failure("custom floating move distances are not implemented yet"),
        Command::MoveToWorkspace(target) => {
            if let Err(error) = state.swayward.layout.move_to_sway_workspace(target) {
                return failure(error);
            }
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
        Command::Fullscreen { global: true, .. } => {
            return failure("global fullscreen is not implemented yet");
        }
        Command::Fullscreen {
            mode,
            global: false,
        } => {
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
                return success();
            };
            match mode {
                Toggle::Enable => state.swayward.layout.set_fullscreen(&window, true),
                Toggle::Disable => state.swayward.layout.set_fullscreen(&window, false),
                Toggle::Toggle => state.swayward.layout.toggle_fullscreen(&window),
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
        Command::Kill => Some(Action::CloseWindow),
        Command::Resize {
            grow,
            axis,
            amount,
            unit,
        } => {
            let sign = if grow { 1 } else { -1 };
            let amount = amount.saturating_mul(sign);
            let change = match unit {
                ResizeUnit::Pixels => SizeChange::AdjustFixed(amount),
                ResizeUnit::PercentagePoints => SizeChange::AdjustProportion(f64::from(amount)),
            };
            Some(match axis {
                ResizeAxis::Width => Action::SetWindowWidth(change),
                ResizeAxis::Height => Action::SetWindowHeight(change),
            })
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
        Command::Mark {
            add,
            toggle,
            identifier,
        } => {
            if let Some(target) = focused_target(state) {
                mark_target(state, target, &identifier, add, toggle);
            }
            None
        }
        Command::Unmark(identifier) => {
            if parsed.criteria.is_some() {
                for target in targets {
                    unmark_target(state, target, identifier.as_deref());
                }
            } else {
                state.swayward.unmark(None, identifier.as_deref());
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

fn execute_targeted(state: &mut State, command: &Command, target: CommandTarget) -> CommandOutcome {
    match command {
        Command::Mark {
            add,
            toggle,
            identifier,
        } => mark_target(state, target, identifier, *add, *toggle),
        Command::Unmark(identifier) => unmark_target(state, target, identifier.as_deref()),
        Command::Fullscreen {
            mode,
            global: false,
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
            match mode {
                Toggle::Enable => state.swayward.layout.set_fullscreen(&window, true),
                Toggle::Disable => state.swayward.layout.set_fullscreen(&window, false),
                Toggle::Toggle => state.swayward.layout.toggle_fullscreen(&window),
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
    if let Some(window) = focused_id(state) {
        return Some(CommandTarget::Window(window));
    }
    let workspace = state.swayward.layout.active_workspace()?;
    workspace
        .focused_tiling_node()
        .map(|node| CommandTarget::Container(workspace.id(), node))
}

fn mark_target(state: &mut State, target: CommandTarget, mark: &str, add: bool, toggle: bool) {
    match target {
        CommandTarget::Window(window) => state.swayward.set_mark(window, mark, add, toggle),
        CommandTarget::Container(workspace, node) => {
            let marks = state
                .swayward
                .marks_by_container
                .entry((workspace, node))
                .or_default();
            if !add {
                marks.clear();
            }
            if let Some(index) = marks.iter().position(|existing| existing == mark) {
                if toggle {
                    marks.remove(index);
                }
            } else {
                marks.push(mark.to_owned());
            }
        }
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
    fn parses_every_supported_command_family() {
        assert_eq!(
            command("focus left"),
            Command::FocusDirection(Direction::Left)
        );
        assert_eq!(command("focus parent"), Command::FocusParent);
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
        assert_eq!(command("move scratchpad"), Command::MoveScratchpad);
        assert_eq!(command("scratchpad show"), Command::ScratchpadShow);
        assert_eq!(command("layout stacked"), Command::Layout(Layout::Stacked));
        assert_eq!(
            command("layout toggle split"),
            Command::Layout(Layout::ToggleSplit)
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
            command("workspace next_on_output"),
            Command::Workspace(WorkspaceTarget::NextOnOutput)
        );
        assert_eq!(
            command("workspace number 2:chat"),
            Command::Workspace(WorkspaceTarget::Number("2:chat".into()))
        );
        assert_eq!(command("kill"), Command::Kill);
        assert_eq!(
            command("resize shrink height 10 ppt"),
            Command::Resize {
                grow: false,
                axis: ResizeAxis::Height,
                amount: 10,
                unit: ResizeUnit::PercentagePoints
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
                amount: 5,
                unit: ResizeUnit::PercentagePoints,
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
            "fullscreen enable nope",
            "exec",
        ] {
            let error = parse(input).into_iter().next().unwrap().unwrap_err();
            assert!(!error.success, "{input}");
            assert_eq!(error.parse_error, Some(true), "{input}");
            assert!(error.error.is_some(), "{input}");
        }
    }
}

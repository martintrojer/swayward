use swayward_config::Action;
use swayward_ipc::command::{parse_with_variables, set_variable};
use swayward_ipc::{criteria, CommandOutcome};

use super::bindings::{mutate_key_binding, mutate_switch_binding, BindingMutation};
use super::movement::{
    move_position, move_target_to_mark, move_target_to_workspace, move_tiling_subtree_to_output,
    move_workspace_to_output, output_target,
};
use super::settings::{criteria_global_setting, execute_global_setting};
use super::targeted::{
    execute_targeted, focused_con_id, focused_target, mark_target, matching_targets,
    move_direction, set_client_colors, set_shortcuts_inhibitor, unmark_globally, unmark_target,
};
use super::{
    command_failure, create_output, failure, focus, layout, movement, parse_boolean, scratchpad,
    success, window, Command, CommandTarget, Direction, ParsedCommand, PositionChange, Toggle,
    WorkspaceTarget, XkbLayoutTarget,
};
use crate::swayward::State;
use crate::utils::spawning::{spawn_sh, spawn_sh_without_startup_id};

pub fn execute(state: &mut State, input: &str) -> Vec<CommandOutcome> {
    state.ipc_begin_workspace_transaction();
    // Sway expands variables before dispatch, for every argument except the
    // name being defined by `set` (`sway/sway/commands.c:283-285`). This is the
    // single choke point for both IPC commands and key bindings, matching
    // sway, where a binding re-enters execute_command at press time
    // (`sway/sway/commands/bind.c:635`).
    let mut parsed = parse_with_variables(input, &state.swayward.sway_variables);
    if state.swayward.layout.focus().is_none() {
        for parsed in &mut parsed {
            if matches!(parsed, Ok(parsed) if matches!(parsed.command, Command::Border(_))) {
                *parsed = Err(swayward_ipc::command::parse_error(
                    "Only views can have borders",
                ));
                continue;
            }
            if let Err(error) = parsed {
                let message = error.error.as_deref().unwrap_or_default();
                if message.starts_with("Expected 'border ") {
                    *error = swayward_ipc::command::parse_error("Only views can have borders");
                } else if message.starts_with("Expected 'resize ")
                    || message.starts_with("Invalid resize ")
                {
                    *error = swayward_ipc::command::parse_error("Cannot resize nothing");
                }
            }
        }
    }
    let mut retained_targets = None;
    let outcomes = parsed
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
        .collect();
    state.ipc_commit_workspace_transaction();
    outcomes
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
        if matches!(parsed.command, Command::Exit | Command::Reload) {
            let name = if matches!(parsed.command, Command::Exit) {
                "exit"
            } else {
                "reload"
            };
            return failure(format!("criteria are not supported for {name}"));
        }
        if let Command::SetLayoutOption(option) = &parsed.command {
            if criteria_global_setting(option) {
                for _ in targets {
                    let outcome = execute_global_setting(state, option);
                    if !outcome.success {
                        return outcome;
                    }
                }
                return success();
            }
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
        Command::Focus => return command_failure("No container to focus was specified."),
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
        Command::FocusNext => {
            if let Err(error) = focus::next_or_prev(state, true) {
                return error;
            }
            None
        }
        Command::FocusPrev => {
            if let Err(error) = focus::next_or_prev(state, false) {
                return error;
            }
            None
        }
        Command::FocusNextSibling => {
            focus::next_prev_sibling(state, true);
            None
        }
        Command::FocusPrevSibling => {
            focus::next_prev_sibling(state, false);
            None
        }
        Command::FocusFloating => match focus::mode(state, true) {
            Ok(action) => Some(action),
            Err(error) => return error,
        },
        Command::FocusTiling => match focus::mode(state, false) {
            Ok(action) => Some(action),
            Err(error) => return error,
        },
        Command::FocusModeToggle => {
            let floating = state
                .swayward
                .layout
                .active_workspace()
                .is_some_and(|workspace| workspace.floating_is_active());
            match focus::mode(state, !floating) {
                Ok(action) => Some(action),
                Err(error) => return error,
            }
        }
        Command::MoveDirection { direction, pixels } => {
            let Some(workspace) = state.swayward.layout.active_workspace() else {
                return failure("Cannot move workspaces in a direction");
            };
            let target = focused_target(state);
            if target.is_none()
                || matches!(target, Some(CommandTarget::Container(workspace, node))
                    if state.swayward.layout.is_tiling_root(workspace, node))
            {
                return command_failure("Cannot move workspaces in a direction");
            };
            let fullscreen_floating = workspace.active_floating_is_fullscreen();
            if workspace.floating_is_active() || fullscreen_floating {
                if fullscreen_floating {
                    return failure("Cannot move fullscreen floating container");
                }
                if state
                    .swayward
                    .layout
                    .focused_leaf_is_only_child_of_floating_tree_root()
                {
                    return success();
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
                let Some(target) = target else {
                    unreachable!();
                };
                let outcome = move_direction(
                    state,
                    target,
                    direction,
                    pixels,
                    crate::layout::ActivateWindow::Smart,
                    true,
                );
                if !outcome.success {
                    return outcome;
                }
                None
            }
        }
        Command::MovePosition(position) => {
            if let Err(error) = move_position(state, None, &position) {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::MoveToWorkspace {
            target,
            auto_back_and_forth,
        } => {
            if state.swayward.layout.global_fullscreen_active()
                && state
                    .swayward
                    .layout
                    .focused_window_is_fullscreen_or_child()
            {
                return failure("Can't move fullscreen global container");
            }
            let Some(focused) = focused_target(state) else {
                return command_failure("Can't move an empty workspace");
            };
            let auto_back_and_forth = auto_back_and_forth
                && state
                    .swayward
                    .config
                    .borrow()
                    .input
                    .workspace_auto_back_and_forth;
            let outcome =
                move_target_to_workspace(state, focused, target, false, auto_back_and_forth);
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
            let focused_target = focused_target(state);
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
            if let Some(CommandTarget::Container(workspace, node)) = focused_target {
                let outcome = move_tiling_subtree_to_output(state, workspace, node, &output);
                if !outcome.success {
                    return outcome;
                }
            } else {
                state.swayward.layout.move_to_output(
                    None,
                    &output,
                    None,
                    crate::layout::ActivateWindow::No,
                );
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::MoveWorkspaceToOutput(target) => {
            let outcome = move_workspace_to_output(state, None, &target);
            if !outcome.success {
                return outcome;
            }
            None
        }
        Command::MoveScratchpad => {
            let floating_root = state
                .swayward
                .layout
                .active_workspace()
                .and_then(crate::layout::workspace::Workspace::focused_floating_tree_root);
            let target = focused_target(state);
            if target.is_none() {
                return swayward_ipc::command::parse_error(
                    "Can't move an empty workspace to the scratchpad",
                );
            }
            let window = match target {
                Some(CommandTarget::Container(workspace, node)) => {
                    state.swayward.layout.window_in_node(workspace, node)
                }
                Some(CommandTarget::Window(_)) => None,
                None => unreachable!(),
            };
            state.ipc_order_scratchpad_events(crate::ipc::server::ScratchpadEventOrder::Hide);
            state.swayward.layout.move_to_scratchpad(window.as_ref());
            if let Some(root) = floating_root {
                state.ipc_refresh_layout();
                if let Some(server) = &state.swayward.ipc_server {
                    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
                        &state.swayward.layout,
                        &state.swayward.global_space,
                        &state.swayward.marks_by_window,
                        &state.swayward.marks_by_container,
                    ))
                    .unwrap_or_default();
                    if let Some(mut container) = crate::ipc::server::find_node_by_id(
                        &tree,
                        crate::ipc::tree::container_id(root),
                    )
                    .cloned()
                    {
                        container.as_object_mut().unwrap().remove("visible");
                        server.send_event(swayward_ipc::legacy::Event::SwayWindowChanged {
                            change: "move".into(),
                            container,
                        });
                    }
                }
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::ScratchpadShow => {
            if state.swayward.layout.scratchpad_is_empty() {
                return swayward_ipc::command::parse_error("Scratchpad is empty");
            }
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
        Command::Opacity(value) | Command::OpacityRelative(value) => {
            let Some(target) = focused_target(state) else {
                return failure("No current container");
            };
            let relative = matches!(parsed.command, Command::OpacityRelative(_));
            if let Err(error) = window::opacity(state, target, value, relative) {
                return error;
            }
            None
        }
        Command::TitleFormat(format) => {
            let Some(target) = focused_target(state) else {
                return failure("Only valid containers can have a title_format");
            };
            if let Err(error) = window::title_format(state, target, &format) {
                return error;
            }
            None
        }
        Command::ShortcutsInhibitor(enable) => {
            let Some(target) = focused_target(state) else {
                return failure("Only views can have shortcuts inhibitors");
            };
            if let Err(error) = set_shortcuts_inhibitor(state, target, enable) {
                return error;
            }
            None
        }
        Command::Sticky(value) => {
            let target = focused_target(state);
            let container_window = match target {
                Some(CommandTarget::Container(workspace, node)) => {
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
                return command_failure("No current container");
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
            let Some(target) = focused_target(state) else {
                return failure("Only views can have borders");
            };
            if let Err(error) = window::border(state, target, &border) {
                return error;
            }
            None
        }
        Command::Floating(mode) => {
            if let Some(CommandTarget::Container(workspace, node)) = focused_target(state) {
                let floating = match mode {
                    Toggle::Enable => true,
                    Toggle::Disable => false,
                    Toggle::Toggle => state
                        .swayward
                        .layout
                        .active_workspace()
                        .is_some_and(|workspace| workspace.contains_tiling_node(node)),
                };
                let Some(root) = state
                    .swayward
                    .layout
                    .set_container_floating(workspace, node, floating)
                else {
                    return failure("No matching node.");
                };
                state.ipc_refresh_layout();
                if let Some(server) = &state.swayward.ipc_server {
                    let tree = serde_json::to_value(crate::ipc::tree::describe_tree(
                        &state.swayward.layout,
                        &state.swayward.global_space,
                        &state.swayward.marks_by_window,
                        &state.swayward.marks_by_container,
                    ))
                    .unwrap_or_default();
                    if let Some(mut container) = crate::ipc::server::find_node_by_id(
                        &tree,
                        crate::ipc::tree::container_id(root),
                    )
                    .cloned()
                    {
                        if floating {
                            container["type"] = "floating_con".into();
                            container["floating"] = "user_on".into();
                        }
                        server.send_event(swayward_ipc::legacy::Event::SwayWindowChanged {
                            change: "floating".into(),
                            container,
                        });
                    }
                }
                state.swayward.queue_redraw_all();
                return success();
            }
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
                return swayward_ipc::command::parse_error("Can't float an empty workspace");
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
        Command::Urgent(value) => {
            let Some(target) = focused_target(state) else {
                return failure("No current container");
            };
            let CommandTarget::Window(target) = target else {
                return failure("Only views can be urgent");
            };
            // `target` came from `layout.focus()` through `focused_target`, and
            // no layout mutation occurs before this lookup. `Layout::windows`
            // includes every focus source, including interactive moves and the
            // scratchpad, so the focused ID must still be present here.
            let urgent = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, window)| (window.id() == target).then(|| window.is_urgent()))
                .expect("the focused window must be yielded by the same layout");
            let urgent = parse_boolean(&value, urgent);
            state.swayward.set_window_urgent(target, urgent);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Workspace {
            target,
            auto_back_and_forth,
        } => {
            if target != WorkspaceTarget::BackAndForth {
                // Sway completes focus changes synchronously. Finish a prior
                // render-only transition before resolving the next named or
                // numbered command so its inactive empty workspace is gone.
                state.swayward.layout.finish_sway_workspace_switch(&target);
            }
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
        Command::AssignWorkspace { target, outputs } => {
            if let Err(error) = state
                .swayward
                .layout
                .assign_sway_workspace(target, &outputs)
            {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::RenameWorkspace { old, new_name } => {
            if let Err(error) = state.swayward.layout.rename_sway_workspace(old, new_name) {
                return swayward_ipc::command::parse_error(error);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::Kill => {
            let workspace_windows =
                state
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
            } else if let Some(target) = focused_target(state) {
                if let Err(error) = window::kill(state, target) {
                    return error;
                }
            }
            None
        }
        Command::ResizeSet { width, height } => {
            let Some(target) = focused_target(state) else {
                return swayward_ipc::command::parse_error("Cannot resize nothing");
            };
            if let Err(error) = window::resize_set(state, target, width, height) {
                return error;
            }
            None
        }
        Command::Resize {
            grow,
            axis,
            first,
            second,
        } => {
            if state
                .swayward
                .layout
                .active_workspace()
                .is_some_and(|workspace| workspace.is_workspace_focused())
            {
                return swayward_ipc::command::parse_error("Cannot resize nothing");
            }
            let Some(target) = focused_target(state) else {
                return swayward_ipc::command::parse_error("Cannot resize nothing");
            };
            if let Err(error) = window::resize(state, target, grow, axis, first, second) {
                return error;
            }
            None
        }
        Command::Reload => {
            let Some(watcher) = &state.swayward.config_file_watcher else {
                return failure("config reload is not available without a config file watcher");
            };
            if !watcher.validate_config() {
                return failure("Error(s) reloading config.");
            }
            watcher.load_config(None);
            None
        }
        Command::Exit => {
            state.request_stop("exit");
            None
        }
        Command::CreateOutput => {
            let State { backend, swayward } = state;
            let headless = match backend {
                crate::backend::Backend::Headless(headless) => Some(headless),
                crate::backend::Backend::Tty(_) | crate::backend::Backend::Winit(_) => None,
            };
            let outcome = create_output(headless, swayward);
            if !outcome.success {
                return outcome;
            }
            None
        }
        Command::InputSwitchLayout { identifier, target } => {
            let matches_keyboard = state.swayward.ipc_input_devices.values().any(|device| {
                device.device_type == "keyboard"
                    && (matches!(identifier.as_str(), "*" | "type:keyboard")
                        || device.identifier == identifier)
            });
            if let (true, Some(keyboard)) = (matches_keyboard, state.swayward.seat.get_keyboard()) {
                keyboard.with_xkb_state(state, |mut context| match target {
                    XkbLayoutTarget::Next => context.cycle_next_layout(),
                    XkbLayoutTarget::Prev => context.cycle_prev_layout(),
                    XkbLayoutTarget::Index(index) => {
                        let count = context.xkb().lock().unwrap().layouts().count();
                        if (index as usize) < count {
                            context.set_layout(smithay::input::keyboard::Layout(index));
                        }
                    }
                });
                state.ipc_refresh_keyboard_layout_index();
            }
            None
        }
        Command::Output { target, actions } => {
            if actions
                .iter()
                .any(|action| matches!(action, swayward_ipc::OutputAction::Off))
            {
                state.ipc_suppress_workspace_moves();
            }
            let targets = if target == "*" {
                state
                    .swayward
                    .global_space
                    .outputs()
                    .map(|output| output.name())
                    .collect::<Vec<_>>()
            } else {
                vec![target]
            };
            let has_output_event = actions.iter().any(|action| {
                matches!(
                    action,
                    swayward_ipc::OutputAction::On
                        | swayward_ipc::OutputAction::Off
                        | swayward_ipc::OutputAction::Power { .. }
                )
            });
            for target in targets {
                for action in &actions {
                    if let swayward_ipc::OutputAction::Power { power } = action {
                        let output = state.swayward.output_by_name_match(&target).cloned();
                        if *power == Toggle::Toggle && output.is_none() {
                            return failure(format!(
                                "Cannot apply toggle to unknown output {target}"
                            ));
                        }
                        let target = output
                            .as_ref()
                            .map_or_else(|| target.clone(), |output| output.name());
                        let current = state
                            .swayward
                            .output_power
                            .get(&target)
                            .copied()
                            .unwrap_or(true);
                        let enabled = match power {
                            Toggle::Enable => true,
                            Toggle::Disable => false,
                            Toggle::Toggle => !current,
                        };
                        state.swayward.output_power.insert(target, enabled);
                        if let Some(output) = output {
                            state.backend.set_output_power(&output, enabled);
                            if enabled {
                                state.swayward.queue_redraw(&output);
                            }
                        }
                    }
                }
                let config_actions = actions
                    .iter()
                    .filter(|action| !matches!(action, swayward_ipc::OutputAction::Power { .. }))
                    .cloned()
                    .collect::<Vec<_>>();
                if !config_actions.is_empty() {
                    state.apply_transient_output_config(&target, &config_actions);
                }
            }
            if has_output_event {
                state.swayward.ipc_output_changed();
            }
            None
        }
        Command::SetClientColors { class, colors } => {
            set_client_colors(state, class, colors);
            None
        }
        Command::SetLayoutOption(option) => return execute_global_setting(state, &option),
        Command::Gaps {
            inner,
            sides,
            all,
            operation,
            amount,
        } => {
            if all {
                for workspace in state.swayward.layout.workspaces_mut() {
                    workspace.update_gaps(inner, sides, operation, amount);
                }
            } else if let Some(workspace) = state.swayward.layout.active_workspace_mut() {
                workspace.update_gaps(inner, sides, operation, amount);
            }
            state.swayward.queue_redraw_all();
            None
        }
        Command::GapsDefaults {
            inner,
            sides,
            amount,
        } => {
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
                    // Sway clamps a negative outer gap to -inner so windows
                    // cannot leave the workspace (`gaps.c:30-43`).
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
            // Re-apply through the config path so a later workspace picks the
            // new default up, exactly as the other live config settings do.
            let config = state.swayward.config.clone();
            state.swayward.layout.update_config(&config.borrow());
            state.swayward.queue_redraw_all();
            None
        }
        Command::WorkspaceGaps {
            name,
            inner,
            sides,
            amount,
        } => {
            // A per-workspace-name default. Sway stores it on the workspace
            // CONFIG and applies it when a workspace of that name is created
            // (`sway/sway/commands/workspace.c:57-117`;
            // `sway/sway/tree/workspace.c:224-242`), so like sway this does not
            // retroactively change a workspace that already exists.
            {
                let mut config = state.swayward.config.borrow_mut();
                let entry = match config
                    .workspaces
                    .iter_mut()
                    .find(|ws| ws.name.0.eq_ignore_ascii_case(&name))
                {
                    Some(entry) => entry,
                    None => {
                        config.workspaces.push(swayward_config::Workspace {
                            name: swayward_config::workspace::WorkspaceName(name.clone()),
                            sway_output_assignment: None,
                            open_on_output: None,
                            layout: None,
                        });
                        config.workspaces.last_mut().unwrap()
                    }
                };
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
            None
        }
        Command::Mode {
            name,
            pango_markup,
            subcommand,
        } => {
            if let Some(subcommand) = subcommand {
                let mut config = state.swayward.config.borrow_mut();
                if name != "default" && !config.binding_modes.iter().any(|mode| mode.name == name) {
                    config.binding_modes.push(swayward_config::BindingMode {
                        name: name.clone(),
                        pango_markup,
                        binds: Default::default(),
                    });
                }
                drop(config);
                // Sway points `config->current_mode` at the named mode, runs
                // the same handler the top level would, then restores the
                // previous mode (`sway/sway/commands/mode.c:69-84`), so a
                // nested bind never switches modes.
                match *subcommand {
                    Command::Set { name, value } => {
                        set_variable(&mut state.swayward.sway_variables, name, value);
                    }
                    Command::Bind {
                        key,
                        command,
                        keycode,
                        release,
                        locked,
                        inhibited,
                        no_repeat,
                        input_device,
                    } => {
                        if let Err(error) = mutate_key_binding(
                            state,
                            BindingMutation {
                                mode: &name,
                                key: &key,
                                command,
                                keycode,
                                release,
                                locked,
                                inhibited,
                                no_repeat,
                                input_device,
                            },
                        ) {
                            return failure(error);
                        }
                    }
                    Command::SwitchBind {
                        switch,
                        command,
                        locked,
                    } => {
                        if let Err(error) =
                            mutate_switch_binding(state, &name, &switch, command, locked)
                        {
                            return failure(error);
                        }
                    }
                    _ => unreachable!("mode parser only admits reachable subcommands"),
                }
                None
            } else {
                let pango_markup = if name == "default" {
                    false
                } else {
                    let pango_markup = state
                        .swayward
                        .config
                        .borrow()
                        .binding_modes
                        .iter()
                        .find(|mode| mode.name == name)
                        .map(|mode| mode.pango_markup);
                    let Some(pango_markup) = pango_markup else {
                        return failure(format!("Unknown mode `{name}'"));
                    };
                    pango_markup
                };
                state.swayward.binding_mode = name.clone();
                if let Some(server) = &state.swayward.ipc_server {
                    server.send_event(swayward_ipc::legacy::Event::BindingModeChanged {
                        mode: name,
                        pango_markup,
                    });
                }
                None
            }
        }
        Command::Set { name, value } => {
            // Sway replaces an existing value in place and keeps the list
            // sorted longest name first, so a longer name is never shadowed by
            // a shorter prefix of itself (`sway/sway/commands/set.c:36-55`).
            set_variable(&mut state.swayward.sway_variables, name, value);
            None
        }
        Command::Bind {
            key,
            command,
            keycode,
            release,
            locked,
            inhibited,
            no_repeat,
            input_device,
        } => {
            let mode = state.swayward.binding_mode.clone();
            if let Err(error) = mutate_key_binding(
                state,
                BindingMutation {
                    mode: &mode,
                    key: &key,
                    command,
                    keycode,
                    release,
                    locked,
                    inhibited,
                    no_repeat,
                    input_device,
                },
            ) {
                return failure(error);
            }
            None
        }
        Command::SwitchBind {
            switch,
            command,
            locked,
        } => {
            let mode = state.swayward.binding_mode.clone();
            if let Err(error) = mutate_switch_binding(state, &mode, &switch, command, locked) {
                return failure(error);
            }
            None
        }
        Command::Nop => None,
        Command::Exec {
            command,
            no_startup_id,
        } => {
            let (token, _) = state.swayward.activation_state.create_external_token(None);
            if no_startup_id {
                spawn_sh_without_startup_id(command, Some(token.clone()));
            } else {
                spawn_sh(command, Some(token.clone()));
            }
            None
        }
        Command::Mark {
            add,
            toggle,
            identifier,
        } => {
            if state
                .swayward
                .layout
                .active_workspace()
                .is_some_and(|workspace| workspace.is_workspace_focused())
            {
                return swayward_ipc::command::parse_error("Only containers can have marks");
            }
            let Some(target) = focused_target(state) else {
                return swayward_ipc::command::parse_error("Only containers can have marks");
            };
            mark_target(state, target, &identifier, add, toggle);
            None
        }
        Command::Unmark(identifier) => {
            unmark_globally(state, identifier.as_deref());
            None
        }
        Command::Assign { criteria, target } => {
            let parsed = match criteria::Criteria::parse(&criteria, focused_con_id(state)) {
                Ok(criteria) => criteria,
                Err(error) => return failure(error),
            };
            state
                .swayward
                .runtime_window_rules
                .push(crate::swayward::RuntimeWindowRule::Assign(parsed, target));
            None
        }
        Command::NoFocus { criteria } => {
            let parsed = match criteria::Criteria::parse(&criteria, focused_con_id(state)) {
                Ok(criteria) => criteria,
                Err(error) => return failure(error),
            };
            if !state.swayward.runtime_window_rules.iter().any(|rule| {
                matches!(rule, crate::swayward::RuntimeWindowRule::NoFocus(raw, _) if raw == &criteria)
            }) {
                state
                    .swayward
                    .runtime_window_rules
                    .push(crate::swayward::RuntimeWindowRule::NoFocus(criteria, parsed));
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
                state
                    .swayward
                    .runtime_for_window
                    .insert((criteria.clone(), command.clone()));
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

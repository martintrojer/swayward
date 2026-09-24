use std::time::Duration;

use swayward_config::Action;
pub use swayward_ipc::command::{
    parse, parse_boolean, AssignmentTarget, BorderStyle, ClientColorClass, Command, Direction,
    Layout, LayoutToggle, LayoutToggleEntry, MovePosition, OutputTarget, ParsedCommand,
    ResizeAmount, ResizeAxis, ResizeUnit, SwapTarget, Toggle, WorkspaceTarget, XkbLayoutTarget,
};
use swayward_ipc::command::{parse_with_variables, set_variable};
use swayward_ipc::legacy::PositionChange;
use swayward_ipc::{criteria, CommandOutcome};

use crate::swayward::State;
use crate::utils::spawning::{spawn_sh, spawn_sh_without_startup_id};
use crate::window::mapped::ShortcutsInhibitPolicy;

mod focus;
mod layout;
mod movement;
mod scratchpad;
mod window;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandTarget {
    Window(crate::window::mapped::MappedId),
    Container(
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
}

pub fn execute(state: &mut State, input: &str) -> Vec<CommandOutcome> {
    // Sway expands variables before dispatch, for every argument except the
    // name being defined by `set` (`sway/sway/commands.c:283-285`). This is the
    // single choke point for both IPC commands and key bindings, matching
    // sway, where a binding re-enters execute_command at press time
    // (`sway/sway/commands/bind.c:635`).
    let mut parsed = parse_with_variables(input, &state.swayward.sway_variables);
    if state.swayward.layout.focus().is_none()
        && input
            .split_whitespace()
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("resize"))
        && parsed.first().is_some_and(Result::is_err)
    {
        parsed[0] = Err(swayward_ipc::command::parse_error("Cannot resize nothing"));
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
        Command::Focus => None,
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
            if focused_target(state).is_none() {
                return command_failure("Cannot move workspaces in a direction");
            };
            let fullscreen_floating = workspace.active_floating_is_fullscreen();
            if workspace.floating_is_active() || fullscreen_floating {
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
                let Some(target) = focused_target(state) else {
                    return success();
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
            let target = focused_target(state);
            if target.is_none() {
                return swayward_ipc::command::parse_error(
                    "Can't move an empty workspace to the scratchpad",
                );
            }
            if matches!(target, Some(CommandTarget::Container(_, _))) {
                return failure("floating container groups are not supported");
            }
            scratchpad::move_focused(state);
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
            if matches!(focused_target(state), Some(CommandTarget::Container(_, _))) {
                return failure("floating container groups are not supported");
            }
            let Some(window) = state
                .swayward
                .layout
                .focus()
                .map(|mapped| mapped.window.clone())
            else {
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
            if matches!(focused_target(state), Some(CommandTarget::Container(_, _))) {
                return if mode == Toggle::Disable {
                    success()
                } else {
                    failure("floating container groups are not supported")
                };
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
            let urgent = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, window)| (window.id() == target).then(|| window.is_urgent()))
                .expect("focused window must remain in the layout");
            let urgent = parse_boolean(&value, urgent);
            state.swayward.set_window_urgent(target, urgent);
            state.swayward.queue_redraw_all();
            None
        }
        Command::Workspace {
            target,
            auto_back_and_forth,
        } => {
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
                return failure(error);
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
            let has_power_action = actions
                .iter()
                .any(|action| matches!(action, swayward_ipc::OutputAction::Power { .. }));
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
            if has_power_action {
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
                            &name,
                            &key,
                            command,
                            keycode,
                            release,
                            locked,
                            inhibited,
                            no_repeat,
                            input_device,
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
                &mode,
                &key,
                command,
                keycode,
                release,
                locked,
                inhibited,
                no_repeat,
                input_device,
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

use movement::{
    move_position, move_target_to_mark, move_target_to_workspace, move_tiling_subtree_to_output,
    move_workspace_to_output, output_target, output_target_by_name_or_direction,
};

fn move_target_to_adjacent_output(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
    activate: crate::layout::ActivateWindow,
) {
    let (reference, window) = match target {
        CommandTarget::Window(target) => {
            let Some(found) = state
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
                })
            else {
                return;
            };
            found
        }
        CommandTarget::Container(workspace, _) => {
            let Some((_, workspace)) = state.swayward.layout.find_workspace_by_id(workspace) else {
                return;
            };
            let Some(window) = workspace
                .active_window()
                .map(|mapped| mapped.window.clone())
            else {
                return;
            };
            let reference = state
                .swayward
                .layout
                .windows()
                .find_map(|(monitor, mapped)| {
                    (mapped.window == window)
                        .then(|| monitor.map(|monitor| monitor.output()))
                        .flatten()
                });
            (reference, window)
        }
    };
    let destination = OutputTarget::Direction(direction);
    let reference_point = state.swayward.layout.window_center(&window);
    if let Ok(output) = output_target(state, &destination, reference, reference_point) {
        match target {
            CommandTarget::Window(_) => {
                state
                    .swayward
                    .layout
                    .move_to_output(Some(&window), &output, None, activate)
            }
            CommandTarget::Container(workspace, node) => {
                let _ = move_tiling_subtree_to_output(state, workspace, node, &output);
            }
        }
    }
}

fn move_direction(
    state: &mut State,
    target: CommandTarget,
    direction: Direction,
    pixels: Option<i32>,
    activate: crate::layout::ActivateWindow,
    focused: bool,
) -> CommandOutcome {
    if matches!(
        target,
        CommandTarget::Window(target)
            if state.swayward.layout.windows().any(|(_, mapped)| {
                mapped.id() == target
                    && state.swayward.layout.fullscreen_mode(&mapped.window)
                        == Some(crate::layout::tiling_tree::FullscreenMode::Global)
            })
    ) {
        return success();
    }
    let layout_direction = match direction {
        Direction::Left => crate::layout::tiling_tree::Direction::Left,
        Direction::Right => crate::layout::tiling_tree::Direction::Right,
        Direction::Up => crate::layout::tiling_tree::Direction::Up,
        Direction::Down => crate::layout::tiling_tree::Direction::Down,
    };
    let moved_within_workspace = if focused {
        let moved = match target {
            CommandTarget::Container(workspace, node) => state
                .swayward
                .layout
                .move_tiling_node_in_direction(workspace, node, layout_direction),
            CommandTarget::Window(_) => match direction {
                Direction::Left => state.swayward.layout.move_left(),
                Direction::Right => state.swayward.layout.move_right(),
                Direction::Up => state.swayward.layout.move_up(),
                Direction::Down => state.swayward.layout.move_down(),
            },
        };
        if !moved && state.swayward.layout.focused_fullscreen_mode().is_none() {
            move_target_to_adjacent_output(state, target, direction, activate);
        }
        moved
    } else {
        match target {
            CommandTarget::Window(target) => {
                let window =
                    state.swayward.layout.windows().find_map(|(_, mapped)| {
                        (mapped.id() == target).then(|| mapped.window.clone())
                    });
                let Some(window) = window else {
                    return failure("No matching node.");
                };
                let moved = state.swayward.layout.move_window_in_direction(
                    &window,
                    layout_direction,
                    f64::from(pixels.unwrap_or(10)),
                );
                if !moved {
                    move_target_to_adjacent_output(
                        state,
                        CommandTarget::Window(target),
                        direction,
                        activate,
                    );
                }
                moved
            }
            CommandTarget::Container(workspace, node) => state
                .swayward
                .layout
                .move_tiling_node_in_direction(workspace, node, layout_direction),
        }
    };
    state.swayward.queue_redraw_all();
    if moved_within_workspace && focused {
        if let CommandTarget::Window(window) = target {
            state.ipc_refresh_layout();
            if let Some(server) = &state.swayward.ipc_server {
                server.send_event(swayward_ipc::legacy::Event::WindowMoved {
                    id: crate::ipc::tree::window_id(window),
                });
            }
        }
    }
    success()
}

fn set_client_colors(
    state: &mut State,
    class: ClientColorClass,
    colors: swayward_ipc::command::ClientColors,
) {
    let colors = swayward_config::TitlebarColors {
        border_color: swayward_config::Color::from_rgba8_unpremul(
            colors.border[0],
            colors.border[1],
            colors.border[2],
            colors.border[3],
        ),
        background_color: swayward_config::Color::from_rgba8_unpremul(
            colors.background[0],
            colors.background[1],
            colors.background[2],
            colors.background[3],
        ),
        text_color: swayward_config::Color::from_rgba8_unpremul(
            colors.text[0],
            colors.text[1],
            colors.text[2],
            colors.text[3],
        ),
    };
    let mut config = state.swayward.config.borrow_mut();
    let titlebar = &mut config.layout.titlebar;
    *match class {
        ClientColorClass::Focused => &mut titlebar.focused,
        ClientColorClass::FocusedInactive => &mut titlebar.focused_inactive,
        ClientColorClass::FocusedTabTitle => &mut titlebar.focused_tab_title,
        ClientColorClass::Unfocused => &mut titlebar.unfocused,
        ClientColorClass::Urgent => &mut titlebar.urgent,
    } = colors;
    state.swayward.layout.update_config(&config);
    drop(config);
    state.swayward.queue_redraw_all();
}

fn execute_targeted(state: &mut State, command: &Command, target: CommandTarget) -> CommandOutcome {
    match command {
        Command::Mark {
            add,
            toggle,
            identifier,
        } => mark_target(state, target, identifier, *add, *toggle),
        Command::Unmark(identifier) => unmark_target(state, target, identifier.as_deref()),
        Command::Swap(swap_target) => {
            let outcome = movement::swap_target(state, target, swap_target);
            if !outcome.success {
                return outcome;
            }
        }
        Command::MoveDirection { direction, pixels } => {
            let outcome = move_direction(
                state,
                target,
                *direction,
                *pixels,
                crate::layout::ActivateWindow::No,
                false,
            );
            if !outcome.success {
                return outcome;
            }
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
        Command::MoveToWorkspace {
            target: workspace_target,
            auto_back_and_forth,
        } => {
            let auto_back_and_forth = *auto_back_and_forth
                && state
                    .swayward
                    .config
                    .borrow()
                    .input
                    .workspace_auto_back_and_forth;
            let outcome = move_target_to_workspace(
                state,
                target,
                workspace_target.clone(),
                true,
                auto_back_and_forth,
            );
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
        Command::MoveWorkspaceToOutput(output_target_name) => {
            let outcome = move_workspace_to_output(state, Some(target), output_target_name);
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
                crate::layout::ActivateWindow::No,
            );
            state.swayward.queue_redraw_all();
        }
        Command::MoveScratchpad => {
            if let Err(error) = scratchpad::move_targeted(state, target) {
                return error;
            }
        }
        Command::ScratchpadShow => {
            if let Err(error) = scratchpad::show_targeted(state, target) {
                return error;
            }
        }
        Command::Fullscreen { mode, global } => {
            if let Err(error) = layout::fullscreen_targeted(state, target, *mode, *global) {
                return error;
            }
        }
        Command::ShortcutsInhibitor(enable) => {
            if let Err(error) = set_shortcuts_inhibitor(state, target, *enable) {
                return error;
            }
        }
        Command::Sticky(value) => {
            if let Err(error) = window::sticky(state, target, value) {
                return error;
            }
        }
        Command::SetClientColors { class, colors } => {
            set_client_colors(state, *class, *colors);
        }
        Command::SetLayoutOption(_) => return failure("command cannot be applied to a container"),
        Command::TitleFormat(format) => {
            if let Err(error) = window::title_format(state, target, format) {
                return error;
            }
        }
        Command::Border(border) => {
            if let Err(error) = window::border(state, target, border) {
                return error;
            }
        }
        Command::Floating(mode) => {
            if let Err(error) = window::floating(state, target, mode) {
                return error;
            }
            // Re-run this window's `for_window` commands now that its float
            // state has changed, so a `tiling` or `floating` criterion is
            // re-evaluated rather than only being applied at map time. i3
            // encodes the same idea through its tiling_from and floating_from
            // provenance criteria.
            if let CommandTarget::Window(window) = target {
                rerun_for_window_rules(state, window);
            }
        }
        Command::Urgent(value) => {
            let CommandTarget::Window(target) = target else {
                return failure("Only views can be urgent");
            };
            let urgent = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, window)| (window.id() == target).then(|| window.is_urgent()));
            let Some(urgent) = urgent else {
                return failure("No matching node.");
            };
            let urgent = parse_boolean(value, urgent);
            state.swayward.set_window_urgent(target, urgent);
            state.swayward.queue_redraw_all();
        }
        Command::Kill => {
            if let Err(error) = window::kill(state, target) {
                return error;
            }
        }
        Command::ResizeSet { width, height } => {
            if let Err(error) = window::resize_set(state, target, *width, *height) {
                return error;
            }
        }
        Command::Resize {
            grow,
            axis,
            first,
            second,
        } => {
            if let Err(error) = window::resize(state, target, *grow, *axis, *first, *second) {
                return error;
            }
        }
        Command::Focus => {
            if let Err(error) = focus::targeted(state, target) {
                return error;
            }
        }
        Command::FocusWorkspace => {
            if let Err(error) = focus::targeted_workspace(state, target) {
                return error;
            }
        }
        Command::FocusDirection(direction) => {
            if let Err(error) = focus::targeted_direction(state, target, *direction) {
                return error;
            }
        }
        Command::FocusOutput(identifier) => {
            if let Err(error) = focus::output(state, identifier) {
                return error;
            }
        }
        Command::Layout(value) => {
            if let Err(error) = layout::targeted(state, target, *value) {
                return error;
            }
        }
        Command::LayoutToggle(toggle) => {
            if let Err(error) = layout::toggle_targeted(state, target, toggle) {
                return error;
            }
        }
        Command::LayoutDefault => {
            if let Err(error) = layout::default_targeted(state, target) {
                return error;
            }
        }
        Command::Split(value) => {
            if let Err(error) = layout::split_targeted(state, target, *value) {
                return error;
            }
        }
        Command::RenameWorkspace { old, new_name } => {
            // Only the `rename workspace to <new>` form reads the matched
            // container's workspace. The `<old>` and `number <n>` forms resolve
            // by name regardless of criteria, and sway still runs the handler
            // once per match, so the second pass finds the old name gone and
            // fails (`sway/sway/commands/rename.c:35-58`).
            let resolved = match old {
                Some(target) => state
                    .swayward
                    .layout
                    .rename_sway_workspace(Some(target.clone()), new_name.clone()),
                None => match target_workspace(state, target) {
                    Some(workspace) => state
                        .swayward
                        .layout
                        .rename_sway_workspace_by_id(workspace, new_name.clone()),
                    // Sway's NULL workspace lands on the same message, because
                    // `!workspace` is the branch that reports it
                    // (`sway/sway/commands/rename.c:60-63`).
                    None => Err("There is no workspace with that name".to_owned()),
                },
            };
            if let Err(error) = resolved {
                return failure(error);
            }
            state.swayward.queue_redraw_all();
        }
        Command::Nop => {}
        _ => return failure("criteria targets are not implemented for this command yet"),
    }
    state.ipc_refresh_layout();
    success()
}

/// The workspace a criteria-matched target lives on.
///
/// Sway sets `handler_context.workspace` from the matched node before running
/// the handler, taking a container's `pending.workspace`
/// (`sway/sway/commands.c:181-202`). A hidden scratchpad container has a NULL
/// workspace there (`sway/sway/tree/container.c:1458`), so it resolves to
/// nothing rather than to the focused workspace.
fn target_workspace(
    state: &State,
    target: CommandTarget,
) -> Option<crate::layout::workspace::WorkspaceId> {
    match target {
        CommandTarget::Container(workspace, _) => Some(workspace),
        CommandTarget::Window(id) => state.swayward.layout.windows().find_map(|(_, mapped)| {
            (mapped.id() == id)
                .then(|| state.swayward.layout.window_workspace_id(&mapped.window))
                .flatten()
        }),
    }
}

fn tiling_target(
    state: &State,
    target: CommandTarget,
    floating_error: &str,
) -> Result<
    (
        crate::layout::workspace::WorkspaceId,
        crate::layout::tiling_tree::NodeId,
    ),
    CommandOutcome,
> {
    match target {
        CommandTarget::Container(workspace, node) => Ok((workspace, node)),
        CommandTarget::Window(window) => {
            let window = state
                .swayward
                .layout
                .windows()
                .find_map(|(_, mapped)| (mapped.id() == window).then(|| mapped.window.clone()))
                .ok_or_else(|| failure("No matching node."))?;
            state
                .swayward
                .layout
                .tiling_target_for_window(&window)
                .ok_or_else(|| failure(floating_error))
        }
    }
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
    if !toggle || !had_mark {
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
    refresh_titlebar_marks(state);
    if let CommandTarget::Window(window) = target {
        // Sway rechecks only command criteria for the marked view here;
        // `view_execute_criteria` skips rules that this view already ran.
        run_for_window(state, window);
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

fn refresh_titlebar_marks(state: &mut State) {
    let marks = state.swayward.marks_by_window.clone();
    state.swayward.layout.with_windows_mut(|mapped, _| {
        mapped.set_titlebar_marks(marks.get(&mapped.id()).cloned().unwrap_or_default());
    });
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
    refresh_titlebar_marks(state);
}

fn set_shortcuts_inhibitor(
    state: &mut State,
    target: CommandTarget,
    enable: bool,
) -> Result<(), CommandOutcome> {
    let CommandTarget::Window(target) = target else {
        return Err(failure("Only views can have shortcuts inhibitors"));
    };
    let mut surface = None;
    state.swayward.layout.with_windows_mut(|window, _| {
        if window.id() == target {
            window.set_shortcuts_inhibit_policy(if enable {
                ShortcutsInhibitPolicy::Enable
            } else {
                ShortcutsInhibitPolicy::Disable
            });
            surface = Some(window.toplevel().wl_surface().clone());
        }
    });
    let surface = surface.ok_or_else(|| failure("No matching node."))?;
    if !enable {
        if let Some(inhibitor) = state
            .swayward
            .keyboard_shortcuts_inhibiting_surfaces
            .get(&surface)
        {
            inhibitor.inactivate();
        }
    }
    Ok(())
}

fn focused_id(state: &State) -> Option<crate::window::mapped::MappedId> {
    state.swayward.layout.focus().map(|mapped| mapped.id())
}

fn focused_con_id(state: &State) -> Option<u64> {
    match focused_target(state)? {
        CommandTarget::Container(_, node) => Some(crate::ipc::tree::container_id(node) as u64),
        CommandTarget::Window(window) => Some(crate::ipc::tree::window_id(window) as u64),
    }
}

type WindowSnapshot = (
    crate::window::mapped::MappedId,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
    Option<Duration>,
    Option<i32>,
    Option<crate::swayward::SecurityContextMetadata>,
    Option<std::sync::Arc<str>>,
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
        floating: snapshot.4,
        urgent_since: snapshot.5,
        workspace: snapshot.3.as_deref(),
        pid: snapshot.6.and_then(|pid| u32::try_from(pid).ok()),
        sandbox_engine: snapshot
            .7
            .as_ref()
            .and_then(|context| context.sandbox_engine.as_deref()),
        sandbox_app_id: snapshot
            .7
            .as_ref()
            .and_then(|context| context.app_id.as_deref()),
        sandbox_instance_id: snapshot
            .7
            .as_ref()
            .and_then(|context| context.instance_id.as_deref()),
        tag: snapshot.8.as_deref(),
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
                mapped.urgent_since(),
                mapped.credentials().map(|c| c.pid),
                mapped.security_context().cloned(),
                mapped.tag(),
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
    if let Some(order) = criteria.urgent() {
        targets.sort_by_key(|target| {
            let CommandTarget::Window(id) = target else {
                return None;
            };
            snapshots
                .iter()
                .find(|snapshot| snapshot.0 == *id)
                .and_then(|snapshot| snapshot.5)
        });
        if matches!(order, criteria::Urgent::Latest) {
            targets.reverse();
        }
        targets.truncate(1);
    }
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

/// Re-resolve and re-run the window rules for one window.
///
/// Called when a window's float state changes, so criteria that test that state
/// see the new value.
fn rerun_for_window_rules(state: &mut State, id: crate::window::mapped::MappedId) {
    let commands = {
        let config = state.swayward.config.borrow();
        let rules = &config.window_rules;
        state
            .swayward
            .layout
            .windows()
            .find(|(_, mapped)| mapped.id() == id)
            .map(|(_, mapped)| {
                crate::window::ResolvedWindowRules::compute(
                    rules,
                    crate::window::WindowRef::Mapped(mapped),
                    false,
                )
                .sway_for_window_commands
            })
            .unwrap_or_default()
    };
    for command in commands {
        let targeted = format!("[con_id={}] {command}", crate::ipc::tree::window_id(id));
        let _ = execute(state, &targeted);
    }
}

/// Execute newly matching runtime `for_window` criteria once for this window.
///
/// Sway records a criterion before executing its command, which also prevents a
/// mark-producing rule from recursively executing itself.
pub fn run_for_window(state: &mut State, id: crate::window::mapped::MappedId) {
    let commands = state
        .swayward
        .for_window
        .iter()
        .filter_map(|(raw, command, criteria)| {
            let key = (id, raw.clone(), command.clone());
            (!state.swayward.executed_for_window.contains(&key)
                && matching_ids(state, criteria).contains(&id))
            .then_some(key)
        })
        .collect::<Vec<_>>();
    for (id, raw, command) in commands {
        let targeted = format!("[con_id={}] {command}", crate::ipc::tree::window_id(id));
        state
            .swayward
            .executed_for_window
            .insert((id, raw, command));
        let _ = execute(state, &targeted);
    }
}

fn criteria_global_setting(option: &swayward_ipc::command::LayoutOption) -> bool {
    use swayward_ipc::command::LayoutOption;

    matches!(
        option,
        LayoutOption::FloatingMinimumSize(..)
            | LayoutOption::FloatingMaximumSize(..)
            | LayoutOption::FocusWrapping(..)
            | LayoutOption::ForceFocusWrapping(..)
            | LayoutOption::PopupDuringFullscreen(..)
            | LayoutOption::SmartBorders(..)
            | LayoutOption::SmartGaps(..)
            | LayoutOption::ShowMarks(..)
            | LayoutOption::TitleAlignment(..)
            | LayoutOption::TilingDrag(..)
            | LayoutOption::TilingDragThreshold(..)
            | LayoutOption::ForceDisplayUrgencyHint(..)
            | LayoutOption::PrimarySelection(..)
            | LayoutOption::FocusOnWindowActivation(..)
            | LayoutOption::WorkspaceAutoBackAndForth(..)
    )
}

#[cfg(test)]
std::thread_local! {
    static GLOBAL_SETTING_EXECUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_global_setting_executions() {
    GLOBAL_SETTING_EXECUTIONS.set(0);
}

#[cfg(test)]
pub(crate) fn global_setting_executions() -> usize {
    GLOBAL_SETTING_EXECUTIONS.get()
}

fn mutate_switch_binding(
    state: &mut State,
    mode: &str,
    combo: &str,
    command: Option<String>,
    locked: bool,
) -> Result<(), String> {
    let Some((switch, trigger)) = combo.split_once(':') else {
        return Err("Expected switch binding in '<switch>:<state>' form".into());
    };
    let switch = match switch {
        "lid" => smithay::backend::input::Switch::Lid,
        "tablet" => smithay::backend::input::Switch::TabletMode,
        _ => unreachable!("parser validated switch"),
    };
    let trigger = match trigger {
        "on" => Some(smithay::backend::input::SwitchState::On),
        "off" => Some(smithay::backend::input::SwitchState::Off),
        "toggle" => None,
        _ => unreachable!("parser validated switch state"),
    };
    let mode = mode.to_owned();
    if mode == "default" {
        let config = state.swayward.config.borrow();
        let file_binding_exists = match (switch, trigger) {
            (
                smithay::backend::input::Switch::Lid,
                Some(smithay::backend::input::SwitchState::On),
            ) => config.switch_events.lid_close.is_some(),
            (
                smithay::backend::input::Switch::Lid,
                Some(smithay::backend::input::SwitchState::Off),
            ) => config.switch_events.lid_open.is_some(),
            (
                smithay::backend::input::Switch::TabletMode,
                Some(smithay::backend::input::SwitchState::On),
            ) => config.switch_events.tablet_mode_on.is_some(),
            (
                smithay::backend::input::Switch::TabletMode,
                Some(smithay::backend::input::SwitchState::Off),
            ) => config.switch_events.tablet_mode_off.is_some(),
            _ => false,
        };
        if file_binding_exists {
            return Err(
                "runtime switch binding conflicts with a narrower KDL switch-event binding".into(),
            );
        }
    }
    let bindings = &mut state.swayward.runtime_switch_bindings;
    let existing = bindings.iter().position(|binding| {
        binding.mode == mode
            && binding.switch == switch
            && binding.state == trigger
            && binding.locked == locked
    });
    if let Some(command) = command {
        let binding = crate::swayward::RuntimeSwitchBinding {
            mode,
            switch,
            state: trigger,
            locked,
            command,
        };
        if let Some(index) = existing {
            bindings[index] = binding;
        } else {
            bindings.push(binding);
        }
    } else if let Some(index) = existing {
        bindings.remove(index);
    } else {
        return Err(format!("Could not find switch binding `{combo}`"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn mutate_key_binding(
    state: &mut State,
    mode: &str,
    key: &str,
    command: Option<String>,
    keycode: bool,
    release: bool,
    locked: bool,
    inhibited: bool,
    no_repeat: bool,
    input_device: String,
) -> Result<(), String> {
    let keycombo = key.to_owned();
    let key = if keycode {
        let (modifiers, code) = key
            .rsplit_once('+')
            .map_or(("", key), |(mods, code)| (mods, code));
        let code: u32 = code
            .parse()
            .map_err(|_| format!("Invalid keycode '{code}'"))?;
        if !(8..=255).contains(&code) {
            return Err(format!("Invalid keycode '{code}'"));
        }
        if modifiers.is_empty() {
            format!("code:{code}")
        } else {
            format!("{modifiers}+code:{code}")
        }
    } else {
        key.to_owned()
    };
    let key = key
        .parse::<swayward_config::Key>()
        .map_err(|error| error.to_string())?;
    if !matches!(
        key.trigger,
        swayward_config::Trigger::Keysym(_) | swayward_config::Trigger::Keycode(_)
    ) {
        return Err("runtime mouse bindings require exact pointer-region semantics".into());
    }
    let identity_matches = |bind: &swayward_config::Bind| {
        bind.key == key
            && bind.input_device == input_device
            && bind.release == release
            && bind.allow_when_locked == locked
            && bind.allow_inhibiting != inhibited
            && bind.group.is_none()
            && bind.mouse_regions.is_empty()
    };

    let binding_mode = mode;
    let mut config = state.swayward.config.borrow_mut();
    let binds = if binding_mode == "default" {
        &mut config.binds.0
    } else {
        &mut config
            .binding_modes
            .iter_mut()
            .find(|mode| mode.name == binding_mode)
            .ok_or_else(|| format!("Unknown binding mode '{binding_mode}'"))?
            .binds
            .0
    };
    let existing = binds.iter().position(identity_matches);
    if let Some(command) = command {
        let bind = swayward_config::Bind {
            key,
            action: Action::SwayCommand(command),
            mouse_regions: swayward_config::MouseRegions::empty(),
            input_device,
            group: None,
            release,
            repeat: !release && !no_repeat,
            cooldown: None,
            allow_when_locked: locked,
            allow_inhibiting: !inhibited,
            hotkey_overlay_title: None,
        };
        if let Some(index) = existing {
            // Sway overwrites an equal binding in place (`binding_upsert`,
            // sway/sway/commands/bind.c:260-278).
            binds[index] = bind;
        } else {
            binds.push(bind);
        }
    } else if let Some(index) = existing {
        binds.remove(index);
    } else {
        return Err(format!(
            "Could not find binding `{keycombo}` for the given flags"
        ));
    }
    drop(config);
    refresh_binding_caches(state);
    Ok(())
}

fn refresh_binding_caches(state: &mut State) {
    let config = state.swayward.config.borrow();
    let mod_key = state.backend.mod_key(&config);
    state
        .swayward
        .hotkey_overlay
        .on_hotkey_config_updated(mod_key);
    state.swayward.mods_with_mouse_binds =
        crate::input::mods_with_mouse_binds(mod_key, &config.binds);
    state.swayward.mods_with_wheel_binds =
        crate::input::mods_with_wheel_binds(mod_key, &config.binds);
    state.swayward.mods_with_tablet_stylus_binds =
        crate::input::mods_with_tablet_stylus_binds(mod_key, &config.binds);
    state.swayward.mods_with_finger_scroll_binds =
        crate::input::mods_with_finger_scroll_binds(mod_key, &config.binds);
}

fn execute_global_setting(
    state: &mut State,
    option: &swayward_ipc::command::LayoutOption,
) -> CommandOutcome {
    // Sway invokes the handler once per criteria match (`sway/commands.c:288-330`).
    // Reading toggle state here preserves that repeat-per-match behavior.
    #[cfg(test)]
    GLOBAL_SETTING_EXECUTIONS.set(GLOBAL_SETTING_EXECUTIONS.get() + 1);

    use swayward_ipc::command::LayoutOption;

    if let LayoutOption::TitlebarFont { font, .. } = option {
        let description = pangocairo::pango::FontDescription::from_string(font);
        if description.family().is_none() {
            return failure("Invalid font family.");
        }
        if description.size() == 0 {
            return failure("Invalid font size.");
        }
    }

    {
        let mut config = state.swayward.config.borrow_mut();
        let layout = &mut config.layout;
        let parsed = match option {
            LayoutOption::FocusWrapping(value) => {
                layout.focus_wrapping = match value.as_str() {
                    "force" => swayward_config::FocusWrapping::Force,
                    "workspace" => swayward_config::FocusWrapping::Workspace,
                    "toggle" if layout.focus_wrapping == swayward_config::FocusWrapping::Yes => {
                        swayward_config::FocusWrapping::No
                    }
                    "toggle" => swayward_config::FocusWrapping::Yes,
                    "yes" => swayward_config::FocusWrapping::Yes,
                    _ => swayward_config::FocusWrapping::No,
                };
                Ok(())
            }
            LayoutOption::ForceFocusWrapping(value) => {
                let enabled = parse_boolean(
                    value,
                    layout.focus_wrapping == swayward_config::FocusWrapping::Force,
                );
                layout.focus_wrapping = if enabled {
                    swayward_config::FocusWrapping::Force
                } else {
                    swayward_config::FocusWrapping::Yes
                };
                Ok(())
            }
            LayoutOption::WorkspaceLayout(value) => {
                value.parse().map(|value| layout.workspace_layout = value)
            }
            LayoutOption::DefaultOrientation(value) => value
                .parse()
                .map(|value| layout.default_orientation = value),
            LayoutOption::HideEdgeBorders(value) => {
                value.parse().map(|value| layout.hide_edge_borders = value)
            }
            LayoutOption::SmartBorders(value) => {
                if value == "toggle" {
                    layout.smart_borders =
                        if layout.smart_borders == swayward_config::SmartBorders::On {
                            swayward_config::SmartBorders::Off
                        } else {
                            swayward_config::SmartBorders::On
                        };
                    Ok(())
                } else {
                    value.parse().map(|value| layout.smart_borders = value)
                }
            }
            LayoutOption::SmartGaps(value) => {
                if value == "toggle" {
                    layout.smart_gaps = if layout.smart_gaps == swayward_config::SmartGaps::Off {
                        swayward_config::SmartGaps::On
                    } else {
                        swayward_config::SmartGaps::Off
                    };
                    Ok(())
                } else {
                    value.parse().map(|value| layout.smart_gaps = value)
                }
            }
            LayoutOption::ShowMarks(value) => {
                layout.titlebar.show_marks = parse_boolean(value, layout.titlebar.show_marks);
                Ok(())
            }
            LayoutOption::TitleAlignment(value) => {
                value.parse().map(|value| layout.titlebar.alignment = value)
            }
            LayoutOption::TilingDrag(value) => {
                config.input.tiling_drag = if value == "toggle" {
                    !config.input.tiling_drag
                } else {
                    parse_boolean(value, config.input.tiling_drag)
                };
                Ok(())
            }
            LayoutOption::TilingDragThreshold(value) => {
                config.input.tiling_drag_threshold = *value;
                Ok(())
            }
            LayoutOption::ForceDisplayUrgencyHint(value) => {
                config.urgent_timeout_ms = *value;
                Ok(())
            }
            LayoutOption::PrimarySelection(enabled) => {
                if *enabled == config.clipboard.disable_primary {
                    return failure("primary_selection can only be enabled/disabled at launch");
                }
                Ok(())
            }
            LayoutOption::FocusOnWindowActivation(value) => value
                .parse()
                .map(|value| config.focus_on_window_activation = value),
            LayoutOption::FocusFollowsMouse(mode) => {
                use swayward_config::input::{FocusFollowsMouse, FocusFollowsMouseMode};
                use swayward_ipc::command::FocusFollowsMouse as Requested;

                // Sway's FOLLOWS_NO is this field's absence. The KDL
                // form carries an optional max-scroll-amount that
                // sway's command has no argument for, so a mode change
                // keeps whatever threshold is already configured.
                let mode = match mode {
                    Requested::No => None,
                    Requested::Yes => Some(FocusFollowsMouseMode::Yes),
                    Requested::Always => Some(FocusFollowsMouseMode::Always),
                };
                config.input.focus_follows_mouse = mode.map(|mode| FocusFollowsMouse {
                    mode,
                    max_scroll_amount: config
                        .input
                        .focus_follows_mouse
                        .and_then(|ffm| ffm.max_scroll_amount),
                });
                Ok(())
            }
            LayoutOption::WorkspaceAutoBackAndForth(value) => {
                config.input.workspace_auto_back_and_forth =
                    parse_boolean(value, config.input.workspace_auto_back_and_forth);
                Ok(())
            }
            LayoutOption::FloatingMinimumSize(width, height) => {
                layout.floating_minimum_size = swayward_config::layout::FloatingSize {
                    width: *width,
                    height: *height,
                };
                Ok(())
            }
            LayoutOption::FloatingMaximumSize(width, height) => {
                layout.floating_maximum_size = swayward_config::layout::FloatingSize {
                    width: *width,
                    height: *height,
                };
                Ok(())
            }
            LayoutOption::TitlebarFont { font, pango_markup } => {
                layout.titlebar.font = font.clone();
                layout.titlebar.pango_markup = *pango_markup;
                Ok(())
            }
            LayoutOption::TitlebarPadding {
                horizontal,
                vertical,
            } => {
                if f64::from((*horizontal).min(*vertical))
                    < f64::from(layout.titlebar.border_thickness)
                {
                    return failure("Invalid size specified");
                }
                layout.titlebar.horizontal_padding = f64::from(*horizontal);
                layout.titlebar.vertical_padding = f64::from(*vertical);
                Ok(())
            }
            LayoutOption::TitlebarBorderThickness(thickness) => {
                if f64::from(*thickness) > layout.titlebar.vertical_padding {
                    return failure("Invalid size specified");
                }
                layout.titlebar.border_thickness = *thickness;
                Ok(())
            }
            LayoutOption::DefaultBorder {
                floating,
                style,
                width,
            } => {
                use swayward_config::layout::{SwayBorderDefault, SwayBorderStyle};
                let style = match style.as_str() {
                    "none" => SwayBorderStyle::None,
                    "pixel" => SwayBorderStyle::Pixel,
                    _ => SwayBorderStyle::Normal,
                };
                let slot = if *floating {
                    &mut layout.default_floating_border
                } else {
                    &mut layout.default_border
                };
                *slot = SwayBorderDefault {
                    style,
                    width: width.or(slot.width),
                };
                Ok(())
            }
            LayoutOption::PopupDuringFullscreen(value) => {
                config.popup_during_fullscreen = match value.as_str() {
                    "ignore" => swayward_config::misc::PopupDuringFullscreen::Ignore,
                    "leave_fullscreen" => {
                        swayward_config::misc::PopupDuringFullscreen::LeaveFullscreen
                    }
                    _ => swayward_config::misc::PopupDuringFullscreen::Smart,
                };
                Ok(())
            }
            LayoutOption::FloatingModifier { modifier, inverse } => {
                use swayward_config::input::{FloatingModifier, ModKey};

                // Sway keeps the modifier and the inverse bit as two
                // independent fields, and `none` is a value rather than a key
                // name. This is its own setting: mutating `mod_key` would move
                // every compositor binding as collateral.
                let modifier = match modifier {
                    None => Ok(ModKey::None),
                    Some(name) => name.parse(),
                };
                modifier.map(|modifier| {
                    config.input.floating_modifier = Some(FloatingModifier {
                        modifier,
                        inverse: *inverse,
                    });
                })
            }
            LayoutOption::MouseWarping(mode) => {
                use swayward_config::input::MouseWarping;
                use swayward_ipc::command::MouseWarping as Requested;

                config.input.mouse_warping = match mode {
                    Requested::No => MouseWarping::No,
                    Requested::Output => MouseWarping::Output,
                    Requested::Container => MouseWarping::Container,
                };
                Ok(())
            }
            LayoutOption::Xwayland { enabled } => {
                if *enabled == config.xwayland_satellite.off {
                    return failure("xwayland can only be enabled/disabled at launch");
                }
                Ok(())
            }
        };
        if let Err(error) = parsed {
            return failure(error.to_string());
        }
    }

    let config = state.swayward.config.clone();
    state.swayward.layout.update_config(&config.borrow());
    state.swayward.queue_redraw_all();
    success()
}

pub(crate) fn create_output(
    headless: Option<&mut crate::backend::Headless>,
    swayward: &mut crate::swayward::Swayward,
) -> CommandOutcome {
    let Some(headless) = headless else {
        // Sway uses this when its multi-backend contains no Wayland, X11, or
        // headless backend (`sway/commands/create_output.c:12-52`). Swayward's
        // tty and single-window winit backends likewise cannot add an output.
        return failure("Can only create outputs for Wayland, X11 or headless backends");
    };
    match headless.create_output(swayward) {
        Ok(()) => success(),
        Err(error) => failure(error),
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

fn command_failure(error: impl Into<String>) -> CommandOutcome {
    CommandOutcome {
        success: false,
        error: Some(error.into()),
        parse_error: Some(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(input: &str) -> Command {
        parse(input).into_iter().next().unwrap().unwrap().command
    }

    #[test]
    fn parses_all_title_format_placeholders() {
        let format = "%title %app_id %class %instance %shell %sandbox_engine %sandbox_app_id %sandbox_instance_id";
        assert_eq!(
            command(&format!("title_format {format}")),
            Command::TitleFormat(format.into())
        );
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
        assert_eq!(command("focus next"), Command::FocusNext);
        assert_eq!(command("focus prev"), Command::FocusPrev);
        assert_eq!(command("focus next sibling"), Command::FocusNextSibling);
        assert_eq!(command("focus prev sibling"), Command::FocusPrevSibling);
    }

    #[test]
    fn parses_shortcuts_inhibitor_view_policy_only() {
        assert_eq!(
            command("shortcuts_inhibitor enable"),
            Command::ShortcutsInhibitor(true)
        );
        assert_eq!(
            command("shortcuts_inhibitor disable"),
            Command::ShortcutsInhibitor(false)
        );
        for input in [
            "shortcuts_inhibitor",
            "shortcuts_inhibitor toggle",
            "shortcuts_inhibitor activate",
            "shortcuts_inhibitor deactivate",
            "shortcuts_inhibitor enable extra",
        ] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some("Expected `shortcuts_inhibitor enable|disable`"),
                "{input}"
            );
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
    fn parses_exit_and_rejects_arguments() {
        assert_eq!(command("exit"), Command::Exit);
        assert_eq!(
            parse("exit now")[0].as_ref().unwrap_err().error.as_deref(),
            Some("Invalid exit command (expected 0 arguments, got 1)")
        );
    }

    #[test]
    fn unsupported_runtime_state_commands_fail_loud() {
        for (input, error) in [
            (
                "opacity 0.5",
                "opacity requires mutable per-container opacity support",
            ),
            (
                "inhibit_idle visible",
                "inhibit_idle requires user inhibitor policy support",
            ),
        ] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some(error),
                "{input}"
            );
        }
    }

    #[test]
    fn runtime_presentation_commands_fail_loud() {
        for (input, error) in [
            (
                "allow_tearing yes",
                "allow_tearing requires immediate presentation support",
            ),
            (
                "max_render_time 1",
                "max_render_time requires per-view render deadline support",
            ),
        ] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some(error),
                "{input}"
            );
        }
        assert_eq!(
            parse("max_render_time")[0]
                .as_ref()
                .unwrap_err()
                .error
                .as_deref(),
            Some("Missing max render time argument.")
        );
    }

    #[test]
    fn parses_create_output_and_rejects_arguments() {
        assert_eq!(command("create_output"), Command::CreateOutput);
        assert!(parse("create_output extra")[0].is_err());
    }

    #[test]
    fn parses_urgent_boolean_modes_and_refuses_request_policy_modes() {
        for mode in [
            "1", "yes", "on", "true", "enable", "enabled", "active", "toggle", "0", "no", "off",
            "false", "disable", "disabled", "inactive", "invalid",
        ] {
            assert_eq!(
                command(&format!("urgent {mode}")),
                Command::Urgent(mode.into()),
                "{mode}"
            );
        }
        for mode in ["allow", "deny"] {
            assert_eq!(
                parse(&format!("urgent {mode}"))[0]
                    .as_ref()
                    .unwrap_err()
                    .error
                    .as_deref(),
                Some("urgent allow|deny requires client urgency-request policy support")
            );
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
        assert_eq!(command("focus workspace"), Command::FocusWorkspace);
        assert_eq!(
            command("focus left"),
            Command::FocusDirection(Direction::Left)
        );
        assert_eq!(command("focus parent"), Command::FocusParent);
        assert_eq!(command("focus floating"), Command::FocusFloating);
        assert_eq!(command("focus tiling"), Command::FocusTiling);
        assert_eq!(command("focus mode_toggle"), Command::FocusModeToggle);
        assert_eq!(
            command("mode --pango_markup created SET $destination workspace-7"),
            Command::Mode {
                name: "created".into(),
                pango_markup: true,
                subcommand: Some(Box::new(Command::Set {
                    name: "$destination".into(),
                    value: "workspace-7".into(),
                })),
            }
        );
        assert_eq!(
            parse("mode --pango_markup")[0]
                .as_ref()
                .unwrap_err()
                .error
                .as_deref(),
            Some("Mode name is missing")
        );
        assert_eq!(
            command("move right 12 px"),
            Command::MoveDirection {
                direction: Direction::Right,
                pixels: Some(12)
            }
        );
        assert_eq!(
            command("move to workspace number 3:web"),
            Command::MoveToWorkspace {
                target: WorkspaceTarget::Number("3:web".into()),
                auto_back_and_forth: true,
            }
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
        assert_eq!(
            command("swap container with con_id 42"),
            Command::Swap(SwapTarget::ConId("42".into()))
        );
        let parsed = parse("swap container with id 42");
        let id = parsed[0].as_ref().unwrap_err();
        assert_eq!(id.parse_error, Some(true));
        assert_eq!(
            id.error.as_deref(),
            Some("swap container with id is unsupported because X11 window IDs are unavailable")
        );
        for input in [
            "swap",
            "swap window with con_id 42",
            "swap container to con_id 42",
            "swap container with nope 42",
        ] {
            assert_eq!(
                parse(input)[0].as_ref().unwrap_err().error.as_deref(),
                Some("Expected 'swap container with id|con_id|mark <arg>'"),
                "{input}"
            );
        }
        let stacked = parse("layout stacked");
        assert_eq!(
            stacked[0].as_ref().unwrap_err().error.as_deref(),
            Some("Expected 'layout <splith|splitv|tabbed|stacking|toggle>'")
        );
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
            Command::Workspace {
                target: WorkspaceTarget::NextOnOutput,
                auto_back_and_forth: true,
            }
        );
        assert_eq!(
            command("workspace number 2:chat"),
            Command::Workspace {
                target: WorkspaceTarget::Number("2:chat".into()),
                auto_back_and_forth: true,
            }
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
        assert_eq!(
            command("gaps outer all set -10px"),
            Command::Gaps {
                inner: false,
                sides: [true; 4],
                all: true,
                operation: swayward_ipc::command::GapOperation::Set,
                amount: -10,
            }
        );
        assert_eq!(command("nop anything is ignored"), Command::Nop);
        assert_eq!(
            command("exec --no-startup-id notify-send 'hello; world'"),
            Command::Exec {
                command: "notify-send 'hello; world'".into(),
                no_startup_id: true,
            }
        );
        assert_eq!(
            command("exec_always echo hi"),
            Command::Exec {
                command: "echo hi".into(),
                no_startup_id: false,
            }
        );
        assert_eq!(
            command("exec --no-startup-identity"),
            Command::Exec {
                command: "--no-startup-identity".into(),
                no_startup_id: false,
            }
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
            Command::Exec {
                command: "echo 'a,b;c'".into(),
                no_startup_id: false,
            }
        );
    }

    #[test]
    fn comma_keeps_criteria_and_semicolon_starts_a_new_scope() {
        let parsed =
            parse(r#"[app_id="foo,bar"] focus left, focus right; [app_id="baz"] focus up"#);
        assert_eq!(parsed.len(), 3);
        assert_eq!(
            parsed[0].as_ref().unwrap().criteria.as_deref(),
            Some(r#"[app_id="foo,bar"]"#)
        );
        assert!(parsed[0].as_ref().unwrap().criteria_start);
        assert_eq!(
            parsed[1].as_ref().unwrap().criteria.as_deref(),
            Some(r#"[app_id="foo,bar"]"#)
        );
        assert!(!parsed[1].as_ref().unwrap().criteria_start);
        assert_eq!(
            parsed[2].as_ref().unwrap().criteria.as_deref(),
            Some(r#"[app_id="baz"]"#)
        );
        assert!(parsed[2].as_ref().unwrap().criteria_start);
    }

    #[test]
    fn malformed_criteria_after_semicolon_uses_the_criteria_error() {
        let parsed = parse(r#"[app_id="foo"] nop; [con_id=nope] nop"#);
        assert_eq!(parsed.len(), 2);
        let error = parsed[1].as_ref().unwrap_err();
        assert_eq!(error.parse_error, Some(true));
        assert_eq!(
            error.error.as_deref(),
            Some("The value for 'con_id' should be '__focused__' or numeric")
        );
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
    fn parses_move_no_auto_back_and_forth_only_for_workspace_targets() {
        for input in [
            "move --no-auto-back-and-forth workspace 3",
            "move --no-auto-back-and-forth window to workspace 3",
            "move --NO-AUTO-BACK-AND-FORTH CONTAINER workspace 3",
        ] {
            assert_eq!(
                command(input),
                Command::MoveToWorkspace {
                    target: WorkspaceTarget::Name("3".into()),
                    auto_back_and_forth: false,
                },
                "{input}"
            );
        }
        for input in [
            "move --no-auto-back-and-forth output right",
            "move --no-auto-back-and-forth mark target",
        ] {
            assert!(parse(input)[0].is_err(), "{input}");
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
    fn parses_sway_resize_set_forms_and_rejects_trailing_junk() {
        let amount = |amount, unit| ResizeAmount { amount, unit };
        for (input, width, height) in [
            (
                "resize set 201 131",
                Some(amount(201, ResizeUnit::Default)),
                Some(amount(131, ResizeUnit::Default)),
            ),
            (
                "resize set width 80 ppt",
                Some(amount(80, ResizeUnit::PercentagePoints)),
                None,
            ),
            (
                "resize set height 200 px",
                None,
                Some(amount(200, ResizeUnit::Pixels)),
            ),
            (
                "resize set 75 ppt 200 px",
                Some(amount(75, ResizeUnit::PercentagePoints)),
                Some(amount(200, ResizeUnit::Pixels)),
            ),
            (
                "resize set 0 ppt 75 ppt",
                Some(amount(0, ResizeUnit::PercentagePoints)),
                Some(amount(75, ResizeUnit::PercentagePoints)),
            ),
            (
                "resize set 75 ppt 0 ppt",
                Some(amount(75, ResizeUnit::PercentagePoints)),
                Some(amount(0, ResizeUnit::PercentagePoints)),
            ),
            (
                "resize set -1 px -2 ppt",
                Some(amount(-1, ResizeUnit::Pixels)),
                Some(amount(-2, ResizeUnit::PercentagePoints)),
            ),
        ] {
            assert_eq!(
                command(input),
                Command::ResizeSet { width, height },
                "{input}"
            );
        }
        for input in [
            "resize set width height 10",
            "resize set 100 px height 200 px junk",
        ] {
            assert!(parse(input)[0].is_err(), "{input}");
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
            Command::Workspace {
                target: WorkspaceTarget::Number("3: web browser".into()),
                auto_back_and_forth: true,
            }
        );
        assert_eq!(
            command("workspace 'mail and chat'"),
            Command::Workspace {
                target: WorkspaceTarget::Name("mail and chat".into()),
                auto_back_and_forth: true,
            }
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

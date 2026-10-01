use swayward_ipc::command::parse_with_variables;
use swayward_ipc::{criteria, CommandOutcome};

use super::settings::execute_global_setting;
use super::targeted::{
    execute_targeted, focused_con_id, mark_focused, matching_targets, set_client_colors,
    unmark_focused, unmark_target,
};
use super::{
    bindings, command_failure, failure, focus, gaps, layout, movement, output, rules, scratchpad,
    session, success, window, workspace, Command, CommandTarget, ParsedCommand,
};
use crate::swayward::State;

pub fn execute(state: &mut State, input: &str) -> Vec<CommandOutcome> {
    state.ipc_begin_workspace_transaction();
    // Sway expands variables before dispatch, for every argument except the
    // name being defined by `set` (`sway/sway/commands.c:283-285`). This is the
    // single choke point for both IPC commands and key bindings, matching
    // sway, where a binding re-enters execute_command at press time
    // (`sway/sway/commands/bind.c:635`).
    let has_view = state.swayward.layout.focus().is_some();
    let mut parsed = parse_with_variables(input, &state.swayward.sway_variables, has_view);
    // `border` checks for a view before anything else, so a well-formed
    // border command with nothing focused is refused too
    // (`sway/sway/commands/border.c:61-67`).
    if !has_view {
        for parsed in &mut parsed {
            if matches!(parsed, Ok(parsed) if matches!(parsed.command, Command::Border(_))) {
                *parsed = Err(swayward_ipc::command::parse_error(
                    "Only views can have borders",
                ));
            }
        }
    }
    // Sway stops the list after the first CMD_INVALID result, whether the
    // parser or a handler produced it, and continues past CMD_FAILURE
    // (`sway/sway/commands.c:296-299`, `316-321`).
    let mut retained_targets = None;
    let mut outcomes = Vec::new();
    for parsed in parsed {
        let outcome = match parsed {
            Ok(parsed) => {
                if parsed.criteria_start {
                    retained_targets = None;
                }
                execute_one(state, parsed, &mut retained_targets)
            }
            Err(error) => error,
        };
        let invalid = is_invalid(&outcome);
        outcomes.push(outcome);
        if invalid {
            break;
        }
    }
    state.ipc_commit_workspace_transaction();
    outcomes
}

/// Whether an outcome is sway's CMD_INVALID, which ends a command list.
fn is_invalid(outcome: &CommandOutcome) -> bool {
    !outcome.success && outcome.parse_error == Some(true)
}

/// Run a handler once per criteria match as sway does: every match runs, the
/// last failure is reported, and a CMD_INVALID stops the remaining matches
/// (`sway/sway/commands.c:305-323`).
fn for_each_match<T>(
    targets: impl IntoIterator<Item = T>,
    mut run: impl FnMut(T) -> CommandOutcome,
) -> CommandOutcome {
    let mut last_failure = None;
    for target in targets {
        let outcome = run(target);
        if is_invalid(&outcome) {
            return outcome;
        }
        if !outcome.success {
            last_failure = Some(outcome);
        }
    }
    last_failure.unwrap_or_else(success)
}

fn run_focused(state: &mut State, command: Command) -> super::HandlerResult {
    match command {
        Command::Swap(target) => movement::swap_focused(state, target),
        Command::Focus => Err(command_failure("No container to focus was specified.")),
        Command::FocusWorkspace => Err(failure("No container to focus was specified.")),
        Command::FocusDirection(direction) => Ok(focus::direction(state, direction)),
        Command::FocusOutput(identifier) => super::handled(focus::output(state, &identifier)),
        Command::FocusParent => {
            focus::parent(state);
            Ok(None)
        }
        Command::FocusChild => {
            focus::child(state);
            Ok(None)
        }
        Command::FocusNext => super::handled(focus::next_or_prev(state, true)),
        Command::FocusPrev => super::handled(focus::next_or_prev(state, false)),
        Command::FocusNextSibling => {
            focus::next_prev_sibling(state, true);
            Ok(None)
        }
        Command::FocusPrevSibling => {
            focus::next_prev_sibling(state, false);
            Ok(None)
        }
        Command::FocusFloating => focus::mode(state, true).map(Some),
        Command::FocusTiling => focus::mode(state, false).map(Some),
        Command::FocusModeToggle => focus::mode_toggle(state).map(Some),
        Command::MoveDirection { direction, pixels } => {
            movement::direction_focused(state, direction, pixels)
        }
        Command::MovePosition(position) => movement::position_focused(state, position),
        Command::MoveToWorkspace {
            target,
            auto_back_and_forth,
        } => movement::to_workspace_focused(state, target, auto_back_and_forth),
        Command::MoveToMark(mark) => movement::to_mark_focused(state, &mark),
        Command::MoveToOutput(target) => movement::to_output_focused(state, &target),
        Command::MoveWorkspaceToOutput(target) => {
            movement::workspace_to_output_focused(state, &target)
        }
        Command::MoveScratchpad => scratchpad::move_focused(state),
        Command::ScratchpadShow => scratchpad::show_focused(state),
        Command::LayoutDefault => super::handled(layout::default(state)),
        Command::LayoutToggle(cycle) => super::handled(layout::toggle(state, &cycle)),
        Command::Layout(value) => super::handled(layout::set(state, value)),
        Command::Split(value) => super::handled(layout::split(state, value)),
        Command::Fullscreen { mode, global } => {
            layout::fullscreen(state, mode, global);
            Ok(None)
        }
        Command::Opacity(value) => window::opacity_focused(state, value, false),
        Command::OpacityRelative(value) => window::opacity_focused(state, value, true),
        Command::TitleFormat(format) => window::title_format_focused(state, &format),
        Command::ShortcutsInhibitor(enable) => window::shortcuts_inhibitor_focused(state, enable),
        Command::Sticky(value) => window::sticky_focused(state, &value),
        Command::Border(border) => window::border_focused(state, &border),
        Command::Floating(mode) => window::floating_focused(state, mode),
        Command::Urgent(value) => window::urgent_focused(state, &value),
        Command::Workspace {
            target,
            auto_back_and_forth,
        } => workspace::activate(state, target, auto_back_and_forth),
        Command::AssignWorkspace { target, outputs } => workspace::assign(state, target, &outputs),
        Command::RenameWorkspace { old, new_name } => workspace::rename(state, old, new_name),
        Command::Kill => window::kill_focused(state),
        Command::ResizeSet { width, height } => window::resize_set_focused(state, width, height),
        Command::Resize {
            grow,
            axis,
            first,
            second,
        } => window::resize_focused(state, grow, axis, first, second),
        Command::Reload => session::reload(state),
        Command::Exit => session::exit(state),
        Command::CreateOutput => output::create(state),
        Command::InputSwitchLayout { identifier, target } => {
            output::switch_layout(state, &identifier, target)
        }
        Command::Output { target, actions } => output::configure(state, target, actions),
        Command::SetClientColors { class, colors } => {
            set_client_colors(state, class, colors);
            Ok(None)
        }
        Command::SetLayoutOption(option) => Err(execute_global_setting(state, &option)),
        Command::Gaps {
            inner,
            sides,
            all,
            operation,
            amount,
        } => gaps::update(state, inner, sides, all, operation, amount),
        Command::GapsDefaults {
            inner,
            sides,
            amount,
        } => gaps::defaults(state, inner, sides, amount),
        Command::WorkspaceGaps {
            name,
            inner,
            sides,
            amount,
        } => gaps::workspace(state, name, inner, sides, amount),
        Command::Mode {
            name,
            pango_markup,
            subcommand,
        } => bindings::mode(state, name, pango_markup, subcommand),
        Command::Set { name, value } => {
            bindings::set_variable(state, name, value);
            Ok(None)
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
        } => bindings::bind(
            state,
            bindings::BindingCommand {
                key,
                command,
                keycode,
                release,
                locked,
                inhibited,
                no_repeat,
                input_device,
            },
        ),
        Command::SwitchBind {
            switch,
            command,
            locked,
        } => bindings::switch_bind(state, switch, command, locked),
        Command::Nop => Ok(None),
        Command::Exec {
            command,
            no_startup_id,
        } => session::exec(state, command, no_startup_id),
        Command::Mark {
            add,
            toggle,
            identifier,
        } => mark_focused(state, add, toggle, &identifier),
        Command::Unmark(identifier) => unmark_focused(state, identifier.as_deref()),
        Command::Assign { criteria, target } => rules::assign(state, criteria, target),
        Command::NoFocus { criteria } => rules::no_focus(state, criteria),
        Command::ForWindow { criteria, command } => rules::for_window(state, criteria, command),
    }
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
            if option.is_global() {
                return for_each_match(targets, |_| execute_global_setting(state, option));
            }
        }
        if let Command::Unmark(identifier) = &parsed.command {
            for target in targets {
                unmark_target(state, target, identifier.as_deref());
            }
            return success();
        }
        return for_each_match(targets, |target| {
            execute_targeted(state, &parsed.command, target)
        });
    }

    let action = match run_focused(state, parsed.command) {
        Ok(action) => action,
        Err(outcome) => return outcome,
    };

    if let Some(action) = action {
        state.do_action(action, false);
    }
    state.ipc_refresh_layout();
    success()
}

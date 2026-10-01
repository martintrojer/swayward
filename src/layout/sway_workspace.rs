//! Sway workspace switching, creation, renaming and move-to-workspace commands.

use super::*;

impl<W: LayoutElement> Layout<W> {
    pub(super) fn move_activation(focus: bool) -> ActivateWindow {
        if focus {
            ActivateWindow::Smart
        } else {
            ActivateWindow::No
        }
    }

    fn create_workspace_at(&mut self, output: &Output, index: usize) -> WorkspaceId {
        let (name, number) = self.next_free_workspace_identity();
        let layout_config = self.workspace_layout_config(name.as_deref());
        let monitor = self.monitor_for_output_mut(output).unwrap();
        let id = monitor.add_sway_workspace_at(index, name, number, layout_config);
        monitor.sort_sway_workspaces();
        id
    }

    pub(super) fn create_next_workspace(&mut self, output: &Output) -> WorkspaceId {
        let index = self
            .monitor_for_output(output)
            .map(|monitor| monitor.workspaces.len())
            .unwrap();
        self.create_workspace_at(output, index)
    }

    pub(super) fn prepare_workspace_at(
        &mut self,
        output: &Output,
        index: usize,
    ) -> Option<WorkspaceId> {
        let workspace = self.monitor_for_output(output)?.workspaces.get(index)?;
        if workspace.has_sway_identity() {
            Some(workspace.id())
        } else {
            Some(self.create_workspace_at(output, index))
        }
    }

    pub(super) fn move_to_workspace_id(
        &mut self,
        window: Option<&W::Id>,
        target: WorkspaceId,
        activate: ActivateWindow,
    ) {
        if let Some(InteractiveMoveState::Moving(move_)) = &mut self.interactive_move {
            if window.is_none() || window == Some(move_.tile.window().id()) {
                return;
            }
        }

        let monitor = if let Some(window) = window {
            match &mut self.monitor_set {
                MonitorSet::Normal { monitors, .. } => {
                    let Some(monitor) = monitors.iter_mut().find(|mon| mon.has_window(window))
                    else {
                        return;
                    };
                    monitor
                }
                MonitorSet::NoOutputs { .. } => {
                    return;
                }
            }
        } else {
            let Some(monitor) = self.active_monitor() else {
                return;
            };
            monitor
        };
        monitor.move_to_workspace(window, target, activate);
    }

    pub fn move_focused_to_workspace_up(&mut self, activate: bool) {
        let Some(target) = self.active_monitor_ref().and_then(|monitor| {
            monitor
                .active_workspace_idx
                .checked_sub(1)
                .map(|index| monitor.workspaces[index].id())
        }) else {
            return;
        };
        self.move_focused_to_workspace_id(target, activate);
    }

    pub fn move_focused_to_workspace_down(&mut self, activate: bool) {
        let Some((output, target_index)) = self
            .active_monitor_ref()
            .filter(|monitor| monitor.active_workspace_ref().active_window().is_some())
            .map(|monitor| (monitor.output.clone(), monitor.active_workspace_idx + 1))
        else {
            return;
        };
        let target = self
            .prepare_workspace_at(&output, target_index)
            .unwrap_or_else(|| self.create_next_workspace(&output));
        self.move_focused_to_workspace_id(target, activate);
    }

    pub fn move_focused_to_workspace(&mut self, idx: usize, activate: bool) {
        if self
            .active_workspace()
            .and_then(Workspace::active_window)
            .is_none()
        {
            return;
        }
        let Some(output) = self.active_output().cloned() else {
            return;
        };
        if let Some(target) = self.prepare_workspace_at(&output, idx) {
            self.move_focused_to_workspace_id(target, activate);
        }
    }

    fn move_focused_to_workspace_id(&mut self, target: WorkspaceId, activate: bool) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.move_focused_to_workspace(target, activate);
    }

    pub fn switch_workspace_up_wrapping(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.switch_workspace_up_wrapping();
    }

    pub fn switch_workspace_down_wrapping(&mut self) {
        let Some(monitor) = self.active_monitor() else {
            return;
        };
        monitor.switch_workspace_down_wrapping();
    }

    pub fn finish_sway_workspace_switch(&mut self, target: &crate::command::WorkspaceTarget) {
        let target = self
            .workspaces()
            .find(|(_, _, workspace)| workspace_matches_target(workspace, target))
            .map(|(_, _, workspace)| workspace.id());
        if let Some(monitor) = self.active_monitor() {
            monitor.finish_workspace_switch(target);
        }
    }

    pub fn activate_sway_workspace_auto_back_and_forth(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(), String> {
        let existing = self.workspaces().find_map(|(monitor, index, workspace)| {
            workspace_matches_target(workspace, &target)
                .then(|| (monitor.map(|monitor| monitor.output().clone()), index))
        });
        let Some((output, index)) = existing else {
            return self.activate_sway_workspace(target);
        };
        if let Some(output) = output {
            if self.active_output() == Some(&output) {
                self.switch_workspace_auto_back_and_forth(index);
            } else {
                self.focus_output(&output);
                self.switch_workspace(index);
            }
        } else {
            self.switch_workspace_auto_back_and_forth(index);
        }
        Ok(())
    }

    pub fn activate_sway_workspace(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(), String> {
        use crate::command::WorkspaceTarget;

        match target {
            WorkspaceTarget::Current => return Ok(()),
            WorkspaceTarget::BackAndForth => {
                let Some(monitor) = self.active_monitor() else {
                    return Err("cannot switch workspaces without an output".into());
                };
                if let Some(previous) = monitor.previous_workspace_idx() {
                    self.switch_workspace(previous);
                    return Ok(());
                }
                let Some(previous_name) = monitor.previous_workspace_name().map(str::to_owned)
                else {
                    return Err("There is no previous workspace".into());
                };
                return self.activate_sway_workspace(WorkspaceTarget::Name(previous_name));
            }
            WorkspaceTarget::NextOnOutput | WorkspaceTarget::PrevOnOutput => {
                let next = target == WorkspaceTarget::NextOnOutput;
                let Some((output, workspace)) =
                    self.relative_sway_workspace_position_on_output(next)
                else {
                    return Err("cannot switch workspaces without an output".into());
                };
                if let Some(output) = output {
                    self.focus_output(&output);
                }
                self.switch_workspace(workspace);
                return Ok(());
            }
            WorkspaceTarget::Next | WorkspaceTarget::Prev => {
                return self.activate_relative_sway_workspace(target == WorkspaceTarget::Next);
            }
            _ => {}
        }

        // Collect every candidate so duplicate numeric identities resolve to
        // the explicitly named workspace.
        let existing = self.find_sway_workspace_position(&target);

        if let Some((output, index)) = existing {
            if let Some(output) = output.as_ref() {
                let monitor = self.monitor_for_output_mut(output).unwrap();
                if index != monitor.active_workspace_idx()
                    && !monitor.workspaces[index].tiling().has_had_tile()
                {
                    monitor.refresh_empty_auto_layout(index);
                }
            }
            self.activate_workspace_at(output.as_ref(), index);
            return Ok(());
        }

        let (name, number) = sway_workspace_identity(target)?;
        let (output, index) = self.create_sway_workspace(name, number)?;
        // Sway's workspace_switch focuses the new workspace, and with it the
        // output it was created on.
        self.activate_workspace_at(Some(&output), index);
        Ok(())
    }

    pub fn activate_workspace_at(&mut self, output: Option<&Output>, index: usize) {
        if let Some(output) = output {
            self.focus_output(output);
        }
        self.switch_workspace(index);
    }

    pub fn rename_sway_workspace(
        &mut self,
        old: Option<crate::command::WorkspaceTarget>,
        new_name: String,
    ) -> Result<(), String> {
        let id = match old {
            Some(ref target) => self
                .workspaces()
                .find(|(monitor, index, workspace)| {
                    workspace_matches_target(workspace, target)
                        && (workspace.has_windows()
                            || monitor
                                .is_none_or(|monitor| monitor.active_workspace_idx() == *index))
                })
                .map(|(_, _, workspace)| workspace.id()),
            None => self.active_workspace().map(Workspace::id),
        }
        .ok_or_else(|| "There is no workspace with that name".to_owned())?;
        self.rename_sway_workspace_by_id(id, new_name)
    }

    /// Rename an already-resolved workspace.
    ///
    /// Sway resolves the workspace first and then runs one rename body
    /// (`sway/sway/commands/rename.c:33-100`), so both the focus-resolved and
    /// the criteria-resolved paths share this, rather than duplicating the
    /// special-name, already-exists and persistence rules.
    pub fn rename_sway_workspace_by_id(
        &mut self,
        id: WorkspaceId,
        new_name: String,
    ) -> Result<(), String> {
        if matches!(
            new_name.to_ascii_lowercase().as_str(),
            "next"
                | "prev"
                | "next_on_output"
                | "prev_on_output"
                | "back_and_forth"
                | "current"
                | "number"
        ) {
            return Err(format!("Cannot use special workspace name '{new_name}'"));
        }
        self.destroy_stale_workspaces_named(&new_name, id);
        // Whatever survived (a persistent or still-visible workspace) is live,
        // and sway refuses to rename onto a live name
        // (sway/sway/commands/rename.c:84-91).
        if let Some(existing) = self.workspaces().find_map(|(_, _, workspace)| {
            workspace
                .sway_name()
                .is_some_and(|name| name.eq_ignore_ascii_case(&new_name))
                .then(|| workspace.id())
        }) {
            return (existing == id)
                .then_some(())
                .ok_or_else(|| "Workspace already exists".into());
        }

        // Persistence follows the configuration, not the name: a workspace
        // renamed away from its declared name no longer outlives its last
        // window.
        let declared = self
            .workspace_configs
            .iter()
            .any(|config| config.name.0.eq_ignore_ascii_case(&new_name));
        let (name, number) = sway_identity_from_name(new_name);
        let workspace = self.workspace_mut(id).unwrap();
        workspace.set_sway_identity(name, number);
        workspace.set_persistent(declared);
        self.reap_outputless_workspaces();
        if let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set {
            if let Some(monitor) = monitors.iter_mut().find(|monitor| monitor.has_ws(id)) {
                monitor.sort_sway_workspaces();
                // Refresh the back-and-forth target if it names this workspace.
                // Sway stores a workspace pointer, so a rename is transparent to
                // it; we cache the name, which went stale and sent
                // `workspace back_and_forth` to the old name.
                monitor.refresh_previous_workspace_name(id);
            }
        }
        Ok(())
    }

    /// Destroys the empty, non-visible workspaces named `new_name` other than
    /// `keep`, freeing the name for a rename.
    fn destroy_stale_workspaces_named(&mut self, new_name: &str, keep: WorkspaceId) {
        // Destroy a workspace sway would already have destroyed. Sway
        // destroys an empty, non-visible workspace no seat retains, and does so
        // when focus LEAVES it (seat_set_focus, sway/sway/input/seat.c:1244),
        // so by the time a rename runs the name is free. We can still hold
        // such a workspace, and skipping it without removing it let the rename
        // produce two workspaces with one name.
        if let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set {
            for monitor in monitors
                .iter_mut()
                .filter(|monitor| monitor.workspace_switch.is_none())
            {
                let stale = monitor
                    .workspaces
                    .iter()
                    .enumerate()
                    .filter(|(index, workspace)| {
                        // Sway also retains the workspace a seat's
                        // focus-inactive points at (workspace_consider_destroy,
                        // sway/sway/tree/workspace.c:322-329), which is the one
                        // we would return to via back_and_forth.
                        workspace.id() != keep
                            && !workspace.has_windows()
                            && *index != monitor.active_workspace_idx()
                            && monitor.previous_workspace_id() != Some(workspace.id())
                            && workspace
                                .sway_name()
                                .is_some_and(|name| name.eq_ignore_ascii_case(new_name))
                    })
                    .map(|(_, workspace)| workspace.id())
                    .collect::<Vec<_>>();
                for stale in stale {
                    monitor.consider_destroy_workspace(stale);
                }
            }
        }
    }

    /// The identity and layout for the workspace that replaces one leaving
    /// `old_output`.
    ///
    /// Name the replacement workspace the way a newly enabled output would
    /// be named, so an output vacated by this move gets back the workspace
    /// its `workspace <name> output <output>` assignment claims rather than
    /// a bare free number. Sway re-runs workspace_next_name for the same
    /// reason when a workspace leaves an output.
    pub(super) fn replacement_identity_for(
        &self,
        old_output: Option<&Output>,
    ) -> (
        (Option<String>, Option<i32>),
        Option<swayward_config::LayoutPart>,
    ) {
        let identity = old_output
            .and_then(|output| self.next_initial_workspace_name_for_output(Some(output)))
            .map(sway_identity_from_name)
            .unwrap_or_else(|| self.next_free_workspace_identity_for_output(old_output));
        let layout_config = self.workspace_layout_config(identity.0.as_deref());
        (identity, layout_config)
    }

    fn activate_relative_sway_workspace(&mut self, next: bool) -> Result<(), String> {
        let Some((output, workspace)) = self.relative_sway_workspace_position(next) else {
            return Err("cannot switch workspaces without an output".into());
        };
        if let Some(output) = output {
            self.focus_output(&output);
        }
        self.switch_workspace(workspace);
        Ok(())
    }

    fn relative_sway_workspace_position(&self, next: bool) -> Option<(Option<Output>, usize)> {
        let active = self.active_workspace()?;
        let current_number = active.number();
        let active_id = active.id();
        let positions = self
            .workspaces()
            .filter(|(_, _, workspace)| {
                workspace.has_windows()
                    || workspace.has_sway_identity()
                    || workspace.id() == active_id
            })
            .map(|(monitor, index, workspace)| WorkspacePosition {
                output: monitor.map(|monitor| monitor.output().clone()),
                index,
                id: workspace.id(),
                number: workspace.number(),
            })
            .collect::<Vec<_>>();
        let current = positions
            .iter()
            .position(|position| position.id == active_id)?;
        let mut order = (0..positions.len()).collect::<Vec<_>>();
        if !next {
            order.reverse();
        }

        // Sway scans outputs and each output's stored workspace list forwards
        // for next and backwards for prev. Numeric comparison chooses the next
        // distinct number, but scan order breaks ties between names with the
        // same numeric prefix (sway/sway/tree/workspace.c:548-677).
        let target = match current_number {
            Some(number) => next_numbered(&positions, &order, number, next),
            None => next_unnumbered(&positions, &order, current, next),
        }?;
        Some((target.output.clone(), target.index))
    }

    fn relative_sway_workspace_position_on_output(
        &self,
        next: bool,
    ) -> Option<(Option<Output>, usize)> {
        let output = self.active_output()?;
        let monitor = self.monitor_for_output(output)?;
        let current = monitor.active_workspace_idx;
        let positions = monitor
            .workspaces
            .iter()
            .enumerate()
            .filter(|(index, workspace)| {
                workspace.has_windows() || workspace.has_sway_identity() || *index == current
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let current = positions.iter().position(|index| *index == current)?;
        let target = if next {
            positions.get(current + 1).or_else(|| positions.first())
        } else {
            current
                .checked_sub(1)
                .and_then(|index| positions.get(index))
                .or_else(|| positions.last())
        }?;
        Some((Some(output.clone()), *target))
    }

    fn active_workspace_position(&self) -> Option<(Option<Output>, usize)> {
        let output = self.active_output()?.clone();
        let monitor = self.monitor_for_output(&output)?;
        Some((Some(output), monitor.active_workspace_idx))
    }

    fn previous_workspace_position(&self) -> Option<(Option<Output>, usize)> {
        let output = self.active_output()?.clone();
        let monitor = self.monitor_for_output(&output)?;
        Some((Some(output), monitor.previous_workspace_idx()?))
    }

    pub fn move_window_to_sway_workspace(
        &mut self,
        window: &W::Id,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> Result<(), String> {
        self.move_to_sway_workspace_inner(Some(window), target, auto_back_and_forth)
    }

    pub fn move_to_sway_workspace(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(), String> {
        self.move_to_sway_workspace_inner(None, target, true)
    }

    pub fn window_workspace_id(&self, window: &W::Id) -> Option<WorkspaceId> {
        self.workspaces()
            .find_map(|(_, _, workspace)| workspace.has_window(window).then(|| workspace.id()))
    }

    pub fn move_window_to_workspace_id(
        &mut self,
        window: &W::Id,
        target: WorkspaceId,
    ) -> Result<(), String> {
        let (target_output, target_index) = self
            .workspaces()
            .find_map(|(monitor, index, workspace)| {
                (workspace.id() == target)
                    .then(|| (monitor.map(|monitor| monitor.output().clone()), index))
            })
            .ok_or_else(|| "target workspace does not exist".to_owned())?;
        let source_output = self
            .workspaces()
            .find(|(_, _, workspace)| workspace.has_window(window))
            .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output().clone()));
        if target_output != source_output {
            let output =
                target_output.ok_or_else(|| "target workspace has no output".to_owned())?;
            self.move_to_output(
                Some(window),
                &output,
                Some(target_index),
                ActivateWindow::No,
            );
        } else {
            self.move_to_workspace_id(Some(window), target, ActivateWindow::No);
        }
        Ok(())
    }

    pub fn active_workspace_id_for_output(&self, output: &Output) -> Option<WorkspaceId> {
        self.monitor_for_output(output)
            .map(|monitor| monitor.active_workspace_ref().id())
    }

    pub(super) fn resolve_sway_workspace_target(
        &mut self,
        target: crate::command::WorkspaceTarget,
    ) -> Result<(Option<Output>, usize), String> {
        use crate::command::WorkspaceTarget;

        let target_position = match target {
            WorkspaceTarget::Current => self.active_workspace_position(),
            WorkspaceTarget::BackAndForth => self.previous_workspace_position(),
            WorkspaceTarget::Next | WorkspaceTarget::Prev => {
                let next = target == WorkspaceTarget::Next;
                self.relative_sway_workspace_position(next)
            }
            WorkspaceTarget::NextOnOutput | WorkspaceTarget::PrevOnOutput => {
                let next = target == WorkspaceTarget::NextOnOutput;
                self.relative_sway_workspace_position_on_output(next)
            }
            _ => self.find_sway_workspace_position(&target),
        };
        if let Some(position) = target_position {
            Ok(position)
        } else {
            let (name, number) = sway_workspace_identity(target)?;
            let (output, index) = self.create_sway_workspace(name, number)?;
            Ok((Some(output), index))
        }
    }

    pub(super) fn resolve_move_workspace_target(
        &self,
        source_workspace: WorkspaceId,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> crate::command::WorkspaceTarget {
        if !auto_back_and_forth {
            return target;
        }
        let targets_source = self
            .workspace(source_workspace)
            .is_some_and(|workspace| workspace_matches_target(workspace, &target));
        if !targets_source {
            return target;
        }
        self.active_monitor_ref()
            .and_then(|monitor| monitor.previous_workspace_name())
            .map(|name| crate::command::WorkspaceTarget::Name(name.to_owned()))
            .unwrap_or(target)
    }

    fn move_to_sway_workspace_inner(
        &mut self,
        window: Option<&W::Id>,
        target: crate::command::WorkspaceTarget,
        auto_back_and_forth: bool,
    ) -> Result<(), String> {
        let moved_window = window.cloned().or_else(|| {
            self.active_workspace()
                .and_then(Workspace::active_window)
                .map(|window| window.id().clone())
        });
        let moved_window_was_focused = moved_window
            .as_ref()
            .is_some_and(|window| self.focus().map(|focused| focused.id()) == Some(window));
        let source_workspace = window
            .and_then(|window| {
                self.workspaces()
                    .find(|(_, _, workspace)| workspace.has_window(window))
                    .map(|(_, _, workspace)| workspace.id())
            })
            .or_else(|| self.active_workspace().map(Workspace::id));
        let target = source_workspace.map_or(target.clone(), |source| {
            self.resolve_move_workspace_target(source, target, auto_back_and_forth)
        });
        let (target_output, target_index) = self.resolve_sway_workspace_target(target)?;
        let source_output = window
            .and_then(|window| {
                self.workspaces()
                    .find(|(_, _, workspace)| workspace.has_window(window))
                    .and_then(|(monitor, _, _)| monitor.map(|monitor| monitor.output().clone()))
            })
            .or_else(|| self.active_output().cloned());
        let target_workspace = match target_output.as_ref() {
            Some(output) => self
                .monitor_for_output(output)
                .and_then(|monitor| monitor.workspaces.get(target_index)),
            None => self
                .workspaces()
                .nth(target_index)
                .map(|(_, _, workspace)| workspace),
        }
        .map(Workspace::id)
        .ok_or_else(|| "target workspace does not exist".to_owned())?;
        if source_workspace == Some(target_workspace) {
            // `container_move_to_workspace` returns early for the current
            // workspace, but `cmd_move_container` still arranges the parent of
            // the destination, the workspace's focus-inactive tiling container
            // (sway/commands/move.c:98-99, 199-207). Under fullscreen that
            // gives the fullscreen container its tiled slot.
            if let Some(workspace) = self.workspace_mut(target_workspace) {
                workspace.tiling_mut().arrange_fullscreen_parent();
            }
            return Ok(());
        }
        if target_output != source_output {
            let output =
                target_output.ok_or_else(|| "target workspace has no output".to_owned())?;
            self.move_to_output(window, &output, Some(target_index), ActivateWindow::No);
        } else {
            self.move_to_workspace_id(window, target_workspace, ActivateWindow::No);
        }
        if let Some(window) = moved_window.filter(|_| moved_window_was_focused) {
            // The move may have been refused, so the target need not exist or
            // hold the window any more.
            // Sway then raises the destination's fullscreen container back
            // above a view moved under it (`workspace_focus_fullscreen`,
            // sway/commands/move.c:96-110), so only refocus the moved view when
            // nothing else on the destination is fullscreen.
            if let Some(workspace) = self.workspace_mut(target_workspace).filter(|workspace| {
                workspace.fullscreen_window().is_none()
                    || workspace.fullscreen_contains_window(&window)
            }) {
                workspace.activate_window(&window);
            }
        }
        Ok(())
    }

    /// Assign a workspace to the first of `output_names` that resolves.
    ///
    /// Sway accepts a list and walks it in order, taking the first output that
    /// exists (`sway/sway/commands/workspace.c:153-155`;
    /// `sway/sway/tree/workspace.c:244-250`).
    pub fn assign_sway_workspace(
        &mut self,
        target: crate::command::WorkspaceTarget,
        output_names: &[String],
    ) -> Result<(), String> {
        let output = output_names
            .iter()
            .find_map(|name| {
                self.outputs()
                    .find(|output| output_matches_name(output, name))
                    .cloned()
            })
            .ok_or_else(|| match output_names {
                [name] => format!("unknown output '{name}'"),
                names => format!("no such output: {}", names.join(", ")),
            })?;
        let (old_monitor, _, workspace) = self
            .workspaces()
            .find(|(_, _, workspace)| workspace_matches_target(workspace, &target))
            .ok_or_else(|| "workspace does not exist".to_owned())?;
        let old_output = old_monitor.map(|monitor| monitor.output().clone());
        self.move_workspace_to_output_by_id(workspace.id(), old_output, &output);
        Ok(())
    }

    /// The workspace with `id`, on any output or none.
    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace<W>> {
        self.workspaces()
            .find(|(_, _, workspace)| workspace.id() == id)
            .map(|(_, _, workspace)| workspace)
    }

    /// The workspace with `id`, on any output or none.
    pub fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace<W>> {
        self.workspaces_mut().find(|workspace| workspace.id() == id)
    }
}

/// A workspace considered by `workspace next`/`prev`, in sway's scan order.
struct WorkspacePosition {
    output: Option<Output>,
    index: usize,
    id: WorkspaceId,
    number: Option<i32>,
}

/// `positions` in scan order: forwards for next, backwards for prev.
fn scan<'a>(
    positions: &'a [WorkspacePosition],
    order: &'a [usize],
) -> impl Iterator<Item = (usize, &'a WorkspacePosition)> + 'a {
    order.iter().map(|index| (*index, &positions[*index]))
}

/// The least numbered workspace for next, the greatest for prev.
fn extreme_numbered<'a>(
    positions: &'a [WorkspacePosition],
    order: &'a [usize],
    next: bool,
) -> Option<&'a WorkspacePosition> {
    scan(positions, order)
        .filter(|(_, position)| position.number.is_some())
        .min_by_key(|(_, position)| {
            position
                .number
                .map(|number| if next { number } else { -number })
        })
        .map(|(_, position)| position)
}

/// The first named (unnumbered) workspace in scan order.
fn first_unnumbered<'a>(
    positions: &'a [WorkspacePosition],
    order: &'a [usize],
) -> Option<&'a WorkspacePosition> {
    scan(positions, order)
        .find(|(_, position)| position.number.is_none())
        .map(|(_, position)| position)
}

/// From a numbered workspace: the closest number beyond `number`, else the
/// first named workspace, else the extreme number. Mirrors the numbered
/// branches of `workspace_next`/`workspace_prev`
/// (sway/sway/tree/workspace.c:577-604, 641-668).
fn next_numbered<'a>(
    positions: &'a [WorkspacePosition],
    order: &'a [usize],
    number: i32,
    next: bool,
) -> Option<&'a WorkspacePosition> {
    scan(positions, order)
        .filter(|(_, position)| {
            position.number.is_some_and(|candidate| {
                if next {
                    candidate > number
                } else {
                    candidate < number
                }
            })
        })
        .min_by_key(|(_, position)| position.number.map(|candidate| candidate.abs_diff(number)))
        .map(|(_, position)| position)
        .or_else(|| first_unnumbered(positions, order))
        .or_else(|| extreme_numbered(positions, order, next))
}

/// From a named workspace: the next named workspace in scan order, else the
/// extreme number, else the first named workspace. Mirrors the named
/// branches of `workspace_next`/`workspace_prev`
/// (sway/sway/tree/workspace.c:552-576, 617-640).
fn next_unnumbered<'a>(
    positions: &'a [WorkspacePosition],
    order: &'a [usize],
    current: usize,
    next: bool,
) -> Option<&'a WorkspacePosition> {
    scan(positions, order)
        .find(|(index, position)| {
            position.number.is_none()
                && if next {
                    *index > current
                } else {
                    *index < current
                }
        })
        .map(|(_, position)| position)
        .or_else(|| extreme_numbered(positions, order, next))
        .or_else(|| first_unnumbered(positions, order))
}

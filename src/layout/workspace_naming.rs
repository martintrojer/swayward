//! Sway workspace naming and output-assignment rules.

use super::*;

fn parse_workspace_num(name: &str) -> Option<i32> {
    let end = name
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(name.len());
    (end > 0).then(|| name[..end].parse().ok()).flatten()
}

/// Whether `name` is matched by the `workspace number <target>` form.
///
/// Mirrors `_workspace_by_number` (sway/sway/tree/workspace.c:493-502): the
/// digits of `target` must equal the leading digits of `name`, and `name` must
/// not carry a further digit. So "1" matches "1" and "1:first" but not "11".
pub(crate) fn workspace_name_matches_number(name: &str, target: &str) -> bool {
    let mut name_chars = name.chars();
    for digit in target.chars().take_while(char::is_ascii_digit) {
        if name_chars.next() != Some(digit) {
            return false;
        }
    }
    !name_chars.next().is_some_and(|c| c.is_ascii_digit())
}

pub(crate) fn sway_workspace_num(name: &str) -> i32 {
    parse_workspace_num(name).unwrap_or(-1)
}

pub(super) fn workspace_matches_target<W: LayoutElement>(
    workspace: &Workspace<W>,
    target: &crate::command::WorkspaceTarget,
) -> bool {
    match target {
        // Match the digit prefix of the name, as sway's _workspace_by_number
        // does (sway/sway/tree/workspace.c:493-502), so `number 1` finds
        // "1:first". Comparing a stored number missed it, and `move ... to
        // workspace number` then created a second workspace instead.
        crate::command::WorkspaceTarget::Number(value) => {
            workspace.has_sway_identity()
                && workspace
                    .sway_name()
                    .is_some_and(|name| workspace_name_matches_number(&name, value))
        }
        crate::command::WorkspaceTarget::Name(value) => workspace
            .sway_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(value)),
        _ => false,
    }
}

pub(super) fn sway_workspace_identity(
    target: crate::command::WorkspaceTarget,
) -> Result<(Option<String>, Option<i32>), String> {
    match target {
        crate::command::WorkspaceTarget::Number(name) => {
            let number = parse_workspace_num(&name)
                .ok_or_else(|| format!("invalid workspace number '{name}'"))?;
            Ok(((name != number.to_string()).then_some(name), Some(number)))
        }
        crate::command::WorkspaceTarget::Name(name) => Ok(sway_identity_from_name(name)),
        _ => Err("relative workspace target cannot be created".into()),
    }
}

/// The sway identity of a workspace created by name: its parsed number, and
/// the name too unless it is exactly that number. Unlike
/// [`sway_workspace_identity`], a name always yields an identity.
pub(super) fn sway_identity_from_name(name: String) -> (Option<String>, Option<i32>) {
    let number = parse_workspace_num(&name);
    let keep_name = number.is_none_or(|number| name != number.to_string());
    (keep_name.then_some(name), number)
}

fn initial_workspace_name_from_action(action: &swayward_config::Action) -> Option<String> {
    let target = match action {
        swayward_config::Action::SwayCommand(command) => {
            let parsed = crate::command::parse(command).into_iter().next()?.ok()?;
            let crate::command::Command::Workspace { target, .. } = parsed.command else {
                return None;
            };
            target
        }
        swayward_config::Action::FocusWorkspace(reference) => {
            return match reference {
                swayward_config::WorkspaceReference::Name(name) => Some(name.clone()),
                swayward_config::WorkspaceReference::Index(index) => Some(index.to_string()),
                swayward_config::WorkspaceReference::Id(_) => None,
            };
        }
        _ => return None,
    };
    match target {
        crate::command::WorkspaceTarget::Name(name) if !name.eq_ignore_ascii_case("number") => {
            Some(name)
        }
        crate::command::WorkspaceTarget::Number(name) => Some(name),
        _ => None,
    }
}

/// Free-function form of [`Layout::workspace_layout_config`], for the creation
/// sites that already hold a mutable borrow of `self.monitor_set`.
fn layout_config_for(
    workspace_configs: &[WorkspaceConfig],
    name: Option<&str>,
) -> Option<swayward_config::LayoutPart> {
    let name = name?;
    workspace_configs
        .iter()
        .find(|config| config.name.0.eq_ignore_ascii_case(name))
        .and_then(|config| config.layout.clone())
        .map(|layout| layout.0)
}

pub(super) fn initial_workspace_names(config: &Config) -> Vec<String> {
    config
        .binds
        .0
        .iter()
        .filter_map(|bind| initial_workspace_name_from_action(&bind.action))
        .collect()
}

impl<W: LayoutElement> Layout<W> {
    pub(super) fn next_free_workspace_identity(&self) -> (Option<String>, Option<i32>) {
        self.next_free_workspace_identity_for_output(None)
    }

    pub(super) fn next_free_workspace_identity_for_output(
        &self,
        output: Option<&Output>,
    ) -> (Option<String>, Option<i32>) {
        let mut used = self
            .workspaces()
            .filter_map(|(_, _, workspace)| workspace.sway_name())
            .filter_map(|name| parse_workspace_num(&name))
            .filter(|number| *number > 0)
            .collect::<HashSet<_>>();
        // Also skip a number that an assignment claims for a DIFFERENT output.
        // Sway's fallback loop rejects a candidate while workspace_by_number
        // finds it (sway/sway/tree/workspace.c:484-490), and such a name is
        // reserved for the output its assignment names, so handing it to this
        // output would take a name that is not free.
        if let Some(output) = output {
            for config in &self.workspace_configs {
                if Self::workspace_assignment(config).is_none() {
                    continue;
                }
                if self.workspace_assigned_to_output(&config.name.0, output) {
                    continue;
                }
                // Only a name whose assignment actually RESOLVES is reserved.
                // An assignment naming solely absent outputs claims nothing, so
                // its number stays free: sway's workspace_by_number test only
                // rejects a number some existing workspace holds, and such a
                // workspace is never created.
                let resolves = Self::workspace_assignment(config)
                    .into_iter()
                    .flatten()
                    .any(|name| {
                        self.monitors()
                            .any(|monitor| output_matches_name(monitor.output(), &name))
                    });
                if !resolves {
                    continue;
                }
                if let Some(number) = parse_workspace_num(&config.name.0) {
                    used.insert(number);
                }
            }
        }
        let number = (1..).find(|number| !used.contains(number)).unwrap();
        (None, Some(number))
    }

    /// The name a newly enabled output should give its first workspace.
    ///
    /// Mirrors `workspace_next_name` (sway/sway/tree/workspace.c:436-490).
    /// Names from bindings come first, then `workspace <name> output <output>`
    /// assignments. An assignment is skipped when a workspace of that name
    /// already exists. Within one assignment sway walks the listed outputs and
    /// `break`s at the FIRST one that resolves, claiming the name only if that
    /// output is this one; an output name that resolves to nothing does not
    /// break, so the search continues. An assignment naming only absent outputs
    /// therefore claims nothing, and its workspace falls back to a free number.
    /// Whether `name` may be used as `output`'s first workspace name.
    ///
    /// Mirrors `workspace_valid_on_output` (sway/sway/tree/workspace.c:334-354).
    /// A name with no assignment is valid on any output. Otherwise the first
    /// output in its assignment that RESOLVES decides: the name is valid only
    /// on that output. An assignment naming only absent outputs is valid
    /// nowhere.
    fn workspace_valid_on_output(&self, name: &str, output: &Output) -> bool {
        let Some(config) = self
            .workspace_configs
            .iter()
            .find(|config| config.name.0.eq_ignore_ascii_case(name))
        else {
            return true;
        };
        if Self::workspace_assignment(config).is_none() {
            return true;
        }
        self.workspace_assigned_to_output(name, output)
    }

    /// The per-name layout configuration for `name`, if the config declares it.
    ///
    /// Sway applies this when the workspace is created
    /// (`sway/sway/tree/workspace.c:224-243`), so every creation path consults
    /// it, not just the eager startup one.
    pub(super) fn workspace_layout_config(
        &self,
        name: Option<&str>,
    ) -> Option<swayward_config::LayoutPart> {
        layout_config_for(&self.workspace_configs, name)
    }

    /// The output and index of the workspace `target` names, preferring a
    /// named workspace when duplicate numeric identities match.
    pub(super) fn find_sway_workspace_position(
        &self,
        target: &crate::command::WorkspaceTarget,
    ) -> Option<(Option<Output>, usize)> {
        let mut found = self
            .workspaces()
            .filter(|(_, _, workspace)| workspace_matches_target(workspace, target))
            .map(|(monitor, index, workspace)| {
                (
                    monitor.map(|monitor| monitor.output().clone()),
                    index,
                    workspace.name().is_some(),
                )
            })
            .collect::<Vec<_>>();
        found.sort_by_key(|(_, _, named)| !*named);
        found
            .into_iter()
            .next()
            .map(|(output, index, _)| (output, index))
    }

    /// The ordered output list a workspace config assigns, under either
    /// spelling: `sway-output-assignment` carries a list, while a single
    /// `open-on-output` is what the translator emits for a sway
    /// `workspace <name> output <output>`.
    fn workspace_assignment(config: &WorkspaceConfig) -> Option<Vec<String>> {
        config.sway_output_assignment.clone().or_else(|| {
            config
                .open_on_output
                .as_ref()
                .map(|output| vec![output.clone()])
        })
    }

    /// Whether `name`'s assignment claims `output`.
    ///
    /// Sway breaks at the first output in the list that RESOLVES and claims the
    /// name only if that output is this one (sway/sway/tree/workspace.c:
    /// 465-475), so an assignment naming only absent outputs claims nothing.
    fn workspace_assigned_to_output(&self, name: &str, output: &Output) -> bool {
        let Some(config) = self
            .workspace_configs
            .iter()
            .find(|config| config.name.0.eq_ignore_ascii_case(name))
        else {
            return false;
        };
        let Some(outputs) = Self::workspace_assignment(config) else {
            return false;
        };
        // `output` may not be in the monitor list yet, since add_output resolves
        // the name before inserting it, so resolve against the monitors PLUS
        // this output. Sway's output_by_name_or_id sees the output because
        // output_enable adds it first (sway/sway/tree/output.c:161-166).
        outputs
            .iter()
            .find(|name| {
                output_matches_name(output, name)
                    || self
                        .monitors()
                        .any(|monitor| output_matches_name(monitor.output(), name))
            })
            .is_some_and(|name| output_matches_name(output, name))
    }

    /// The monitor a newly created workspace called `name` belongs on.
    ///
    /// Sway creates every workspace on the first RESOLVING output in its
    /// workspace config, else on the focused output
    /// (workspace_get_initial_output, sway/tree/workspace.c:153-175), so
    /// `workspace`, `move container to workspace` and `assign` agree.
    fn initial_monitor_for_workspace(&self, name: &str) -> Option<usize> {
        let MonitorSet::Normal {
            monitors,
            active_monitor_idx,
            ..
        } = &self.monitor_set
        else {
            return None;
        };
        let assigned = self
            .workspace_configs
            .iter()
            .find(|config| config.name.0.eq_ignore_ascii_case(name))
            .and_then(Self::workspace_assignment)
            .and_then(|outputs| {
                outputs.iter().find_map(|output| {
                    monitors
                        .iter()
                        .position(|monitor| output_matches_name(&monitor.output, output))
                })
            });
        Some(assigned.unwrap_or(*active_monitor_idx))
    }

    /// Creates a sway workspace on its initial output and returns that output
    /// and the workspace's index there. Sway sorts an output's workspaces on
    /// every creation (workspace_create calls output_sort_workspaces,
    /// sway/tree/workspace.c:259; ordering in sway/tree/output.c:387-405).
    pub(super) fn create_sway_workspace(
        &mut self,
        name: Option<String>,
        number: Option<i32>,
    ) -> Result<(Output, usize), String> {
        let workspace_name = name
            .clone()
            .or_else(|| number.map(|number| number.to_string()))
            .unwrap_or_default();
        let monitor_idx = self
            .initial_monitor_for_workspace(&workspace_name)
            .ok_or_else(|| "cannot create a workspace without an output".to_owned())?;
        let layout_config = layout_config_for(&self.workspace_configs, name.as_deref());
        let MonitorSet::Normal { monitors, .. } = &mut self.monitor_set else {
            return Err("cannot create a workspace without an output".into());
        };
        let monitor = &mut monitors[monitor_idx];
        let index = monitor.workspaces_len().saturating_sub(1);
        if monitor.workspaces_len() == 1 && !monitor.active_workspace_ref().tiling_has_had_window()
        {
            monitor.refresh_empty_auto_layout(0);
        }
        let id = monitor.add_sway_workspace_at(index, name, number, layout_config);
        monitor.sort_sway_workspaces();
        let index = monitor.idx_of_ws(id).unwrap_or(index);
        Ok((monitor.output().clone(), index))
    }

    pub(super) fn next_initial_workspace_name_for_output(
        &self,
        output: Option<&Output>,
    ) -> Option<String> {
        let existing_names = self
            .workspaces()
            .filter_map(|(_, _, workspace)| workspace.sway_name())
            .collect::<Vec<_>>();
        let unused = |name: &str| {
            !existing_names
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(name))
        };

        let output = output?;

        // Sway takes "assignments primarily, falling back to bindings and
        // numbers" (sway/sway/tree/workspace.c:440): both loops run and the
        // ASSIGNMENT loop overwrites whatever a binding chose, so an assignment
        // naming this output wins.
        if let Some(name) = self
            .workspace_configs
            .iter()
            .filter(|config| unused(&config.name.0))
            .find(|config| self.workspace_assigned_to_output(&config.name.0, output))
            .map(|config| config.name.0.clone())
        {
            return Some(name);
        }

        // Then a binding name, but only on an output it is VALID on:
        // workspace_valid_on_output (sway/sway/tree/workspace.c:334-354)
        // requires a name that HAS an assignment to match that assignment's
        // first resolvable output. A name without one is valid anywhere.
        self.initial_workspace_names
            .iter()
            .find(|name| unused(name) && self.workspace_valid_on_output(name, output))
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn initialize_workspaces_from_bindings(&mut self, config: &Config) {
        self.initial_workspace_names = initial_workspace_names(config);
        // Also take the workspace configs, so a `workspace <name> output
        // <output>` assignment is visible while startup names are resolved.
        // Without this the assignment list was empty here and every output fell
        // through to a binding name or a bare number.
        self.workspace_configs = config.workspaces.clone();
        for monitor in self.monitors_mut() {
            for workspace in &mut monitor.workspaces {
                if !workspace.is_persistent() && !workspace.has_windows() {
                    workspace.unname();
                }
            }
        }
        // Pair each unnamed workspace with its own output, so an assignment
        // like `workspace special output fake-0` is resolved against the output
        // the workspace actually sits on, the way sway's workspace_next_name
        // takes the output name. Carry on rather than breaking: an output with
        // no claimable name must not stop a later output from taking its own.
        let available = self
            .workspaces()
            .filter(|(_, _, workspace)| !workspace.has_sway_identity() && !workspace.has_windows())
            .map(|(monitor, _, workspace)| {
                (
                    workspace.id(),
                    monitor.map(|monitor| monitor.output().clone()),
                )
            })
            .collect::<Vec<_>>();
        for (id, output) in available {
            // Fall back to the next free number when no name claims this
            // output, as sway's workspace_next_name does
            // (sway/sway/tree/workspace.c:484-490). Skipping the output left it
            // with the index-derived identity the workspace model forbids.
            let (name, number) = match self.next_initial_workspace_name_for_output(output.as_ref())
            {
                Some(name) => sway_identity_from_name(name),
                None => self.next_free_workspace_identity_for_output(output.as_ref()),
            };
            self.workspace_mut(id)
                .unwrap()
                .set_sway_identity(name, number);
        }
    }
}

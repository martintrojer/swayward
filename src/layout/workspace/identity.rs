//! Sway workspace identity: name, number and persistence.

use super::*;

impl<W: LayoutElement> Workspace<W> {
    pub fn sway_name(&self) -> Option<String> {
        self.name
            .clone()
            .or_else(|| self.number.map(|number| number.to_string()))
    }

    pub fn sway_display_name(&self, index: usize) -> String {
        self.sway_name().unwrap_or_else(|| (index + 1).to_string())
    }

    /// The number a client sees for this workspace.
    ///
    /// `index` is only a fallback for the initial workspace before startup has
    /// assigned its sway identity.
    pub fn sway_display_number(&self, index: usize) -> i32 {
        self.number.unwrap_or_else(|| {
            self.name.as_ref().map_or_else(
                || i32::try_from(index + 1).unwrap_or(-1),
                |name| crate::layout::sway_workspace_num(name),
            )
        })
    }

    pub fn number(&self) -> Option<i32> {
        self.number
    }

    pub fn set_sway_identity(&mut self, name: Option<String>, number: Option<i32>) {
        self.name = name;
        self.number = number;
    }

    pub fn set_persistent_name(&mut self, name: String) {
        self.name = Some(name);
        self.number = None;
        self.persistent = true;
    }

    /// Whether this workspace outlives its last window.
    ///
    /// Only a configuration declaration earns that. A runtime
    /// `rename workspace` moves the name, not the declaration, so a workspace
    /// renamed away from its configured name becomes disposable again.
    pub fn set_persistent(&mut self, persistent: bool) {
        self.persistent = persistent;
    }

    /// Whether a client can address this workspace.
    ///
    /// A sway workspace is identified by a name, a number, or both. niri had
    /// only `name`, so the inherited checks asked about that one field; adding
    /// `number` left them answering a question nobody was asking. Every caller
    /// that means "is this workspace visible to clients" must use this.
    pub fn has_sway_identity(&self) -> bool {
        self.name.is_some() || self.number.is_some()
    }

    /// Whether the configuration declares this workspace, so it outlives its
    /// last window. Implies [`Workspace::has_sway_identity`], because a
    /// persistent workspace is always created with a name.
    pub fn is_persistent(&self) -> bool {
        self.persistent
    }

    /// Whether this workspace outlives focus leaving it.
    ///
    /// Deliberately *not* [`Workspace::has_sway_identity`]. A workspace that
    /// merely holds a number is addressable but disposable: measured on sway
    /// 1.11, focusing an empty workspace 7 and switching away leaves
    /// `get_workspaces` reporting no 7 at all, while an empty workspace that
    /// still holds focus is reported. Only windows or a configured name earn
    /// survival, per `workspace_consider_destroy`,
    /// `sway/tree/workspace.c:313-330`.
    pub fn must_be_kept(&self) -> bool {
        self.has_windows() || self.persistent
    }
}

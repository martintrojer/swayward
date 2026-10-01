use super::query_state::{
    clear_workspace_focus, describe_input, find_focused_node, find_parent_of_node,
    find_workspace_by_id, find_workspace_by_tree_id,
};
use super::*;

mod legacy;
mod transaction;
mod windows;
mod workspaces;

#[derive(Default)]
pub(super) struct WorkspaceEventTransaction {
    pub(super) events: Vec<Event>,
    pub(super) suppress_workspace_moves: bool,
    pub(super) scratchpad: Option<ScratchpadEventOrder>,
}

#[derive(Clone, Copy)]
pub(crate) enum ScratchpadEventOrder {
    Hide,
    Show,
}
